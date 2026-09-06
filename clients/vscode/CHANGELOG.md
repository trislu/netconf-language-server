# Changelog

All notable changes to the **netconf-language-server** VS Code extension will be
documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this extension adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
