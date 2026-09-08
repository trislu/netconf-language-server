# Changelog

All notable changes to the **netconf-language-server** VS Code extension will be
documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this extension adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Restart Language Server command** (`NETCONF: Restart Language Server`):
  stops and restarts the language client via `client.restart()`, wrapped in a
  visible progress bar while the server re-runs its workspace scan.

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
