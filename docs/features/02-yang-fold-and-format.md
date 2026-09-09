# Folding & formatting (YANG)

Part of the YANG authoring family. Folding and formatting driven by the
module's structure: a statement is a `keyword { … }` block, so both work on
whole statements (comments are kept and re-indented with their statement).

## For users

- **Folding**: each statement — keyword, arguments and its whole `{ … }`
  subtree — collapses as one region (statement-level, not bracket-level).
  When it runs: automatically; fold arrows appear as soon as a module is open
  or edited, and you toggle regions with the editor's gutter or fold commands.
- **Formatting**: rewrites the whole document, aligning statements under their
  keyword. When it runs: on the editor's *Format Document* action (and
  format-on-save when you enable it). **Customization**: the indentation
  width is set by `netconf.indentSize` (default `2`).
- **Limitations**
  - Formatting is **skipped while the document has syntax errors** (the
    reformatter would have to guess at error-recovered content).
  - Both are YANG-only: `.xml`/`.json` files keep the editor's built-in
    folding and formatting.
  - Formatting always rewrites the whole file — there is no format-selection
    (range) mode.

## For developers

- **Implementation**: `src/fold.rs`, `src/format.rs`, handlers in
  `src/server.rs`; syntax-error gate via `syntax_broken`.
- **Coupling**: both consume `Statement` ranges/terminators (`StatementEnd`)
  and `Repository::comments` — see [`../architecture.md`](../architecture.md) §4
  (D2/D3).
- **Tests**: unit tests in `src/fold.rs` / `src/format.rs`.
