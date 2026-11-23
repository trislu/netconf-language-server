# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **Hover and go-to-definition agree on a prefixed reference.** In
  `prefix:name`, the caret now chooses the target, for `type`, `base`, `uses`,
  `if-feature`, an identity-valued `default`, and an extension usage head. On
  the prefix, both show and jump to the `import` that binds it (or the
  module's own `prefix` statement); on the local name, both resolve to the
  definition in that module (`typedef`, `identity`, `grouping`, `feature`, or
  `extension`). Hover used to describe the prefix wherever the caret was,
  while goto jumped to the definition.

## [0.8.2] - 2026-11-23

No public API change and no parse-output change; this release only fixes the
VSIX packaging that failed for 0.8.1.

### Fixed

- **The extension icon is within the Marketplace's 1024 KB limit.**
  `resources/images/icon.png` was a 1024×1024 PNG of 1,143 KB, so
  `vsce publish` rejected every platform with "the provided icon image exceeds
  the maximum allowed file size of 1024 KB", and 0.8.1 reached crates.io but no
  VSIX. The icon is now 512×512 (375 KB).

## [0.8.1] - 2026-11-22

No public API change: the server's LSP surface is the same as 0.8.0, and
valid YANG modules parse to the same trees.

### Added

- **One VSIX per platform and architecture, published with Trusted
  Publishing.** `linux-x64`, `linux-arm64` (both static musl), `darwin-x64`,
  `darwin-arm64`, `win32-x64` and `win32-arm64` are built and packaged
  separately, so the Marketplace hands each host the build it can run —
  including the remote side of a Remote-SSH window. A missing `linux-arm64`
  build used to surface as "language server not found"; an Intel Mac had no
  binary at all. Publishing exchanges a GitHub OIDC token for a short-lived
  Marketplace credential (`vsce publish --oidc`), so no personal access token
  is stored, and a tag that does not name both `Cargo.toml` and
  `clients/vscode/package.json` fails before anything is built.
- **A stdio JSON-RPC test harness** (`tests/lsp_stdio.rs`): it spawns the
  built server on a temporary workspace and covers the initialize handshake,
  pull diagnostics, hover, completion, shutdown/exit, and a workspace whose
  XML belongs to no YANG module staying dormant. It answers the
  `workspace/configuration` request the server sends back, without which a
  session stalls.
- **Recorded README demos** (`assets/demos/*.gif`) with the recorder and GIF
  assembler that produce them (`scripts/record-vscode.mjs`,
  `scripts/assemble-gif.py`, `docs/notes/demo-recording.md`).

### Changed

- **The extension activates only where there is NETCONF content**
  (`workspaceContains:**/*.yang`, `onLanguage:yang`) instead of on every
  window, and serves the one workspace folder that holds YANG modules. Several
  such folders still produce a warning, but that is now a warning naming them
  rather than an error, and a window with no YANG folder is left alone: it
  logs why it stayed idle instead of interrupting the user. The contributed
  commands stay available in those windows.
- **Hover snippets are dedented.** A multi-line statement used to start flush
  with the code fence while its continuation lines kept the file's
  indentation; the shared prefix is now removed, so the block reads as one
  statement.
- **The Zed client downloads the per-target asset**
  (`netconf-language-server-<os>-<arch>`) instead of an `<os>`-only name, and
  the release keeps the old `<os>` names as aliases for extensions published
  before 0.0.2.
- The bundled server lives at `resources/bin/<platform>-<arch>/`, one binary
  per VSIX.

### Fixed

- **No error dialog in a workspace of another project.** "open a workspace
  folder containing .yang files" and "multiple workspaces are not supported"
  were modal errors shown in windows that had nothing to do with YANG.
- **Test harness URIs work on Windows** (`file://C:\…` is not a URI).
- **Document selectors match folders literally**: a folder name containing
  glob characters (`netconf [2026]`) turned into a pattern and matched no
  file.
- **A `workflow_dispatch` from a branch no longer breaks the VSIX path** when
  the branch name contains a separator.

## [0.8.0] - 2026-09-17

### Added

- **`identityref` value completion in XML and JSON instance documents.** An
  `identityref` leaf (such as an interface's `<type>`) now suggests its valid
  values from a workspace-wide identity graph built from the summary index:
  every identity that is the leaf's `base` or derived from it, across modules
  outside the open closure too. In XML, at an `xmlns:` attribute name in the
  leaf's start tag it offers one namespace declaration per contributing module
  (`xmlns:<module-prefix>="<namespace>"`, with that module's identities
  listed), and in the leaf's content it offers `<module-prefix>:<identity>`,
  adding the `xmlns` declaration when the prefix is not yet in scope. In JSON
  (RFC 7951 §6.8) it completes the value string with `module-name:identity-name`.
- **`enumeration`/`bits`/`boolean` leaf value completion.** A leaf whose type
  has a finite value set now completes its value — enum members, the bit names
  of a `bits`, or `true`/`false` for a `boolean` — in XML content and in a JSON
  value (a string for enumeration/bits, a literal for boolean). `union` stays
  silent. Unlike `identityref` this needs no workspace index: the values come
  from the leaf's resolved type.
- **JSON value completion at an empty value slot.** With a member key written
  but no value yet (`"color": |`), the tolerant JSON parse kept the member with
  an empty value span, so value completion (enumeration/bits/boolean and
  `identityref`) found nothing. Completion now parses caret-aware, filling the
  slot with a placeholder before mapping: string values are inserted quoted and
  `boolean` as a bare literal. Diagnostics keep using the strict parse, so an
  in-progress value is never validated.

### Fixed

- **XML completion in an `edit-config` payload (and at every other NETCONF
  position that spans the whole workspace).** Two authoring positions were
  served from the compiled library's *open closure* — a few modules — instead of
  the workspace: an `<rpc>` envelope offered data roots but none of the
  workspace modules' own `rpc`/`notification`, and a `<config>`/`<filter>`
  payload wrapper nested under `<edit-config>` (the common rpc template)
  returned **zero** items on a real corpus, because the document's closure
  (e.g. `ietf-netconf`) declares no top-level data node. Both positions are now
  answered from the module-summary index (which carries top-level
  data/rpc/notification names), so a nested `<config>` offers every module's
  data roots and `<rpc>` offers the built-in NETCONF operations plus every
  module's operations.
- **`<data>` is no longer a completion position.** It only ever appears in a
  server's `<rpc-reply>`, so it is not an authoring slot; the instance mapping
  still treats it as a payload wrapper, so a captured reply is still mapped and
  validated.
- **Nested completion under a data node.** Picking a top-level data root from
  the summary index only inserted its element; the language server then
  materialized just the document **root's** module (XML: the root's namespace;
  JSON: the first `module:name` root member), so the child's own module was not
  compiled and typing inside that element returned **zero** items. An instance
  document now seeds the closure of **every** module it references — the
  distinct element namespaces (XML) or member qualifiers (JSON) — in one
  reconcile, so children of a data node complete in an `edit-config` payload,
  a bare `<config>` file, a `<get-config>`/`<filter>` selection, and a
  multi-module RFC 7951 document.
- **A NETCONF payload wrapper's subtree is mapped, modeled or not.** A
  `<config>`/`<filter>`/`<data>` payload wrapper used to lose its whole subtree
  in two ways: when the envelope module was compiled, the wrapper resolved to
  its schema's **`anyxml`** node (`edit-config`'s `config`, `get-config`'s
  `filter`) and the content went *opaque*; when it was not compiled, the wrapper
  inherited the unmodeled envelope's silent slot. Either way, typing inside the
  data root below it (e.g. `<interfaces>`) returned **zero** items and
  hover/goto/diagnostics below it were dead. The instance mapping now maps an
  `anyxml`/`anydata` payload carrier's content as a data tree, and promotes a
  wrapper under an unmodeled envelope to a data position. A `<filter>` nested
  under `<get>`/`<get-config>` is also recognized as a partial-content filter
  (previously only a document-root `<filter>` was), so the newly mapped subtree
  raises no false missing key/mandatory diagnostics.
