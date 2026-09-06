# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
  versioned in lockstep at `0.2.0`; the Zed extension (`clients/zed`) is
  versioned separately (`0.0.1`).
