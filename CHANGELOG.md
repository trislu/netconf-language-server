# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Linux release artifact is now a glibc (`x86_64-unknown-linux-gnu`) build,
  and all Linux builds use `mimalloc` as the global allocator.** The catalog
  scan allocates one owned string per CST leaf; the previous static musl build
  turned that churn into a syscall storm (~10x wall time, ~69% sys CPU on a
  multi-MB subtree). CI keeps a `file`/`ldd` guard so a static musl binary
  cannot be shipped by accident. See
  `docs/perf/catalog-scan-regression-2026-09-11.md`.

### Added

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