- **`identityref` values in XML are resolved through their XML namespace
  context.** An identityref value's prefix is an **XML namespace prefix**
  (RFC 7950 §9.10.3), declared on the element or inherited from an ancestor,
  and an unprefixed value is in the in-scope default namespace — not a YANG
  import prefix. The validator handed the raw text to the semantic check, so a
  valid value such as `ianaift:ethernetCsmacd` (with `xmlns:ianaift` bound to
  the identity's module) was reported as an unknown identity. Identityref
  values are now rewritten to `module:local` via the element's in-scope
  declarations (and the referenced module is seeded into the schema), and the
  check stays **silent** when the namespace does not resolve to a compiled
  module instead of emitting a false "unknown identity". The JSON (RFC 7951)
  path resolves `module-name:identity` the same way and seeds the value's
  module into the schema, so cross-module identityref values are checked in
  both encodings (silent only when the module cannot be compiled).

## [0.7.0] - 2026-09-11

### Added

- **Lazy schema for NETCONF instance documents on giant workspaces**: with no
  YANG file open, typing `<` in XML or `"` in a root JSON object now offers
  data roots from a cached, progress-visible parse-level module-summary index
  (first use scans the tree once; later uses are cached), and
  diagnostics/hover/goto materialize only the document's own module closure.
  In-progress text (a lone `<` or `"`) is tolerated, `classify`/`recognize` no
  longer report `NotNetconf`, and find-all-references/rename still trigger only
  their own lazy `ReferenceIndex`. Measured on the external corpus
  (165 521 files): summary scan 12.05 s (51 -> 481 MB RSS), XML root completion
  13.38 s first / 0.41 s cached (79 371 items), JSON root 0.39 s, per-module
  closure 17.7 ms, diagnostics 0.24 s. Requires yrepo 0.7.1.

## [0.6.0] - 2026-09-11

### Changed

- **Linux releases stay static-musl and now use `mimalloc` with its `override`
  feature.** The catalog scan is allocation-heavy and the previous static musl
  build spent ~69% of its CPU in the kernel on allocator syscalls (~10x wall
  time). Installing mimalloc as the Rust global allocator alone was not enough:
  the tree-sitter parser's C-level `malloc`/`free` calls still hit musl's
  allocator, so `override` also interposes the C symbols. A/B over the giant
  workspace (external corpus, counts recorded per run) shows static
  musl+override ≈ glibc (~12 s full scan, ~3% sys), so the portable static
  artifact is kept instead of moving to a glibc build with an implied glibc
  version floor. CI asserts the artifact is static. See
  `docs/perf/catalog-scan-regression-2026-09-11.md`.
- **Bundled `yrepo` 0.6.0**: the catalog path now parses header-only
  (`Catalog::scan`) and lazy startup uses the new `PathIndex` +
  `CatalogIndex::resolve_lazy`. Retained catalog fields and resolution results
  are unchanged.

### Added

- **Lazy schema for instance documents on giant workspaces**: with no YANG file
  open, typing `<` in XML or `"` in a root JSON object now offers data roots
  from a cached, progress-visible parse-level module-summary index
  (`SummaryIndex`; first use scans the tree once, later uses are cached), and
  diagnostics/hover/goto materialize only the document's own module closure.
  In-progress text (a lone `<` or `"`) is tolerated. `ReferenceIndex` is never
  triggered by instance documents. Measured on the external corpus: summary
  scan 12.05 s, XML root completion 13.38 s → 0.41 s cached, per-module closure
  17.7 ms. See `docs/design-instance-lazy-schema.md`.
- **Instant startup indexing on giant workspaces**: `initialize` no longer
  scans the whole tree. It builds a parse-free basename index (`NameIndex`) and
  returns; the open document's closure is resolved on demand by parsing only
  the candidate headers of the names it needs (bounded, cached, with a bounded
  prefix fallback for declared names that differ from filenames). Whole-tree
  work stays lazy (`ReferenceIndex` is unchanged). Measured on the external
  corpus (165 521 files): 0.27–0.29 s startup with 0 headers parsed; opening a
  standard module resolved its closure in 12.2 ms with 0 diagnostics. The lazy
  `ReferenceIndex` is unchanged (still progress-visible) and its full-tree
  build dropped from 418.6 s to 25.0 s with the current parser fixes, returning
  identical occurrences/references. See `docs/design-lazy-startup-catalog.md`.
- `scripts/lsp_scan_driver.py` and `scripts/lspsample.py`: reproducible
  end-to-end probes for the workspace catalog scan (timed server log lines;
  per-process threads/user/sys CPU/RSS/context-switch sampling).
- `.github/workflows/perf-ab.yml`: manual (workflow_dispatch) gnu-vs-musl
  timing A/B skeleton with a hermetic micro-fixture (the external corpus is
  never required in CI).

## [0.5.0] - 2026-09-10

### Added

- **Customizable YANG semantic highlighting** (`netconf.semantic`): the server
  now announces the **full standard LSP semantic-token legend** (every
  `SemanticTokenType` and `SemanticTokenModifier`) and resolves classification
  per *role* at `semantic_tokens_full` time, so any change applies with no
  server restart. `netconf.semantic` is a per-role struct: each YANG construct
  family (`moduleName`, `typeRef`, `enumAndBitNames`, `units`, … — 20 roles,
  incl. `patternArg`) is
  a member whose value picks the token **type** and optional **modifiers**
  (multi-select), letting users compose any classification the semantic
  highlight guide supports; a role left out keeps its built-in classification.
  Built-in (no config needed): `pattern` arguments are colored `regexp`, and a
  declaration with a `status deprecated;` child marks its whole subtree —
  including the `status deprecated;` marker's own words — with the standard
  `deprecated` modifier.
  (`src/semantic_token.rs` `Class`/`Modifier`/`Role`/`Style`, `src/config.rs`,
  `src/server.rs`.)

### Changed

- **`netconf.indentSize` default lowered to `2`** (was `4`): the formatter
  indents two spaces per level for unconfigured users; an explicit setting is
  unaffected.

### Fixed

- **Live configuration updates** — `netconf.semantic` (and `netconf.indentSize`)
  changes now take effect immediately: `didChangeConfiguration` previously
  stored into a `OnceLock`, so updates after the startup fetch were silently
  dropped. The server now keeps a replaceable config and, when the semantic
  classification changed, asks the client to refresh semantic tokens
  (`workspace/semanticTokens/refresh`) so open documents re-highlight at once.

## [0.4.0] - 2026-09-09

### Added

- **Whole-tree Find References & Rename beyond the open closure**:
  `textDocument/references` and `textDocument/rename` now also search every
  on-disk module that imports the definition's module (e.g. a typedef in
  `ietf-yang-types` used across the tree) via the new lazy
  `yrepo::ReferenceIndex` — the first such request builds the index over the
  workspace behind a server→client work-done progress bar
  (`src/client.rs` `Progress`, `src/server.rs` `ensure_refidx`), with richer
  `window/logMessage` detail (caret word, resolved `module:local`, open-closure
  vs whole-tree hit counts, elapsed ms). A caret anywhere on a definition
  statement (name, keyword, or body gap) resolves to it.

