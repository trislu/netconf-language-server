//! Server state + the `LanguageServer` implementation (thin dispatch).

use std::collections::HashMap;
use std::sync::{
    OnceLock,
    atomic::{AtomicU64, Ordering},
};

use moka::future::Cache;
use ropey::Rope;
use tokio::sync::RwLock;
use tower_lsp_server::{
    Client, LanguageServer,
    jsonrpc::{self, Error},
    ls_types::{
        CompletionParams, CompletionResponse, ConfigurationItem, DiagnosticOptions,
        DiagnosticServerCapabilities, DidChangeConfigurationParams, DidChangeTextDocumentParams,
        DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentDiagnosticParams,
        DocumentDiagnosticReportResult, DocumentFormattingParams, FoldingRange,
        FoldingRangeParams, FoldingRangeProviderCapability, GotoDefinitionParams,
        GotoDefinitionResponse, Hover, HoverContents, HoverParams, InitializeParams,
        InitializeResult, InitializedParams, HoverProviderCapability, LocationLink,
        MarkupContent, MarkupKind, OneOf, Position, Range, SemanticTokensParams, SemanticTokensResult, ServerCapabilities,
        ServerInfo, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri,
    },
};
use yrepo::{Library, Statement, StatementKind};

use crate::{
    client::{self, Window, Workspace},
    completion, config::Config, convert, diagnostic, document::Document, fold, format, goto,
    hover, info, semantic_token, warning, workspace,
};

#[derive(Clone)]
struct Snapshot {
    generation: u64,
    lib: Option<std::sync::Arc<Library>>,
    diags: Vec<yrepo::Diagnostic>,
}

pub(crate) struct Server {
    root_uri: OnceLock<Uri>,
    repo: RwLock<yrepo::Repository>,
    docs: Cache<String, std::sync::Arc<Document>>,
    config: OnceLock<Config>,
    generation: AtomicU64,
    snap: RwLock<Option<Snapshot>>,
}

impl Server {
    pub fn new(c: Client) -> Self {
        client::init(c);
        Self {
            root_uri: OnceLock::new(),
            repo: RwLock::new(yrepo::Repository::new()),
            docs: Cache::new(4096),
            config: OnceLock::new(),
            generation: AtomicU64::new(0),
            snap: RwLock::new(None),
        }
    }

    fn bump(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    fn config(&self) -> Config {
        self.config.get().cloned().unwrap_or_default()
    }

    async fn open_doc(&self, uri: &str) -> jsonrpc::Result<std::sync::Arc<Document>> {
        self.docs
            .get(uri)
            .await
            .ok_or_else(Error::internal_error)
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

    async fn upsert_doc(&self, uri: &str, text: &str, version: i32) {
        self.docs
            .insert(uri.to_owned(), std::sync::Arc::new(Document::new(text, version)))
            .await;
        self.repo.write().await.upsert(uri, text);
        self.bump();
    }

    async fn close_doc(&self, uri: &str) {
        self.docs.remove(uri).await;
        // Prefer the on-disk content after close; remove if the file is gone.
        let mut repo = self.repo.write().await;
        match workspace::url_to_path(uri).and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(text) => repo.upsert(uri, text),
            None => {
                repo.remove(uri);
            }
        }
        drop(repo);
        self.bump();
    }

    /// Scan the workspace and upsert every on-disk `.yang` file that is not an
    /// open (dirty) buffer.
    async fn scan_workspace(&self) {
        let Some(root) = self.root_uri.get() else {
            return;
        };
        let Some(root_path) = workspace::url_to_path(&root.to_string()) else {
            return;
        };
        for path in workspace::walk_yang_files(&root_path) {
            let Some(url) = workspace::path_to_url(&path) else {
                continue;
            };
            // Skip files that have an open buffer (buffer text wins).
            if self.docs.contains_key(&url) {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(&path) {
                self.repo.write().await.upsert(url, text);
            }
        }
        self.bump();
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

impl LanguageServer for Server {
    async fn initialize(&self, params: InitializeParams) -> jsonrpc::Result<InitializeResult> {
        #[allow(deprecated)]
        if let Some(uri) = params.root_uri {
            let _ = self.root_uri.set(uri);
        }
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "netconf-lsp".to_owned(),
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
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(completion::capability()),
                diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
                    DiagnosticOptions::default(),
                )),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        Window::log(info!("[netconf-lsp] initialized.")).await;
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
                let _ = self.config.set(config);
            }
            self.scan_workspace().await;
        }
    }

