# Find references & rename (YANG, whole-tree)

Find-all-references and rename for schema symbols (`typedef`/`grouping`/
`identity`/`feature`/`extension`), including usages in modules that only
*import* the defining module (whole-tree).

## For users

- **Find references** (editor *Find All References* — VS Code
  <kbd>Shift</kbd>+<kbd>F12</kbd>):
  caret on a schema symbol (`typedef`/`grouping`/`identity`/`feature`/
  `extension`) to list every reference across the whole workspace, including
  modules that only *import* the defining module. Open buffers are searched
  live; references in on-disk modules are found through a whole-tree index.
- **Rename** (editor *Rename Symbol* — VS Code F2): renames the declaration
  and every reference in all affected files at once, keeping the `prefix:` on
  prefixed occurrences. The new name must be a valid YANG identifier.
- **When it runs / how it performs**: the whole-tree index is built lazily —
  the first whole-tree request shows a progress bar while it indexes the
  module tree, and later requests reuse it. Files changed on disk outside the
  editor (e.g. `git checkout`) can leave the index stale; a rename detects
  that and drops it, so the next whole-tree request re-indexes from disk.
- **Customization**: none.
- **Limitations**
  - Data nodes (`leaf`/`container`/`list` names) are not rename targets.
  - Renaming a widely-used library symbol rewrites many files at once.
  - The whole-tree index is keyed by module *name* (not revision), so
    different revisions of the same module are grouped together.

## For developers

- **Implementation**: `src/references.rs` (matching engine + `def_at`),
  `src/server.rs` (`references`/`prepare_rename`/`rename`, `ensure_refidx`,
  `invalidate_refidx`), whole-tree index from `yrepo::ReferenceIndex`
  (yrepo `src/refidx.rs`), open-closure helpers in `src/closure.rs`.
- **Serving model**: whole-tree index is lazy and rebuilt after rename — see
  [`../serving-large-trees.md`](../serving-large-trees.md).
- **Tests**: `src/references.rs` (incl. self-prefixed/body-gap caret cases),
  yrepo `refidx.rs` unit tests.
