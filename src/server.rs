//! Server state + the `LanguageServer` implementation (thin dispatch).

use std::collections::{HashMap, HashSet};
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicU64, Ordering},
};
use std::time::Instant;

use moka::future::Cache;
use ropey::Rope;
use tokio::sync::{Mutex, OnceCell, RwLock};

use tower_lsp_server::{
    Client, LanguageServer,
    jsonrpc::{self, Error},
    ls_types::{
        CompletionParams, CompletionResponse, ConfigurationItem, Diagnostic, DiagnosticOptions,
        DiagnosticServerCapabilities, DiagnosticSeverity, DidChangeConfigurationParams,
        DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
        DocumentDiagnosticParams, DocumentDiagnosticReportResult, DocumentFormattingParams,
        ExecuteCommandOptions, ExecuteCommandParams, FoldingRange, FoldingRangeParams,
        FoldingRangeProviderCapability, GotoDefinitionParams, GotoDefinitionResponse, Hover,
        HoverContents, HoverParams, HoverProviderCapability, InitializeParams, InitializeResult,
        InitializedParams, LSPAny, Location, LocationLink, MarkupContent, MarkupKind,
        NumberOrString, OneOf, Position, PrepareRenameResponse, Range, ReferenceParams,
        RenameParams, SemanticTokensParams, SemanticTokensResult, ServerCapabilities, ServerInfo,
        TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit,
        Uri, WorkspaceEdit,
    },
};
use yrepo::{CatalogIndex, Library, ReferenceIndex, Statement, StatementKind};

use crate::{
    client::{self, Diagnostics, Window, Workspace},
    completion,
    config::Config,
    convert, diagnostic,
    document::Document,
    fold, format, goto, hover, info, inst, references, schema_idx, semantic_token, warning,
    workspace,
};

#[derive(Clone)]
struct Snapshot {
    generation: u64,
    lib: Option<std::sync::Arc<Library>>,
    diags: Vec<yrepo::Diagnostic>,
}

/// Arguments of the `netconf/insertTemplate` command.
#[derive(serde::Deserialize)]
struct InsertTemplateArgs {
    uri: String,
    kind: String,
    position: Position,
}

pub(crate) struct Server {
    root_uri: OnceLock<Uri>,
    /// Open-closure yrepo repository: open buffers (full parse) plus the
    /// on-disk modules they can see (text-light parse) — see
    /// `docs/serving-large-trees.md`. Kept small by [`Server::sync_open_closure`].
    repo: RwLock<yrepo::Repository>,
    /// Header-only catalog of the whole on-disk workspace (`Catalog::scan`),
    /// built once by `ensure_scanned`. None until a workspace root exists.
    catalog: RwLock<Option<Arc<CatalogIndex>>>,
    /// Whole-tree reference index (`ReferenceIndex`), built lazily by
    /// `ensure_refidx` on the first whole-tree references request. Records
    /// definition/reference occurrences from every on-disk `.yang` file so a
    /// library symbol's usages in importing modules can be found without
    /// materializing those modules into the open-closure repository.
    refidx: RwLock<Option<Arc<ReferenceIndex>>>,
    /// Urls of the currently open YANG buffers (the roots of the closure).
    open_yang: RwLock<HashSet<String>>,
    docs: Cache<String, std::sync::Arc<Document>>,
    /// Live server configuration. A tokio `RwLock` (not a `OnceLock`) on
    /// purpose: the client pushes updates via `didChangeConfiguration`, so the
    /// stored value must be **replaceable** after the startup fetch — and it
    /// is read from async handlers, so it must not be a std blocking lock.
    config: RwLock<Config>,
    generation: AtomicU64,
    snap: RwLock<Option<Snapshot>>,
    scan: OnceCell<()>,
    /// Build guard for the whole-tree reference index (see [`Server::ensure_refidx`]).
    refidx_build: Mutex<()>,
}

impl Server {
    pub fn new(c: Client) -> Self {
        client::init(c);
        Self {
            root_uri: OnceLock::new(),
            repo: RwLock::new(yrepo::Repository::new()),
            catalog: RwLock::new(None),
            refidx: RwLock::new(None),
            open_yang: RwLock::new(HashSet::new()),
            docs: Cache::new(u32::MAX as u64), // unbounded; the client controls open buffers
            config: RwLock::new(Config::default()),
            generation: AtomicU64::new(0),
            snap: RwLock::new(None),
            scan: OnceCell::new(),
            refidx_build: Mutex::new(()),
        }
    }