### Changed

- **Dependency: `yrepo` 0.5.0** — whole-tree references/rename are now backed
  by the released `yrepo::ReferenceIndex`; the local `[patch.crates-io]`
  override is removed. Bundles the `tree-sitter-yang` 0.4.1 grammar fix and
  the token-stream fix for concatenated quoted fragments.

### Fixed

- **Stale whole-tree results after a rename**: the on-disk `ReferenceIndex`
  was built once and cached, so after a whole-tree rename rewrote files (old
  symbol name → new), a follow-up Find References on the new name only saw the
  open-closure hits. The index is now invalidated after any successful rename
  (`Server::invalidate_refidx`) and rebuilt lazily from disk on the next
  whole-tree request.
- **Concatenated quoted arguments not highlighted** (`namespace "…" + "…"`):
  the grammar lexes the leading quoted fragment as a hidden token, so it was
  missing from `yrepo::Repository::tokens` and never colored (the `+` and
  later fragments were). Fixed upstream in `yrepo` (`parse` now recovers
  quoted runs and `+` operators inside argument spans via
  `augment_quoted_fragments`); the semantic-token lexical pass then colors
  them as any other `String`/`Operator` token.

## [0.3.0] - 2026-09-07

### Added

- **Find References & Rename** (`textDocument/references`, `textDocument/rename`
  - `prepareRename`) for `typedef` / `grouping` / `identity` / `feature` /
  `extension` symbols: references are gathered across every compiled module and
  submodule of the library (`src/references.rs`); rename validates the new name
  and rewrites the declaration plus all reference sites in a single workspace
  edit.
