# Changelog

All notable changes to the **netconf-language-server** VS Code extension will be
documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this extension adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
