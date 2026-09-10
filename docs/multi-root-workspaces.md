# Multi-workspace (multi-root) support — plan

> Status: **proposal / not started**. Effort estimate and phased plan for
> lifting the single-workspace restriction so the language server can serve a
> VS Code multi-root workspace (several folders of `.yang` modules treated as
> one logical tree).
>
> Related: `docs/serving-large-trees.md` (catalog + closure serving model),
> `docs/architecture.md` §6 (server state / open closure).

## Motivation

A NETCONF/YANG user often keeps interdependent vendor/standard trees in
separate folders (e.g. `vendor-a/`, `vendor-b/`, `standards/`) and opens them
as a multi-root workspace. Today the extension refuses this outright, and the
server is architected around a single workspace root.

## Current state (single-root assumptions)

- **Client** (`clients/vscode/src/extension.ts`):
  - hard-blocks `workspace.workspaceFolders.length > 1`
    ("multiple workspaces are not supported") and requires at least one folder;
  - doc selectors are anchored to the single `root_dir`
    (`${root_dir.fsPath}/**/*.{yang,xml,json}`).
- **Server** (`src/server.rs`):
  - `Server.root_uri: OnceLock<Uri>` — exactly one root (lines ~60/89),
    set from `InitializeParams.root_uri` (~748);
  - `ensure_startup_index` / `build_startup_index` walk one root (`~461/560`);
  - `ensure_refidx` (whole-tree `ReferenceIndex`) walks the one root (~504);
  - configuration is fetched for the single root scope (~798).
- **Workspace helpers** (`src/workspace.rs`): `walk_yang_files(root)` walks one
  path; url↔path helpers are single-file oriented.
- Serving model (`docs/serving-large-trees.md`): one header-only catalog +
  open-closure repository + lazily built whole-tree reference index — all
  keyed to the one scanned root.

## Design decision (recommended: Option A)

- **Option A — one merged logical tree (recommended).** Keep a single
  `Server`, repository, `CatalogIndex`, and `ReferenceIndex`, but treat the
  folder set as *one* tree: scan all roots into one catalog, resolve imports
  with the existing canonical-latest rules, and build the reference index
  across all roots. Lowest effort; semantics match today's single-tree behavior
  (modules are name-addressed).
  Caveat: the same module name in two folders resolves to the single canonical
  copy — identical to duplicate-module handling inside one tree today.
- **Option B — root-isolated namespaces.** Per-root catalogs/libraries with
  explicit cross-root resolution and per-root config isolation. Needed only for
  strict isolation or when two same-named modules must never collide. Effort is
  roughly 2–3× Option A.

## Effort estimate (focused, one developer)

| Area | Change | Effort |
|------|--------|--------|
| Client (vscode) | Remove multi-workspace guard; build doc selectors from the folder list (keep xml/json scoped *per folder*); pass `workspaceFolders` | ~0.5–1 day |
| Server state | Replace `root_uri: OnceLock` with a `Vec` of canonical folder URIs read from `InitializeParams.workspace_folders` (fallback `root_uri`); single init guard | ~0.5 day |
| Workspace walking | `workspace::walk_yang_files` → walk N roots with canonical-url dedupe (nested/overlapping roots) | ~0.5 day |
| Catalog + ReferenceIndex | `build_startup_index`/`ensure_refidx` over all roots; confirm import resolution and duplicate-name semantics; progress shows root count | ~1 day |
| Config scoping | Fetch `netconf.*` per folder (or unscoped) at `initialized`; formatting uses the owning root's `indentSize` | ~0.5–1 day |
| Tests + docs | Multi-root fixtures (cross-folder goto/hover/diagnostics/find-references/rename; duplicate-name case); architecture/features/CHANGELOG notes | ~1 day |

**Total ≈ 4–6 focused days** (≈2–3 calendar weeks when interleaved with
reviews/testing). Pure server core ≈ 2 days; the rest is polish, tests, and
semantics decisions.

## Phased plan

1. **Phase 1 — unblock + roots plumbing (client + server state).**
   Remove the client guard; register per-folder doc selectors; server stores a
   `Vec` of folder URIs (support `workspace/workspaceFolders` and
   `workspace/didChangeWorkspaceFolders`).
   *Checkpoint:* extension activates on a two-folder workspace and logs both
   roots; existing single-folder behavior unchanged (server tests + one manual
   single-root session green).
2. **Phase 2 — merged scan/index.**
   Multi-root `walk_yang_files` feeding one `CatalogIndex` + `ReferenceIndex`
   (canonical-url dedupe).
   *Checkpoint:* module A in folder 1 importing module B in folder 2 →
   goto/hover/diagnostics and whole-tree find-references/rename work across the
   boundary; add multi-root regression tests in `closure.rs`/`server`.
3. **Phase 3 — semantics & collisions.**
   Decide the duplicate-module-name policy (config-ordered precedence or a
   diagnostic) and overlapping-root behavior.
   *Checkpoint:* explicit tests for same-name modules across two roots and a
   folder nested inside another (no double catalog entries).
4. **Phase 4 — config + polish.**
   Per-folder configuration fetch; format uses the owning root's `indentSize`;
   restart command re-scans all roots; progress reflects the total file count.
   *Checkpoint:* differing `netconf.indentSize` per folder is respected.
5. **Phase 5 — docs, E2E, perf.**
   Update `docs/architecture.md`, `docs/features.md`, extension README/CHANGELOG;
   measure scan/compile/ReferenceIndex time over two real vendor trees vs the
   single-root baseline.

## Risks

- **Duplicate module names across roots** — the biggest semantic risk under
  Option A; mitigate with a config-ordered precedence policy or a diagnostic.
- **Overlapping/nested folders** — dedupe by canonical file URL, or files are
  double-counted in the catalog/index.
- **XML/JSON selector capture** — keep selectors anchored per folder so the
  server does not claim unrelated XML/JSON in sibling non-NETCONF folders.
- **Memory/perf** — the merged tree is larger; the `ReferenceIndex` stays lazy
  but now spans all roots (progress bar + perf guard in Phase 5).

## Reference touchpoints

- `clients/vscode/src/extension.ts` — workspace guard (lines ~55–64),
  per-root doc selectors (~64–82).
- `src/server.rs` — `root_uri` field/init (60/89), `initialize` (748),
  `ensure_startup_index`/`build_startup_index` (lazy startup index), `ensure_refidx` (504),
  config fetch (798).
- `src/workspace.rs` — `walk_yang_files` and url/path helpers.
- `docs/serving-large-trees.md` — catalog + open-closure + reference-index model.