- **Goto-definition & hover for instance-path constructs**
  (`src/goto.rs`, `src/hover.rs`):
  - `leafref` `path` segments and `deviation` targets resolve to the schema node
    they name (relying on the library's predicate stripping for absolute paths);
  - `uses`-augment targets jump into the used `grouping` body, with matching
    hover output for the refined target.
- **Completion** (`src/completion.rs`):
  - `grouping` names for `uses` arguments;
  - `leafref` `path` segment completion;
  - `augment` / `deviation` path completion;
  - module-prefix candidates when typing the start of an absolute path.
- **Open-closure serving (catalog + closure)**: the workspace is indexed
  header-only (`fill_catalog`, `yrepo::Catalog::scan` per file) and the
  repository holds only the open buffers plus the on-disk modules they can
  reach (`sync_open_closure`, new `src/closure.rs`: header seeds + catalog
  closure with revision-date pins). Retention and compile cost scale with the
  open view, not the tree size — see `docs/serving-large-trees.md`.
  Open buffers keep full parse views; reachable on-disk modules parse
  text-light.
- **Startup UX**: the extension shows a visible progress bar while the initial
  workspace scan runs — the server blocks `initialize` until the scan finishes
  and `clients/vscode` wraps `client.start()` in a `window.withProgress` bar.
- **Scale evidence**: `docs/perf/giant-scale-2026-09/` — a 10k-interval × 5 grid
  run with a CSV summary and a self-contained HTML report.

### Changed

- **Handlers run serially in arrival order** (`Server::concurrency_level(1)`):
  the open-closure state is notification-driven and cross-document, so
  didOpen/didChange/didClose and the feature requests after them must observe
  each other in client order (tower-lsp defaults to 4-way concurrent dispatch
  with no ordering). Trade-off: `$/cancelRequest` cannot preempt a running
  handler.
- **Faster startup on large workspaces**: the one-time catalog scan fans out
  over the rayon pool (`CatalogIndex::scan_many_files_with`) inside a
  `spawn_blocking` task, and the server-side document cache is unbounded (the
  client controls which buffers stay open).
- `inspect`/`probe` development tools moved out to the `yrepo` crate
  (`examples/`); their `src/bin` copies were removed here.
- Updated dependencies and README.

### Fixed

- `netconf` settings are now actually applied on startup: configuration was
  fetched during `initialize`, when tower-lsp silently drops client requests, so
  it always fell back to defaults.

## [0.2.0] - 2026-09-06

### Added

- **Zed extension** (`clients/zed`): the same `netconf-language-server` binary
  attached to Zed's YANG / XML / JSON languages for **read** (diagnostics &
  hover) and **write** (completion).