    fn bump(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    async fn config(&self) -> Config {
        self.config.read().await.clone()
    }

    /// Store a fresh configuration (startup fetch or live `didChangeConfiguration`).
    async fn set_config(&self, config: Config) {
        *self.config.write().await = config;
    }

    async fn open_doc(&self, uri: &str) -> jsonrpc::Result<std::sync::Arc<Document>> {
        self.docs.get(uri).await.ok_or_else(Error::internal_error)
    }

    /// Rope for a document: the open buffer if present, otherwise the file on
    /// disk (used to map byte ranges of cross-file targets).
    async fn rope_for(&self, url: &str) -> Option<Rope> {
        if let Some(doc) = self.docs.get(url).await {
            return Some(doc.rope.clone());
        }
        let path = workspace::url_to_path(url)?;
        let text = std::fs::read_to_string(path).ok()?;
        Some(Rope::from_str(&text))
    }

    /// Store an open buffer's text (both YANG and instance documents live in
    /// the doc cache; only YANG is ever fed to the `yrepo` repository).
    async fn put_doc(&self, uri: &str, text: &str, version: i32) {
        self.docs
            .insert(
                uri.to_owned(),
                std::sync::Arc::new(Document::new(text, version)),
            )
            .await;
    }

    /// Feed a YANG open buffer into the repository (full parse) and pull in
    /// the on-disk modules it depends on, then invalidate the snapshot.
    async fn upsert_yang(&self, uri: &str, text: &str) {
        self.open_yang.write().await.insert(uri.to_owned());
        {
            let mut repo = self.repo.write().await;
            // Open buffers always keep full views (text-light OFF).
            repo.set_text_light(false);
            repo.upsert(uri, text);
        }
        self.bump();
        self.sync_open_closure().await;
    }

    /// Drop a closed YANG buffer from the repository; the on-disk document is
    /// re-materialized (text-light) by `sync_open_closure` when another open
    /// buffer still needs it.
    async fn revert_yang(&self, uri: &str) {
        self.open_yang.write().await.remove(uri);
        self.repo.write().await.remove(uri);
        self.bump();
        self.sync_open_closure().await;
    }

    /// Classify an open XML/JSON buffer against the compiled YANG library as a
    /// NETCONF instance document (M0 content-sniffing). Parsed on demand; a
    /// real parse cache lands with M1 when features consume the instance tree.
    async fn classify(&self, uri: &str) -> Option<inst::DocKind> {
        let text = self.rope_for(uri).await?.to_string();
        self.ensure_scanned().await;
        let modules = self
            .snapshot()
            .await
            .lib
            .as_deref()
            .map(schema_idx::module_summaries)
            .unwrap_or_default();
        match workspace::doc_lang(uri) {
            workspace::DocLang::Xml => Some(inst::classify_xml(
                &crate::xml::parse_root(&text)?,
                &modules,
            )),
            workspace::DocLang::Json => Some(inst::classify_json(
                &crate::json::parse_root(&text)?,
                &modules,
            )),
            _ => None,
        }
    }

    /// Log the intent of a freshly opened XML/JSON buffer — the observable M0
    /// outcome of content-sniffing (recognized vs dormant).
    async fn recognize(&self, uri: &str) {
        let Some(kind) = self.classify(uri).await else {
            return;
        };
        if kind == inst::DocKind::NotNetconf {
            return;
        }
        Window::log(info!(format!("netconf doc {uri}: {kind:?}"))).await;
    }

    /// The XML instance context for a doc: text rope, parsed element tree, and
    /// the compiled library (when present).
    async fn xml_ctx(
        &self,
        uri: &str,
    ) -> Option<(Rope, crate::xml::XmlDoc, std::sync::Arc<Library>)> {
        let rope = self.rope_for(uri).await?;
        let xdoc = crate::xml::parse(&rope.to_string())?;
        let snap = self.snapshot().await;
        let lib = snap.lib?;
        Some((rope, xdoc, lib))
    }

    /// M1 goto: map the element under the caret to its YANG `defining` node.
    async fn xml_goto_definition(
        &self,
        uri: &str,
        pos: Position,
    ) -> jsonrpc::Result<Option<GotoDefinitionResponse>> {
        let Some((rope, xdoc, lib)) = self.xml_ctx(uri).await else {
            return Ok(None);
        };
        let Some(byte) = convert::position_to_byte(&rope, pos) else {
            return Ok(None);
        };
        let Some(elem) = xdoc.element_at(byte) else {
            return Ok(None);
        };
        let map = crate::inst_map::map_doc(&xdoc, &lib);
        let Some(res) = map.resolved(elem) else {
            return Ok(None);
        };
        let Some(loc) = crate::inst_map::defining_of(&lib, res) else {
            return Ok(None);
        };
        let Some(target_rope) = self.rope_for(&loc.url).await else {
            return Ok(None);
        };
        let Some(target_uri) = loc.url.parse::<Uri>().ok() else {
            return Ok(None);
        };
        let origin_range = xdoc.nodes[elem].name_range.clone();
        let link = LocationLink {
            origin_selection_range: Some(convert::range_to_lsp(&rope, origin_range)),
            target_uri,
            target_range: convert::range_to_lsp(&target_rope, loc.range.clone()),
            target_selection_range: convert::range_to_lsp(&target_rope, loc.range),
        };
        Ok(Some(GotoDefinitionResponse::Link(vec![link])))
    }

    /// M1 hover: schema snippet + kind/type for the element under the caret.
    async fn xml_hover(&self, uri: &str, pos: Position) -> jsonrpc::Result<Option<Hover>> {
        let Some((rope, xdoc, lib)) = self.xml_ctx(uri).await else {
            return Ok(None);
        };
        let Some(byte) = convert::position_to_byte(&rope, pos) else {
            return Ok(None);
        };
        let Some(elem) = xdoc.element_at(byte) else {
            return Ok(None);
        };
        let map = crate::inst_map::map_doc(&xdoc, &lib);
        let Some(res) = map.resolved(elem) else {
            return Ok(None);
        };
        let Some(loc) = crate::inst_map::defining_of(&lib, res) else {
            return Ok(None);
        };
        let Some(target_rope) = self.rope_for(&loc.url).await else {
            return Ok(None);
        };
        let Some(snippet) = target_rope.get_byte_slice(loc.range.clone()) else {
            return Ok(None);
        };
        let Some(rec) = lib.module(&res.module) else {
            return Ok(None);
        };
        let Some(node) = rec.node(res.id) else {
            return Ok(None);
        };
        let mut md = format!(
            "```yang\n{}\n```\n\n`{}` **`{}`** (module `{}`)",
            snippet,
            node.kind().as_str(),
            node.name(),
            res.module
        );
        if let Some(t) = node.type_name() {
            md.push_str(&format!("\n- type: `{t}`"));
        }
        if !node.keys().is_empty() {
            md.push_str(&format!("\n- keys: {}", node.keys().join(", ")));
        }
        if node.is_mandatory() {
            md.push_str("\n- mandatory");
        }
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: md,
            }),
            range: None,
        }))
    }

    /// M1/M5 diagnostics: unknown element / wrong namespace / depth over the
    /// whole doc, plus leaf value validation (D31).
    async fn xml_diagnostics(&self, uri: &str, version: i32) -> DocumentDiagnosticReportResult {
        let Some((rope, xdoc, lib)) = self.xml_ctx(uri).await else {
            return diagnostic::report(version.to_string(), Vec::new());
        };
        let text = rope.to_string();
        let mut diags = crate::inst_map::map_doc(&xdoc, &lib).diags;
        diags.extend(crate::inst_map::value_diags(&xdoc, &text, &lib));
        let items: Vec<Diagnostic> = diags
            .iter()
            .map(|d| Diagnostic {
                range: convert::range_to_lsp(&rope, d.range.clone()),
                severity: Some(DiagnosticSeverity::ERROR),
                code: Some(NumberOrString::String(d.code.to_owned())),
                source: Some("netconf".to_owned()),
                message: d.message.clone(),
                ..Default::default()
            })
            .collect();
        diagnostic::report(version.to_string(), items)
    }

    /// The JSON (RFC 7951) instance context: text rope, parsed member tree,
    /// and the compiled library.
    async fn json_ctx(
        &self,
        uri: &str,
    ) -> Option<(Rope, crate::json::JsonDoc, std::sync::Arc<Library>)> {
        let rope = self.rope_for(uri).await?;
        let jdoc = crate::json::parse(&rope.to_string())?;
        let snap = self.snapshot().await;
        let lib = snap.lib?;
        Some((rope, jdoc, lib))
    }

    /// M3 goto: map the JSON member under the caret to its YANG `defining`.
    async fn json_goto_definition(
        &self,
        uri: &str,
        pos: Position,
    ) -> jsonrpc::Result<Option<GotoDefinitionResponse>> {
        let Some((rope, jdoc, lib)) = self.json_ctx(uri).await else {
            return Ok(None);
        };
        let Some(byte) = convert::position_to_byte(&rope, pos) else {
            return Ok(None);
        };
        let Some(member) = jdoc.member_at(byte) else {
            return Ok(None);
        };
        let map = crate::jmap::map(&jdoc, &lib);
        let Some(res) = map.resolved(member) else {
            return Ok(None);
        };
        let Some(loc) = crate::inst_map::defining_of(&lib, res) else {
            return Ok(None);
        };
        let Some(target_rope) = self.rope_for(&loc.url).await else {
            return Ok(None);
        };
        let Some(target_uri) = loc.url.parse::<Uri>().ok() else {
            return Ok(None);
        };
        let origin_range = jdoc.members[member].key_range.clone();
        let link = LocationLink {
            origin_selection_range: Some(convert::range_to_lsp(&rope, origin_range)),
            target_uri,
            target_range: convert::range_to_lsp(&target_rope, loc.range.clone()),
            target_selection_range: convert::range_to_lsp(&target_rope, loc.range),
        };
        Ok(Some(GotoDefinitionResponse::Link(vec![link])))
    }

    /// M3 hover: schema snippet + kind/type for the member under the caret.
    async fn json_hover(&self, uri: &str, pos: Position) -> jsonrpc::Result<Option<Hover>> {
        let Some((rope, jdoc, lib)) = self.json_ctx(uri).await else {
            return Ok(None);
        };
        let Some(byte) = convert::position_to_byte(&rope, pos) else {
            return Ok(None);
        };
        let Some(member) = jdoc.member_at(byte) else {
            return Ok(None);
        };
        let map = crate::jmap::map(&jdoc, &lib);
        let Some(res) = map.resolved(member) else {
            return Ok(None);
        };
        let Some(loc) = crate::inst_map::defining_of(&lib, res) else {
            return Ok(None);
        };
        let Some(target_rope) = self.rope_for(&loc.url).await else {
            return Ok(None);
        };
        let Some(snippet) = target_rope.get_byte_slice(loc.range.clone()) else {
            return Ok(None);
        };
        let Some(rec) = lib.module(&res.module) else {
            return Ok(None);
        };
        let Some(node) = rec.node(res.id) else {
            return Ok(None);
        };
        let mut md = format!(
            "```yang\n{}\n```\n\n`{}` **`{}`** (module `{}`)",
            snippet,
            node.kind().as_str(),
            node.name(),
            res.module
        );
        if let Some(t) = node.type_name() {
            md.push_str(&format!("\n- type: `{t}`"));
        }
        if !node.keys().is_empty() {
            md.push_str(&format!("\n- keys: {}", node.keys().join(", ")));
        }
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: md,
            }),
            range: None,
        }))
    }

    /// M3/M5 diagnostics: unknown member / wrong module / depth over the whole
    /// document, plus leaf value validation (D31).
    async fn json_diagnostics(&self, uri: &str, version: i32) -> DocumentDiagnosticReportResult {
        let Some((rope, jdoc, lib)) = self.json_ctx(uri).await else {
            return diagnostic::report(version.to_string(), Vec::new());
        };
        let text = rope.to_string();
        let mut diags = crate::jmap::map(&jdoc, &lib).diags;
        diags.extend(crate::jmap::value_diags(&jdoc, &text, &lib));
        let items: Vec<Diagnostic> = diags
            .iter()
            .map(|d| Diagnostic {
                range: convert::range_to_lsp(&rope, d.range.clone()),
                severity: Some(DiagnosticSeverity::ERROR),
                code: Some(NumberOrString::String(d.code.to_owned())),
                source: Some("netconf".to_owned()),
                message: d.message.clone(),
                ..Default::default()
            })
            .collect();
        diagnostic::report(version.to_string(), items)
    }

    /// Make sure the on-disk `.yang` workspace catalog has been built **once**
    /// before serving semantics. Runs lazily: whoever needs it first starts it
    /// (`initialized`, or an early `textDocument/diagnostic` pull that beats
    /// it), and every other caller waits for the same single build to finish.
    /// After the catalog exists, the repository is synced to the open closure
    /// so a first-open diagnostic never runs against a half-populated repo.
    async fn ensure_scanned(&self) {
        if self.root_uri.get().is_none() {
            return;
        }
        // Note: do NOT `self.scan.clone()` here — tokio's `OnceCell::clone`
        // returns an *independent* cell, so cloning would run the scan once per
        // call. Call `get_or_init` on `&self.scan` directly: concurrent callers
        // share the one cell and wait for the single init.
        self.scan
            .get_or_init(|| async {
                let start = Instant::now();
                self.fill_catalog().await;
                self.sync_open_closure().await;
                let duration = start.elapsed();
                Window::log(info!(format!(
                    "workspace catalog ready in {:.6}s",
                    duration.as_secs_f64()
                )))
                .await;
            })
            .await;
    }

    /// Make sure the whole-tree [`ReferenceIndex`] has been built before a
    /// whole-tree references search. Unlike [`Server::ensure_scanned`] this
    /// runs lazily (not on startup): the index is only needed by
    /// "find all references" on a symbol whose usages live in modules that
    /// merely *import* the open one, and it costs a full statement-walk of the
    /// tree, so the first such request shows a server→client progress bar
    /// covering the build. The index mirrors the *on-disk* workspace, so it is
    /// **rebuilt** whenever disk content may have changed — see
    /// [`Server::invalidate_refidx`], called after a whole-tree rename that
    /// rewrote non-open files. Returns `None` when there is no workspace root.
    async fn ensure_refidx(&self) -> Option<Arc<ReferenceIndex>> {
        if let Some(ix) = self.refidx.read().await.clone() {
            return Some(ix);
        }
        // Rebuildable single-flight guard: a rename can invalidate the index,
        // so this is not a one-shot OnceCell. Handlers run serially
        // (`concurrency_level(1)`), but keep the double-checked lock anyway.
        let _guard = self.refidx_build.lock().await;
        if let Some(ix) = self.refidx.read().await.clone() {
            return Some(ix);
        }
        let root = self.root_uri.get()?;
        let Some(root_path) = workspace::url_to_path(&root.to_string()) else {
            Window::log(warning!("refidx scan skipped: cannot resolve root path")).await;
            return None;
        };
        let files = workspace::walk_yang_files(&root_path);
        let total = files.len();
        let progress = client::Progress::work_done(
            "Searching references",
            format!("Indexing {total} YANG files …"),
        )
        .await;
        let start = Instant::now();
        let scanned = tokio::task::spawn_blocking(move || {
            let mut ix = ReferenceIndex::default();
            let n = ix.scan_many_files_with(&files, |p| {
                workspace::path_to_url(p).map(|u| workspace::canon_url(&u))
            });
            (ix, n)
        })
        .await
        .expect("refidx scan task panicked");
        let (ix, n) = scanned;
        let duration = start.elapsed();
        let occ = ix.len();
        *self.refidx.write().await = Some(Arc::new(ix));
        if let Some(progress) = progress {
            progress
                .report(format!(
                    "Indexed {n}/{total} YANG files ({occ} references)."
                ))
                .await;
            progress.finish().await;
        }
        Window::log(info!(format!(
            "workspace reference index ready in {:.3}s: {n}/{total} yang files, {occ} occurrences",
            duration.as_secs_f64()
        )))
        .await;
        self.refidx.read().await.clone()
    }

    /// Drop the whole-tree [`ReferenceIndex`] because the on-disk workspace
    /// may have changed (a rename/workspace edit rewrote files the cached
    /// index no longer reflects). The next whole-tree references request
    /// rebuilds it lazily from disk.
    async fn invalidate_refidx(&self) {
        *self.refidx.write().await = None;
    }

    /// Build the header-only catalog of every on-disk `.yang` file
    /// (`Catalog::scan`, transient parse, ~KB per file). No full parses, no
    /// repository ingest: this is what lets a giant tree be indexed without
    /// retaining per-file views. Documents are (re)parsed on demand when they
    /// enter an open closure.
    async fn fill_catalog(&self) {
        let Some(root) = self.root_uri.get() else {
            Window::log(warning!("scan skipped: no workspace root")).await;
            return;
        };
        let Some(root_path) = workspace::url_to_path(&root.to_string()) else {
            Window::log(warning!("scan skipped: cannot resolve root path")).await;
            return;
        };
        let files = workspace::walk_yang_files(&root_path);
        let total = files.len();
        // TEMP EXPERIMENT (uncommitted): rayon-driven parallel catalog scan.
        // yrepo's CatalogIndex::scan_many_files_with fans the read+Catalog::scan
        // out over the rayon global pool (sized to the machine's cores) and
        // takes the caller's url mapping (canonical file urls here), wrapped in
        // spawn_blocking so the async handler never blocks a runtime worker.
        // START → END elapsed is printed for hand A/B against the sequential build.
        let start = Instant::now();
        Window::log(info!(format!(
            "workspace catalog scan START: {total} yang files (parallel, rayon pool)"
        )))
        .await;
        let scanned = tokio::task::spawn_blocking(move || {
            let mut index = CatalogIndex::default();
            let n = index.scan_many_files_with(&files, |p| {
                workspace::path_to_url(p).map(|u| workspace::canon_url(&u))
            });
            (index, n)
        })
        .await
        .expect("catalog scan task panicked");
        let (index, n) = scanned;
        let duration = start.elapsed();
        let distinct = index.names().len();
        *self.catalog.write().await = Some(Arc::new(index));
        Window::log(info!(format!(
            "workspace catalog scan END after {:.3}s (parallel, rayon pool): scanned {n}/{total} yang files, {distinct} distinct module names",
            duration.as_secs_f64()
        )))
        .await;
    }

    /// Reconcile the repository with the OPEN CLOSURE: it must contain exactly
    /// the open buffers (full parse — done by their `upsert_yang`) plus every
    /// on-disk module they can reach through the catalog (imports, includes,
    /// and the belongs-to parent of an open submodule), parsed text-light.
    ///
    /// The sync is incremental and cheap when nothing changed: header seeds are
    /// re-read from the (already parsed) open buffers, the catalog closure is
    /// recomputed, documents that left the closure are dropped and missing
    /// ones are read from disk once. It must be called only from the did_open /
    /// did_change / did_close / scan paths — never while a caller holds the
    /// repository read lock.
    async fn sync_open_closure(&self) {
        let Some(catalog) = self.catalog.read().await.clone() else {
            return; // no workspace catalog yet (no root, or scan not run)
        };
        let open: Vec<String> = self.open_yang.read().await.iter().cloned().collect();
        let mut repo = self.repo.write().await;
        // Seeds: the cross-file dependencies each open buffer declares.
        let mut seeds: Vec<crate::closure::Seed> = Vec::new();
        for url in &open {
            if let Some(root) = repo.statement(url) {
                seeds.extend(crate::closure::header_seeds(root));
            }
        }
        let mut needed = crate::closure::closure_urls(&catalog, &seeds);
        for url in &open {
            needed.insert(url.clone());
        }
        let mut changed = false;
        // Drop documents that are no longer reachable from any open buffer.
        let urls: Vec<String> = repo.urls().into_iter().map(str::to_owned).collect();
        for url in urls {
            if !needed.contains(&url) {
                repo.remove(&url);
                changed = true;
            }
        }
        // Materialize on-disk documents the open closure needs (text-light:
        // their views only feed schema resolution, never open-buffer features).
        repo.set_text_light(true);
        for url in &needed {
            if repo.contains(url) {
                continue;
            }
            if let Some(text) = Self::disk_text(url) {
                repo.upsert(url.clone(), text);
                changed = true;
            }
        }
        repo.set_text_light(false);
        if changed {
            self.bump();
        }
    }

    /// Read a canonical url's file text from disk (used for closure members).
    fn disk_text(url: &str) -> Option<String> {
        workspace::url_to_path(url).and_then(|p| std::fs::read_to_string(p).ok())
    }

    /// Compile the repository (cached by generation). Re-compiles only when a
    /// document changed since the last snapshot.
    async fn snapshot(&self) -> Snapshot {
        let generation = self.generation.load(Ordering::Relaxed);
        {
            let snap = self.snap.read().await;
            if let Some(s) = snap.as_ref()
                && s.generation == generation
            {
                return s.clone();
            }
        }
        let outcome = self.repo.read().await.compile();
        let snap = Snapshot {
            generation,
            lib: outcome.library,
            diags: outcome.diagnostics,
        };
        *self.snap.write().await = Some(snap.clone());
        snap
    }

    /// The module (for prefix maps / symbol lookup) a document's root belongs
    /// to: the module's own name, or the `belongs-to` parent for a submodule.
    fn module_scope(root: &Statement) -> Option<String> {
        use StatementKind as K;
        match root.kind {
            K::Submodule => root
                .find_one(K::BelongsTo)
                .and_then(|b| b.arg.as_ref())
                .map(|a| a.name().to_owned()),
            _ => root.arg.as_ref().map(|a| a.name().to_owned()),
        }
    }

    async fn caret_byte(&self, uri: &str, pos: Position) -> jsonrpc::Result<usize> {
        let doc = self.open_doc(uri).await?;
        convert::position_to_byte(&doc.rope, pos).ok_or_else(Error::internal_error)
    }
}