    async fn shutdown(&self) -> jsonrpc::Result<()> {
        Window::log(info!("[netconf-lsp] shutdown")).await;
        Ok(())
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        if let Ok(config) = serde_json::from_value::<Config>(params.settings) {
            let _ = self.config.set(config);
        }
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        Window::log(info!(format!("did_open: {uri}"))).await;
        self.upsert_doc(&uri, &params.text_document.text, params.text_document.version)
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        for change in &params.content_changes {
            if change.range.is_some() {
                Window::log(warning!("unsupported incremental change")).await;
            } else {
                self.upsert_doc(&uri, &change.text, params.text_document.version)
                    .await;
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        Window::log(info!(format!("did_close: {uri}"))).await;
        self.close_doc(&uri).await;
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> jsonrpc::Result<Option<SemanticTokensResult>> {
        let uri = params.text_document.uri.to_string();
        let doc = self.open_doc(&uri).await?;
        let repo = self.repo.read().await;
        let root = repo.statement(&uri);
        let tokens = repo.tokens(&uri).unwrap_or(&[]);
        let data = semantic_token::handle(&doc.rope, root, tokens).unwrap_or_default();
        let version = doc.version;
        drop(repo);
        Ok(Some(semantic_token::result(data, version)))
    }

    async fn folding_range(
        &self,
        params: FoldingRangeParams,
    ) -> jsonrpc::Result<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri.to_string();
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
        let uri = params.text_document.uri.to_string();
        let doc = self.open_doc(&uri).await?;

        // Never rewrite syntactically broken YANG: the regenerator could drop
        // or re-shape error-recovered content and the result would then fail
        // to parse (e.g. an error over the whole document).
        let source = doc.rope.to_string();
        if syntax_broken(&source) {
            Window::log(warning!("formatting skipped: document has syntax errors")).await;
            return Ok(None);
        }

        let formatted = {
            let repo = self.repo.read().await;
            let root = repo.statement(&uri);
            let comments = repo.comments(&uri).unwrap_or(&[]);
            format::handle(&doc.rope, root, comments, self.config().indent_width())
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
            start: Position { line: 0, character: 0 },
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
        let uri = params
            .text_document_position_params
            .text_document
            .uri
            .to_string();
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
            let targets = goto::resolve(root, byte, &scope, lib).unwrap_or_default();
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
        let uri = params.text_document_position_params.text_document.uri.to_string();
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
        let uri = params.text_document_position.text_document.uri.to_string();
        let pos = params.text_document_position.position;
        let byte = self.caret_byte(&uri, pos).await?;
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
        let items = completion::handle(root, byte, &scope, lib, &params);
        drop(repo);
        Ok(items.map(CompletionResponse::Array))
    }

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> jsonrpc::Result<DocumentDiagnosticReportResult> {
        let uri = params.text_document.uri.to_string();
        let snap = self.snapshot().await;
        let generation = snap.generation;
        let rope = self.rope_for(&uri).await.unwrap_or_else(|| Rope::from_str(""));

        let mut items = diagnostic::convert(&rope, &snap.diags, &uri);

        // LS-side checks (conflict prefix).
        let repo = self.repo.read().await;
        let root = repo.statement(&uri);
        items.extend(diagnostic::conflict_prefix(&rope, root));
        drop(repo);

        Ok(diagnostic::report(generation.to_string(), items))
    }
}