- **Faster startup on large workspaces**: `yrepo` 0.3's `parallel` feature —
  the one-time scan ingests on-disk `.yang` modules in a single
  `upsert_many_files` batch and `compile` phases run across threads.

### Changed

- `yrepo` is now pulled from crates.io at `0.3` with the `parallel` feature
  (previously a local path dependency).

## [0.1.0] - 2026-09-05

Initial release of the **netconf-language-server**: a Rust LSP for authoring
NETCONF/YANG — YANG modules plus XML / RFC 7951 JSON instance documents — with a
VS Code extension.

### Added

- **YANG authoring** (semantic engine: [`yrepo`](https://crates.io/crates/yrepo)):
  - semantic tokens, statement-level folding, and full-document formatting
    (comment-safe, syntax-error guarded);
  - pull-based diagnostics (import/include cycles, unresolved references,
    list-`key` validation, LS-side `conflict_prefix`);
  - go-to-definition (`LocationLink`), hover (type chains / identity ancestry /
    prefix bindings), and completion for `type` / identity `base` arguments.
- **NETCONF instance documents** (milestones M0–M5):
  - content-sniffing vs the compiled YANG library; unmatched files stay dormant;
  - XML (M1) and RFC 7951 JSON (M3) **read**: goto / hover / diagnostics;
  - diagnostics **depth** (M4): missing mandatory nodes / list keys and empty
    `choice`s (XML + JSON), suppressed inside `<filter>`;
  - XML **write** (M2): `hello` / `get-config` / `edit-config` / `<config>`
    payload templates plus element completion (`key` stubs, auto-`xmlns`);
  - JSON **write** (M4): RFC 7951 member-name completion (module-qualified
    where required);
  - leaf **value validation** (M5): scalar-reducible types only — string
    `length`/`pattern`, integer/`decimal64` `range`, `enumeration`/`bits`,
    boolean/empty/binary, semantic `identityref` — `union` deliberately silent;
    typed completion value defaults.
- **VS Code extension** (`clients/vscode`): language ids `yang`/`xml`/`json`,
  `NETCONF: Insert …` commands, `netconf.indentSize` setting.
- **Docs**: architecture & decision record (`docs/architecture.md`, D1–D31),
  detailed LSP feature guide (`docs/features.md`).

### Notes

- The Rust crate (`netconf-language-server`) and the **VS Code** extension are
  versioned in lockstep at `0.3.0`; the Zed extension (`clients/zed`) is
  versioned separately (`0.0.1`).