/// True when `source` does not even parse as a YANG module/submodule
/// (parse error or not a YANG document). Used to keep formatting safe.
fn syntax_broken(source: &str) -> bool {
    let mut repo = yrepo::Repository::new();
    repo.upsert("_check.yang", source);
    repo.compile().diagnostics.iter().any(|d| {
        matches!(
            &d.code,
            yrepo::DiagnosticCode::ParseError | yrepo::DiagnosticCode::NotYangDocument
        )
    })
}

/// The byte range of the identifier token containing `byte` in `rope`
/// (rename `prepare` placeholder; identifiers may contain `-`, `_`, `.`).
fn range_for_word(rope: &Rope, byte: usize) -> std::ops::Range<usize> {
    let len = rope.len_bytes();
    if byte >= len {
        return byte..byte;
    }
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.';
    let text = rope.to_string();
    let as_bytes = text.as_bytes();
    let mut start = byte;
    let mut end = byte;
    while start > 0 && is_word(as_bytes[start - 1]) {
        start -= 1;
    }
    while end < len && is_word(as_bytes[end]) {
        end += 1;
    }
    start..end
}

fn is_valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

impl LanguageServer for Server {
    async fn initialize(&self, params: InitializeParams) -> jsonrpc::Result<InitializeResult> {
        #[allow(deprecated)]
        if let Some(uri) = params.root_uri {
            let _ = self.root_uri.set(uri);
        }

        // Run the initial workspace scan + closure sync here, blocking the
        // `initialize` response until it finishes. tower-lsp can never emit a
        // client progress during `initialize` (it drops pre-initialize
        // notifications), so the visible progress bar is shown by the extension
        // while it awaits `client.start()` — which only resolves once this
        // response is out — making the bar cover exactly the scan duration. It
        // also keeps the first diagnostic pull from racing a half-built catalog.
        self.ensure_scanned().await;

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "netconf-language-server".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                semantic_tokens_provider: Some(semantic_token::capability()),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(completion::capability()),
                diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
                    DiagnosticOptions::default(),
                )),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec!["netconf/insertTemplate".to_owned()],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        Window::log(info!("[netconf-language-server] initialized.")).await;

        // Fetch configuration now that the server is initialized. tower-lsp
        // rejects client *requests* (workspace/configuration etc.) until the
        // initialize response has been sent, so a fetch inside `initialize` was
        // silently failing and config stayed at its default.
        if let Some(uri) = self.root_uri.get() {
            let item = ConfigurationItem {
                scope_uri: Some(uri.clone()),
                section: Some("netconf".to_owned()),
            };
            if let Ok(values) = Workspace::configuration(vec![item]).await
                && let Some(value) = values.into_iter().next()
                && let Ok(config) = serde_json::from_value::<Config>(value)
            {
                Window::log(info!(format!("config: {:?}", config))).await;
                self.set_config(config).await;
            }
        }
    }

    async fn shutdown(&self) -> jsonrpc::Result<()> {
        Window::log(info!("[netconf-language-server] shutdown")).await;
        Ok(())
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        let Ok(config) = serde_json::from_value::<Config>(params.settings) else {
            Window::log(warning!(
                "didChangeConfiguration: could not parse netconf settings"
            ))
            .await;
            return;
        };
        let semantic_changed = self.config().await.semantic != config.semantic;
        self.set_config(config).await;
        Window::log(info!(format!(
            "config updated: {:?} (semantic {})",
            self.config().await,
            if semantic_changed {
                "changed"
            } else {
                "unchanged"
            }
        )))
        .await;
        // A classification change only shows once open documents re-request
        // semantic tokens: ask the client to refresh them now, otherwise the
        // new `netconf.semantic` roles would not appear until a document is
        // edited or reopened.
        if semantic_changed {
            crate::client::Semantics::refresh();
        }
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = workspace::canon_url(&params.text_document.uri.to_string());
        let version = params.text_document.version;
        let text = params.text_document.text;
        Window::log(info!(format!("did_open: {uri}"))).await;
        self.put_doc(&uri, &text, version).await;
        match workspace::doc_lang(&uri) {
            workspace::DocLang::Yang => {
                self.upsert_yang(&uri, &text).await;
                // A newly opened module can satisfy imports of already-open docs.
                Diagnostics::refresh();
            }
            // XML/JSON are never fed to `yrepo`; just observe their intent (M0).
            workspace::DocLang::Xml | workspace::DocLang::Json => self.recognize(&uri).await,
            workspace::DocLang::Other => {}
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = workspace::canon_url(&params.text_document.uri.to_string());
        for change in &params.content_changes {
            if change.range.is_some() {
                Window::log(warning!("unsupported incremental change")).await;
            } else {
                self.put_doc(&uri, &change.text, params.text_document.version)
                    .await;
                if workspace::is_yang(&uri) {
                    self.upsert_yang(&uri, &change.text).await;
                }
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = workspace::canon_url(&params.text_document.uri.to_string());
        Window::log(info!(format!("did_close: {uri}"))).await;
        self.docs.remove(&uri).await;
        if workspace::is_yang(&uri) {
            self.revert_yang(&uri).await;
            // A module going away may break other documents' imports.
            Diagnostics::refresh();
        }
    }

    /// Insert a NETCONF skeleton at the caret (M2 templates). Invoked by the
    /// client as `workspace/executeCommand` with `{uri, kind, position}`.
    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> jsonrpc::Result<Option<LSPAny>> {
        if params.command != "netconf/insertTemplate" {
            return Ok(None);
        }
        let Some(arg) = params.arguments.first() else {
            return Ok(None);
        };
        let Ok(args) = serde_json::from_value::<InsertTemplateArgs>(arg.clone()) else {
            return Ok(None);
        };
        let Some(new_text) = crate::template::skeleton(&args.kind) else {
            return Ok(None);
        };
        let Ok(uri) = args.uri.parse::<Uri>() else {
            return Ok(None);
        };
        let position = args.position;
        let edit = WorkspaceEdit {
            changes: Some(HashMap::from([(
                uri,
                vec![TextEdit {
                    range: Range {
                        start: position,
                        end: position,
                    },
                    new_text: new_text.to_owned(),
                }],
            )])),
            ..Default::default()
        };
        client::Edits::apply(edit).await;
        Ok(None)
    }

    async fn references(&self, params: ReferenceParams) -> jsonrpc::Result<Option<Vec<Location>>> {
        let tdp = &params.text_document_position;
        let uri = workspace::canon_url(&tdp.text_document.uri.to_string());
        if !workspace::is_yang(&uri) {
            Window::log(warning!(format!("references: not a YANG doc: {uri}"))).await;
            return Ok(None);
        }
        self.ensure_scanned().await;
        let byte = match self.caret_byte(&uri, tdp.position).await {
            Ok(b) => b,
            Err(_) => {
                Window::log(warning!(format!(
                    "references: failed to get caret byte for URI: {uri}"
                )))
                .await;
                return Ok(None);
            }
        };
        let include_decl = params.context.include_declaration;
        let started = Instant::now();
        let Some(rope) = self.rope_for(&uri).await else {
            Window::log(warning!(format!("references: no rope for URI: {uri}"))).await;
            return Ok(None);
        };
        let (def, mut hits) = {
            let repo = self.repo.read().await;
            let Some(root) = repo.statement(&uri) else {
                Window::log(warning!(format!("references: no statement for URI: {uri}"))).await;
                return Ok(None);
            };
            let Some(scope) = Self::module_scope(root) else {
                Window::log(warning!(format!("references: no scope for URI: {uri}"))).await;
                return Ok(None);
            };
            let caret_word = rope
                .get_byte_slice(range_for_word(&rope, byte))
                .map(|s| s.to_string())
                .unwrap_or_default();
            let snap = self.snapshot().await;
            let Some(lib) = snap.lib.as_ref() else {
                Window::log(warning!(format!("references: no library for URI: {uri}"))).await;
                return Ok(None);
            };
            let Some(def) = references::def_at(&rope, root, byte, &scope, lib) else {
                // Say *where* the caret landed so "no definition found" is
                // diagnosable: e.g. prose inside a `description` that merely
                // contains the word (not a reference) vs a real name the
                // engine does not know how to resolve.
                let ctx = root
                    .narrowest_at(byte)
                    .map(|s| {
                        let spot = if s.keyword.as_ref().is_some_and(|k| k.contains(&byte)) {
                            "keyword"
                        } else if s.arg.as_ref().is_some_and(|a| a.range.contains(&byte)) {
                            "argument"
                        } else {
                            "body"
                        };
                        format!("{:?} ({spot})", s.kind)
                    })
                    .unwrap_or_else(|| "no statement".to_owned());
                Window::log(warning!(format!(
                    "references: no definition found at '{caret_word}' in module '{scope}' (nearest statement {ctx}) for URI: {uri}"
                )))
                .await;
                return Ok(None);
            };
            Window::log(info!(format!(
                "references: caret on '{caret_word}' in module '{scope}' → {{ module: {}, local: {} }} (include_declaration={include_decl}) for URI: {uri}",
                def.module, def.local
            )))
            .await;
            let mut urls: Vec<String> = Vec::new();
            for m in lib.modules() {
                for u in m.source_urls() {
                    let s = u.to_string();
                    if !urls.contains(&s) {
                        urls.push(s);
                    }
                }
            }
            for sm in lib.submodules() {
                let s = sm.url().to_string();
                if !urls.contains(&s) {
                    urls.push(s);
                }
            }
            let docs: Vec<(String, &Statement, String)> = urls
                .iter()
                .filter_map(|u| {
                    let st = repo.statement(u)?;
                    let sc = Self::module_scope(st)?;
                    Some((u.clone(), st, sc))
                })
                .collect();
            Window::log(info!(format!(
                "references: open-closure search over {} docs for URI: {uri}",
                docs.len()
            )))
            .await;
            let closure = references::find_references(&def, &docs, lib, include_decl);
            Window::log(info!(format!(
                "references: open closure found {} hits for URI: {uri}",
                closure.len()
            )))
            .await;
            (def, closure)
        };
        // Whole-tree search: usages in every on-disk module that imports the
        // definition's module. Those modules are never materialized into the
        // open closure, so the closure search above cannot see them; the
        // whole-tree ReferenceIndex answers them from its recorded
        // occurrences. Open buffers are covered by the closure search (live
        // text), so the index only adds on-disk documents.
        if let Some(ix) = self.ensure_refidx().await {
            let open: HashSet<String> = self.open_yang.read().await.iter().cloned().collect();
            let mut added = 0usize;
            for (u, range) in ix.references(&def.module, &def.local, include_decl) {
                let s = u.to_string();
                if open.contains(&s) || hits.iter().any(|(hu, hr)| hu == &s && hr == &range) {
                    continue;
                }
                hits.push((s, range));
                added += 1;
            }
            Window::log(info!(format!(
                "references: whole-tree index ({} docs) added {added} hits for {}:{} for URI: {uri}",
                ix.doc_count(),
                def.module,
                def.local
            )))
            .await;
        }
        if hits.is_empty() {
            Window::log(info!(format!(
                "references: no references found for URI: {uri}"
            )))
            .await;
            return Ok(Some(Vec::new()));
        }
        let mut out = Vec::with_capacity(hits.len());
        for (u, range) in hits {
            let Some(rrope) = self.rope_for(&u).await else {
                continue;
            };
            let Ok(luri) = u.parse::<Uri>() else {
                continue;
            };
            out.push(Location {
                uri: luri,
                range: convert::range_to_lsp(&rrope, range),
            });
        }
        Window::log(info!(format!(
            "references: found {} references for URI: {uri} in {:.1} ms",
            out.len(),
            started.elapsed().as_secs_f64() * 1e3
        )))
        .await;
        Ok(Some(out))
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> jsonrpc::Result<Option<PrepareRenameResponse>> {
        let uri = workspace::canon_url(&params.text_document.uri.to_string());
        if !workspace::is_yang(&uri) {
            return Ok(None);
        }
        let Some(rope) = self.rope_for(&uri).await else {
            return Ok(None);
        };
        let byte = self.caret_byte(&uri, params.position).await?;
        let (def, range) = {
            let repo = self.repo.read().await;
            let Some(root) = repo.statement(&uri) else {
                return Ok(None);
            };
            let Some(scope) = Self::module_scope(root) else {
                return Ok(None);
            };
            let snap = self.snapshot().await;
            let Some(lib) = snap.lib.as_ref() else {
                return Ok(None);
            };
            let Some(def) = references::def_at(&rope, root, byte, &scope, lib) else {
                return Ok(None);
            };
            (def, byte)
        };
        let local = references::local_name_range(&rope, range_for_word(&rope, range));
        Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
            range: convert::range_to_lsp(&rope, local),
            placeholder: def.local,
        }))
    }

    async fn rename(&self, params: RenameParams) -> jsonrpc::Result<Option<WorkspaceEdit>> {
        let tdp = &params.text_document_position;
        let uri = workspace::canon_url(&tdp.text_document.uri.to_string());
        if !workspace::is_yang(&uri) {
            return Ok(None);
        }
        if !is_valid_identifier(&params.new_name) {
            return Err(jsonrpc::Error::invalid_params(format!(
                "invalid symbol name '{}'",
                params.new_name
            )));
        }
        self.ensure_scanned().await;
        let byte = self.caret_byte(&uri, tdp.position).await?;
        let Some(rope) = self.rope_for(&uri).await else {
            return Ok(None);
        };
        let (module, local, mut hits) = {
            let repo = self.repo.read().await;
            let Some(root) = repo.statement(&uri) else {
                return Ok(None);
            };
            let Some(scope) = Self::module_scope(root) else {
                return Ok(None);
            };
            let snap = self.snapshot().await;
            let Some(lib) = snap.lib.as_ref() else {
                return Ok(None);
            };
            let Some(def) = references::def_at(&rope, root, byte, &scope, lib) else {
                return Ok(None);
            };
            let mut urls: Vec<String> = Vec::new();
            for m in lib.modules() {
                for u in m.source_urls() {
                    let s = u.to_string();
                    if !urls.contains(&s) {
                        urls.push(s);
                    }
                }
            }
            for sm in lib.submodules() {
                let s = sm.url().to_string();
                if !urls.contains(&s) {
                    urls.push(s);
                }
            }
            let docs: Vec<(String, &Statement, String)> = urls
                .iter()
                .filter_map(|u| {
                    let st = repo.statement(u)?;
                    let sc = Self::module_scope(st)?;
                    Some((u.clone(), st, sc))
                })
                .collect();
            let hits = references::find_references(&def, &docs, lib, true);
            (def.module.clone(), def.local.clone(), hits)
        };
        // Whole-tree rename: also rewrite usages in every on-disk module that
        // imports the definition's module (those are never materialized in the
        // open closure). Open buffers are covered by the closure search above
        // (live text), so the index only adds on-disk documents.
        if let Some(ix) = self.ensure_refidx().await {
            let open: HashSet<String> = self.open_yang.read().await.iter().cloned().collect();
            let mut added = 0usize;
            for (u, range) in ix.references(&module, &local, true) {
                let s = u.to_string();
                if open.contains(&s) || hits.iter().any(|(hu, hr)| hu == &s && hr == &range) {
                    continue;
                }
                hits.push((s, range));
                added += 1;
            }
            Window::log(info!(format!(
                "rename: whole-tree index ({} docs) added {added} sites for {}:{} for URI: {uri}",
                ix.doc_count(),
                module,
                local
            )))
            .await;
        }
        if hits.is_empty() {
            return Ok(None);
        }
        let mut changes: HashMap<String, Vec<TextEdit>> = HashMap::new();
        for (u, full) in hits {
            let Some(rrope) = self.rope_for(&u).await else {
                continue;
            };
            let local = references::local_name_range(&rrope, full);
            changes.entry(u.clone()).or_default().push(TextEdit {
                range: convert::range_to_lsp(&rrope, local),
                new_text: params.new_name.clone(),
            });
        }
        let mut uri_map: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
        for (u, edits) in changes {
            if let Ok(lu) = u.parse::<Uri>() {
                uri_map.insert(lu, edits);
            }
        }
        // A successful rename rewrites files on disk (open buffers now carry
        // the new name, and whole-tree sites land in modules the cached
        // ReferenceIndex no longer reflects), so drop the whole-tree index:
        // the next whole-tree references request rebuilds it from the updated
        // disk. The client applies the edit asynchronously, so do NOT rebuild
        // here — just invalidate and let the next request re-read the disk.
        self.invalidate_refidx().await;
        Ok(Some(WorkspaceEdit {
            changes: Some(uri_map),
            ..Default::default()
        }))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> jsonrpc::Result<Option<SemanticTokensResult>> {
        let uri = workspace::canon_url(&params.text_document.uri.to_string());
        // Highlight stays YANG-only so built-in XML/JSON coloring is untouched.
        if !workspace::is_yang(&uri) {
            return Ok(None);
        }
        let doc = self.open_doc(&uri).await?;
        let config = self.config().await;
        let repo = self.repo.read().await;
        let root = repo.statement(&uri);
        let tokens = repo.tokens(&uri).unwrap_or(&[]);
        let data =
            semantic_token::handle(&doc.rope, root, tokens, &config.semantic).unwrap_or_default();
        let version = doc.version;
        drop(repo);
        Ok(Some(semantic_token::result(data, version)))
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> jsonrpc::Result<Option<Vec<FoldingRange>>> {
        let uri = workspace::canon_url(&params.text_document.uri.to_string());
        // Folding stays YANG-only (built-in XML/JSON providers handle the rest).
        if !workspace::is_yang(&uri) {
            return Ok(None);
        }
        let doc = self.open_doc(&uri).await?;
        let repo = self.repo.read().await;
        let root = repo.statement(&uri);
        let ranges = fold::handle(&doc.rope, root);
        drop(repo);
        Ok(Some(ranges))
    }

    async fn formatting(
        &self,
        params: DocumentFormattingParams,
    ) -> jsonrpc::Result<Option<Vec<TextEdit>>> {
        let uri = workspace::canon_url(&params.text_document.uri.to_string());
        // Formatting stays YANG-only (built-in XML/JSON providers handle the rest).
        if !workspace::is_yang(&uri) {
            return Ok(None);
        }
        let doc = self.open_doc(&uri).await?;

        // Never rewrite syntactically broken YANG: the regenerator could drop
        // or re-shape error-recovered content and the result would then fail
        // to parse (e.g. an error over the whole document).
        let source = doc.rope.to_string();
        if syntax_broken(&source) {
            Window::log(warning!("formatting skipped: document has syntax errors")).await;
            return Ok(None);
        }

        let indent = self.config().await.indent_width();
        let formatted = {
            let repo = self.repo.read().await;
            let root = repo.statement(&uri);
            let comments = repo.comments(&uri).unwrap_or(&[]);
            format::handle(&doc.rope, root, comments, indent)
        };
        let Some(new_text) = formatted else {
            return Ok(None);
        };

        // Guard: applying the reformatted text must keep the document parseable.
        if syntax_broken(&new_text) {
            Window::log(warning!("formatting skipped: result would not parse")).await;
            return Ok(None);
        }

        let full = Range {
            start: Position {
                line: 0,
                character: 0,
            },
            end: convert::byte_to_position(&doc.rope, doc.rope.len_bytes()),
        };
        Ok(Some(vec![TextEdit {
            range: full,
            new_text,
        }]))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> jsonrpc::Result<Option<GotoDefinitionResponse>> {
        let uri = workspace::canon_url(
            &params
                .text_document_position_params
                .text_document
                .uri
                .to_string(),
        );
        // Non-YANG docs: XML/JSON instance read features (M1/M3).
        if !workspace::is_yang(&uri) {
            let pos = params.text_document_position_params.position;
            return match workspace::doc_lang(&uri) {
                workspace::DocLang::Xml => self.xml_goto_definition(&uri, pos).await,
                workspace::DocLang::Json => self.json_goto_definition(&uri, pos).await,
                _ => Ok(None),
            };
        }
        let pos = params.text_document_position_params.position;
        let byte = self.caret_byte(&uri, pos).await?;

        let (targets, source_rope) = {
            let repo = self.repo.read().await;
            let Some(root) = repo.statement(&uri) else {
                return Ok(None);
            };
            let Some(scope) = Self::module_scope(root) else {
                return Ok(None);
            };
            let snap = self.snapshot().await;
            let Some(lib) = snap.lib.as_ref() else {
                return Ok(None);
            };
            let doc = self.open_doc(&uri).await?;
            let targets =
                goto::resolve(&doc.rope, root, &uri, byte, &scope, lib).unwrap_or_default();
            (targets, doc.rope.clone())
        };

        if targets.is_empty() {
            return Ok(None);
        }
        // Fetch each target file's text (open buffer or disk) once.
        let mut textmap: HashMap<String, Rope> = HashMap::new();
        for t in &targets {
            if !textmap.contains_key(&t.url)
                && let Some(rope) = self.rope_for(&t.url).await
            {
                textmap.insert(t.url.clone(), rope);
            }
        }
        let links: Vec<LocationLink> = goto::to_links(&source_rope, &targets, &textmap);
        if links.is_empty() {
            return Ok(None);
        }
        Ok(Some(GotoDefinitionResponse::Link(links)))
    }

    async fn hover(&self, params: HoverParams) -> jsonrpc::Result<Option<Hover>> {
        let uri = workspace::canon_url(
            &params
                .text_document_position_params
                .text_document
                .uri
                .to_string(),
        );
        // Non-YANG docs: XML/JSON instance read features (M1/M3).
        if !workspace::is_yang(&uri) {
            let pos = params.text_document_position_params.position;
            return match workspace::doc_lang(&uri) {
                workspace::DocLang::Xml => self.xml_hover(&uri, pos).await,
                workspace::DocLang::Json => self.json_hover(&uri, pos).await,
                _ => Ok(None),
            };
        }
        let pos = params.text_document_position_params.position;
        let byte = self.caret_byte(&uri, pos).await?;
        let doc = self.open_doc(&uri).await?;

        let markdown = {
            let repo = self.repo.read().await;
            let Some(root) = repo.statement(&uri) else {
                return Ok(None);
            };
            let Some(scope) = Self::module_scope(root) else {
                return Ok(None);
            };
            let snap = self.snapshot().await;
            let Some(lib) = snap.lib.as_ref() else {
                return Ok(None);
            };
            hover::handle(&doc.rope, root, byte, &scope, lib)
        };
        Ok(markdown.map(|value| Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value,
            }),
            range: None,
        }))
    }

    async fn completion(
        &self,
        params: CompletionParams,
    ) -> jsonrpc::Result<Option<CompletionResponse>> {
        let uri =
            workspace::canon_url(&params.text_document_position.text_document.uri.to_string());
        if !workspace::is_yang(&uri) {
            // Instance writing: XML (M2) and JSON (M4) completion.
            let pos = params.text_document_position.position;
            let Some(rope) = self.rope_for(&uri).await else {
                return Ok(None);
            };
            let Some(byte) = convert::position_to_byte(&rope, pos) else {
                return Ok(None);
            };
            let snap = self.snapshot().await;
            let Some(lib) = snap.lib.as_ref() else {
                return Ok(None);
            };
            let text = rope.to_string();
            let items = match workspace::doc_lang(&uri) {
                workspace::DocLang::Xml => crate::xcomp::handle(&text, byte, lib),
                workspace::DocLang::Json => crate::jcomp::handle(&text, byte, lib),
                _ => Vec::new(),
            };
            return Ok(Some(CompletionResponse::Array(items)));
        }
        let pos = params.text_document_position.position;
        let byte = self.caret_byte(&uri, pos).await?;
        let Some(rope) = self.rope_for(&uri).await else {
            return Ok(None);
        };
        let repo = self.repo.read().await;
        let Some(root) = repo.statement(&uri) else {
            return Ok(None);
        };
        let Some(scope) = Self::module_scope(root) else {
            return Ok(None);
        };
        let snap = self.snapshot().await;
        let Some(lib) = snap.lib.as_ref() else {
            return Ok(None);
        };
        let items = completion::handle(root, &rope, byte, &scope, lib, &params);
        drop(repo);
        Ok(items.map(CompletionResponse::Array))
    }

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> jsonrpc::Result<DocumentDiagnosticReportResult> {
        let uri = workspace::canon_url(&params.text_document.uri.to_string());
        // XML/JSON instance docs pull `netconf` diagnostics (M1/M3); other
        // non-YANG docs stay dormant (empty).
        if !workspace::is_yang(&uri) {
            let version = self.open_doc(&uri).await.map(|d| d.version).unwrap_or(0);
            return Ok(match workspace::doc_lang(&uri) {
                workspace::DocLang::Xml => self.xml_diagnostics(&uri, version).await,
                workspace::DocLang::Json => self.json_diagnostics(&uri, version).await,
                _ => diagnostic::report(version.to_string(), Vec::new()),
            });
        }
        // Wait for (or run) the initial workspace scan so the very first pull
        // already sees the whole module set — no transient import errors.
        self.ensure_scanned().await;
        let snap = self.snapshot().await;
        let generation = snap.generation;
        let rope = self
            .rope_for(&uri)
            .await
            .unwrap_or_else(|| Rope::from_str(""));

        let mut items = diagnostic::convert(&rope, &snap.diags, &uri);

        // LS-side checks (conflict prefix).
        let repo = self.repo.read().await;
        let root = repo.statement(&uri);
        items.extend(diagnostic::conflict_prefix(&rope, root));
        drop(repo);

        Ok(diagnostic::report(generation.to_string(), items))
    }
}
