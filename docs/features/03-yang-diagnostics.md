# YANG diagnostics

Part of the YANG authoring family. Checks on `*.yang` modules that appear in
the editor's Problems panel: syntax, module-graph problems, broken references,
schema edges, list keys, and duplicate local prefixes.

## For users

- **What it reports**
  - syntax errors (parse / recovery)
  - module graph: unresolved `import` / `include` / `belongs-to`, duplicate
    module, `import` / `include` cycles
  - broken references: unresolved prefix, typedef, grouping, identity
  - schema edges: `augment` / `deviation` target not found
  - list keys: missing or invalid `key`, config `list` without a `key`
  - duplicate local `prefix` declarations in one module
- **When it runs**: as you edit a module, and again when a module is opened or
  closed (a module appearing or disappearing can affect other open modules'
  imports).
- **How it performs**: a module is checked together with everything it pulls in
  — the modules it imports/includes and *their* imports — so a change in a
  library module is reflected in every module that depends on it. Checks are
  cached and recomputed only when one of those modules changes.
- **Customization**: none — individual checks cannot be switched off.

## For developers

- **Implementation**: `src/diagnostic.rs` + `src/server.rs` (pull + refresh),
  codes produced by `yrepo::Repository::compile` (`src/diag.rs`, `compile.rs`)
  — see [`../architecture.md`](../architecture.md) §7 and yrepo README.
- **Serving scope**: diagnostics are computed over the open-closure
  repository; see [`../serving-large-trees.md`](../serving-large-trees.md).
- **Tests**: yrepo numbered `tests/0NN_*.rs`; netconf tests under
  `src/diagnostic.rs` and `src/server.rs`.
