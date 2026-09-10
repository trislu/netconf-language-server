# Serving very large YANG trees (catalog + closure + text-light)

Status: implemented (phases A + B), benchmark pending (phase C) — 2026-09-06.
Motivation & measurements: yrepo `docs/memory-findings.md` (full-parse
retention ~0.1–0.35 MB per real module, catalog ~7 KB/file, text-light −16%
ingest; a 163k-file full compile cannot fit one process).

## Behavior now

Startup builds only a **parse-free basename index** (`build_startup_index`:
walk + `NameIndex`) and an empty catalog — **no header is parsed** (0.27–0.29 s
on the 165k-file corpus; see `benchmarks.md`). Header parsing is deferred to the
open closure: `sync_open_closure` resolves names on demand from their candidate
files (`resolve` over the needed name's candidates, bounded and cached, with a
bounded prefix fallback), and the yrepo `Repository` holds only the **open
closure** — open buffers (full parse) plus every on-disk module they can reach
(imports/includes with revision-date pins, belongs-to parents), parsed
text-light. `snapshot()` compiles that repository, so both retention and compile
cost scale with what the user is looking at, not the tree. Whole-tree work stays
lazy (`ReferenceIndex`, with progress). See `architecture.md` §6.1 and
`design-lazy-startup-catalog.md`.

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

## Phases (each with memstep/memcomp or LS integration tests)

A. yrepo: catalog registry with (name, rev) canonicalization + import/include
   lookup API (`CatalogIndex::resolve(name, rev)`) and closure building
   (`build_closure_repository`; `examples/closure.rs` runs it). **DONE**
   (yrepo commits: header-only `Catalog`/`Catalog::scan`, `CatalogIndex`,
   pinned `resolve`, text-light parse mode).
B. LS: `build_startup_index` + lazy `sync_open_closure` replace the whole-tree startup scan;
   `snapshot()` compiles the open-closure Repository; the feature unit tests
   stay green (regression gate). **DONE** (this repo: `closure.rs`, `server.rs`
   sync paths). Open buffers parse full; closure members parse text-light.
C. Benchmarks: open N real modules in a synthetic giant tree; record wall
   time, RSS curve (memstep logs) and per-feature latency; iterate.
   **DONE** — statistical grid on the restored giant population (10k-step
   intervals × 5 runs each): catalog wall ≈3.6 s @10k → ≈24.7 s @full,
   serving stays flat. See `docs/perf/giant-scale-2026-09/report.html`
   (raw results + summary + charts + repro scripts).
