# Go-to-definition & hover (YANG)

Closely coupled read features: locate the symbol under the caret (definition or
reference) and resolve it across modules, incl. `leafref` paths.

## For users

- **Go-to-definition** (editor *Go to Definition* — VS Code <kbd>F12</kbd> or
  <kbd>Ctrl</kbd>-click):
  caret on a reference → jump to its definition, including when the
  definition lives in another module. Covers `typedef`, `grouping`,
  `identity`, `feature`, `extension` references and `leafref` path
  references.
- **Prefixed references split at the colon**: on `prefix:name`, the caret
  chooses the target, for `type`, `base`, `uses`, `if-feature`, an
  identity-valued `default`, and an extension usage head. On the prefix, hover
  shows and goto opens the `import` that binds it (or the module's own
  `prefix` statement). On the local name, both resolve to the definition in
  the imported module — a `typedef`, `identity`, `grouping`, `feature`, or
  `extension`. Path arguments (`augment`, `deviation`, `leafref` `path`) keep
  resolving as whole paths.
- **Hover**: hover a name to see the defining source snippet plus the node's
  kind/type, `identity` ancestry, and which prefixes are in scope.
- **When it runs / customization**: nothing to enable and no settings — both
  work on any open module and resolve across the modules it can reach (open
  buffers plus the on-disk modules they import/include).
- **Limitations**: plain data-node names (`leaf`/`container`/`list`) and the
  builtin types are not navigation targets — goto/hover cover the schema-symbol
  kinds and `leafref` paths above.

## For developers

- **Implementation**: `src/goto.rs`, `src/hover.rs`, `src/references.rs`
  (`def_at`); resolution through `yrepo::Library` queries (see
  [`../architecture.md`](../architecture.md) §8.4 and yrepo README). Caret anywhere on a
  definition statement (name, keyword, or body gap) resolves to it.
  `goto::ref_part_at` decides which half of a `prefix:name` argument the caret
  is on; both hover and goto use it.
- **Tests**: unit tests in `src/goto.rs` / `src/hover.rs` /
  `src/references.rs`.
