# Changelog

All notable changes to the **netconf-language-server** VS Code extension will be
documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this extension adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.0] - 2026-11-23

Bundled `netconf-language-server` updated to **0.9.0** (no protocol or parse
change; hover and go-to-definition now follow the caret).

- Go to Definition and hover on a `prefix:name` reference treat the two halves
  separately: the prefix opens its `import` statement, the local name opens the
  `typedef`, `identity`, `grouping`, `feature`, or `extension` it names.
- Path arguments (`augment`, `deviation`, `leafref` `path`) resolve the segment
  under the caret instead of only the final node; a prefix inside a path opens
  its `import`.
- The extension is also published to Open VSX from the same tag.

## [0.8.2] - 2026-11-23

Bundled `netconf-language-server` updated to **0.8.2** (no protocol or parse
change). This is the 0.8.1 feature set with a publishable package: 0.8.1 could
not be uploaded because its icon exceeded the Marketplace's 1024 KB limit.

- The packaged icon is 512×512 (375 KB) instead of 1024×1024 (1,143 KB).

## [0.8.1] - 2026-11-22

Bundled `netconf-language-server` updated to **0.8.1** (no protocol or parse
change; hover snippets render flush).

- The extension now starts only where there is NETCONF content: a workspace
  containing YANG modules, or a YANG file opened directly. Opening a project of
  another kind no longer produces an error dialog, and a multi-root workspace
  serves the one folder that holds YANG modules (several such folders raise a
  warning instead of an error).
- Hover code blocks start at the left edge of the fence. A multi-line statement
  used to keep its continuation lines indented while the first line sat flush,
  which read as a ragged block.
- One package per platform and architecture: Linux, macOS and Windows each get
  x64 and arm64 builds carrying the matching server binary, so the Marketplace
  installs the one this machine (or its Remote-SSH host) can run. Windows builds
  now use the MSVC toolchain.
- The commands stay available in windows that serve nothing; they log instead of
  failing with "command not found".

## [0.8.0] - 2026-09-17

- Schema-driven **leaf value completion** in instance documents: `identityref`
  values (XML `xmlns:` declarations and `prefix:identity`; JSON
  `module:identity`) from a workspace-wide identity graph, plus `enumeration`
  members, `bits` names and `boolean` values, including an empty JSON value
  slot.
- Instance completion now works down the data tree and inside an
  `edit-config` payload, and the matching diagnostics no longer report valid
  cross-module `identityref` values as unknown.

## [0.7.0] - 2026-09-11

- Instance-document schema support on giant workspaces: XML/RFC 7951 JSON
  completion, diagnostics and navigation work with no YANG file open, using a
  lazily built module-summary index and a per-module closure.

## [0.6.0] - 2026-09-11

### Changed

- Bundled `netconf-language-server` updated to **0.6.0**: `initialize` no longer
  scans the whole workspace. Startup builds a parse-free name index and returns;
  each open document's closure is resolved on demand by parsing only the
  candidate headers of the names it needs. On the reference giant workspace
  (165 521 files) startup drops to 0.27–0.29 s (from a full-tree scan that took
  minutes), and opening a standard module resolves its closure in ~12 ms.
- The Linux server artifact stays static-musl and now uses `mimalloc` with its
  `override` feature, removing the allocator syscall storm (~10× wall time, ~69%
  kernel CPU) that the previous static musl build caused.

No new settings or commands: semantic highlighting, formatting, completion and
validation behave exactly as in 0.5.0.

## [0.5.0] - 2026-09-10

### Added

- **Semantic-highlight settings** `netconf.semantic`: a per-role object where
  each YANG construct family (module/type/data-node/enum names, `units`,
  `rangeLength`, `patternArg`, …) selects a semantic token **type** from any
  standard type plus **modifiers** (multi-select). Applied live — no server
  restart.
- **Richer built-ins**: `pattern` arguments color as `regexp`, and
  `status deprecated;` declarations get the standard `deprecated` modifier on
  their whole subtree — including the `status deprecated;` marker's own words.
- **Deprecation styling**: striking the `deprecated` subtree is a theme/rule
  concern (VS Code reads `editor.semanticTokenColorCustomizations` at the theme
  layer, so a language-scoped extension default cannot force it) — set it in
  user settings to opt in:
  ```jsonc
  "editor.semanticTokenColorCustomizations": {
    "rules": { "*.deprecated:yang": { "strikethrough": true } }
  }
  ```

### Changed

- Bundled `netconf-language-server` updated to **0.5.0**: announces the **full
  standard** semantic token type + modifier legend so every per-role choice
  applies without a restart; `netconf.indentSize` now defaults to `2` (was
  `4`).

## [0.4.0] - 2026-09-09

### Added

- **Restart Language Server command** (`NETCONF: Restart Language Server`):
  stops and restarts the language client via `client.restart()`, wrapped in a
  visible progress bar while the server re-runs its workspace scan.

### Changed

- Bundled `netconf-language-server` updated to 0.4.0: whole-tree Find
  References & Rename, a caret-resolution fix for definition statements, a
  stale-index fix after rename, and a highlight fix for concatenated quoted
  strings.

## [0.3.0] - 2026-09-07

### Added

- **Startup progress**: a visible progress notification is shown while the server
  runs its initial workspace scan — the extension wraps `client.start()` in a
  `window.withProgress` bar.

### Changed

- Bundled `netconf-language-server` updated to 0.3.0: the server adds
  find-references / rename and goto/hover for `leafref` paths and `deviation`
  targets, richer completion (groupings for `uses`, `leafref` path segments,
  `augment`/`deviation` paths, module prefixes at absolute-path starts), and
  now serves an **open closure** of the workspace (header-only catalog) so
  startup time and memory track the files you have open rather than the whole
  tree.
- `netconf` settings are now applied on startup (previously fetched during
  `initialize`, when the server-side client drops requests, so defaults were
  used silently).

## [0.2.0] - 2026-09-06

### Changed

- Bundled `netconf-language-server` updated to 0.2.0 — engine `yrepo` 0.3 with
  the `parallel` feature makes workspace scans on large YANG trees faster.
  Extension behavior is unchanged.

## [0.1.0] - 2026-09-05

Initial release.

### Added

- Authoring **YANG modules**: semantic highlighting, folding, formatting,
  pull-based diagnostics, goto/hover, and `type`/identity-`base` completion.
- **NETCONF instance documents** (`.xml`, RFC 7951 `.json`): content-sniffed
  recognition (unmatched files stay dormant), diagnostics on elements/members,
  leaf *value* validation (scalar-only; `union` silent), and RFC 7951/XML
  completion.
- `NETCONF: Insert …` commands for `hello` / `get-config` / `edit-config` /
  `<config>` payloads.
- Setting `netconf.indentSize` (formatter indentation width).

For details see the [detailed LSP features guide](../docs/features.md).
