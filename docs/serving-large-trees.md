# Serving very large YANG trees (catalog + closure + text-light)

Status: design (2026-09-06). Motivation & measurements: yrepo
`docs/memory-findings.md` (full-parse retention ~0.1–0.35 MB per real module,
catalog ~7 KB/file, text-light −16% ingest; a 163k-file full compile cannot
fit one process). This note maps the serving path onto the language server.

## Current behavior (to replace)

`Server::scan_workspace` upserts every on-disk `.yang` file into one yrepo
`Repository` (full parse of each) and `snapshot()` compiles all of them. Both
retention and compile scale with the whole tree.

## Target serving model

1. **Catalog-wide scan (cheap)**: walk the workspace and store one
   `yrepo::Catalog` per module (name, revision, prefix, imports, parse status)
   — ~7 KB/file measured on real modules. Memory for 163k files ≈ ~1.2 GB
   catalog; keep it on the side (or on disk later). NO full parses.
2. **Open documents parse full**: documents with open buffers keep full parse
   views (text-light OFF for them — description bodies may be shown/hovered
   later; or ON if we confirm no feature reads them).
3. **Closure compile**: the yrepo `Repository` for resolution contains ONLY
   the open documents plus every module reachable through their imports
   (pulled by name/revision from the catalog, full-parsed with text-light
   OPTIONAL). Compile scope = that closure; augment/deviation and diagnostics
   are computed over the closure. Measured prototype: 20 real roots → 41-file
   closure at 22.5 MB.
4. **Diagnostics** are computed over the closure only (editors only need
   diagnostics for open files and the modules they pull in). Cross-module
   features (goto/hover/references/rename/completion) already query the
   compiled `Library` — they keep working unchanged because the closure
   contains every module they can navigate to from an open file.

## Component map (netconf-language-server)

| today | change |
| --- | --- |
| `scan_workspace` (upsert all, full parse) | walk + `Catalog::scan` only; catalog map name/rev→(url,path) + imports; no Repository ingest |
| `snapshot()` (compile whole repo) | compile a Repository holding the OPEN closure (open docs + imports materialized on demand from disk via catalog) |
| `did_open` / `did_change` / `did_close` | on open/change: add doc to Repository (full parse), ensure its imports are present (pull from disk if missing), drop docs whose buffers closed and that are no longer needed by other open docs (refcount by open-closure) |
| `rope_for` / caret / token queries | unchanged for open docs; on-demand full parse for any catalog file only when a feature explicitly needs it |
| `bump`/generation cache | unchanged; snapshot keyed on the closure set |
| memory guard | optional soft cap: if catalog count > N, serve closure-only and refuse whole-tree compile; log/bench via memstep-style RSS |

## Risks / open points

- Duplicate modules across directories/versions: catalog must canonicalize by
  (name, revision) like `compile` does (highest wins), and pinned imports
  (revision-date) must resolve to the exact catalog entry.
- Submodules: catalog includes `include` names; closure must materialize
  included submodules with their parents.
- Augment/deviation semantics change subtly when the compile closure omits
  augments whose source modules are not open; acceptable (they target modules
  not in the workspace view), but diagnostics on the target that reference
  missing augments may differ — document as a serving-mode trade-off.
- Performance: repeated opens re-parse; per-open cost bounded by closure size.
  Warm catalog should avoid re-reading headers after first scan.

## Suggested phases (each with memstep/memcomp or LS integration tests)

A. yrepo: catalog registry with (name, rev) canonicalization + import/include
   lookup API (`CatalogIndex::lookup(name, rev) -> url/path`); repository
   `compile_closure(roots, resolver)` helper. (Some building blocks exist in
   `examples/closure.rs`.)
B. LS: replace `scan_workspace` with catalog fill; `snapshot()` compiles the
   open-closure Repository; keep diagnostics/hover/goto/references/rename
   tests green on existing small workspaces (regression gate).
C. Benchmarks: open N real modules in a synthetic giant tree; record wall
   time, RSS curve (memstep logs) and per-feature latency; iterate.
