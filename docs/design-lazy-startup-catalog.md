# Lazy startup catalog (design + recon)

Status: implemented and verified locally (2026-09-11); all five acceptance items measured. Goal: `initialize` must not parse the whole tree;
startup builds a cheap path/name index and the open document's closure is
resolved on demand. Test data: https://github.com/YangModels/yang (explicit
corpus input; counts recorded per run). Numbers below: 165 521 `.yang` files,
3 319 MiB, 16 cores.

## 1. Baseline

`Server::initialize` used to call `ensure_scanned()` before responding (see the
comment in `src/server.rs`), so the whole-tree header scan was inside the
client-visible startup path: ~12.2 s (static musl + mimalloc override) / ~11.8 s
(glibc). With the lazy startup index implemented, the measured startup is
**0.27–0.29 s warm** (~0.95 s on the first cold walk, before the index was
changed to move the walked paths instead of cloning them) and the log reports
`0 headers parsed`. Opening a standard RFC module resolved its closure by
parsing **205 candidate headers in 12.2 ms**, with 0 diagnostics.

## 2. Recon measurements (read-only, cheap)

| quantity | value |
| --- | --- |
| directory walk + collecting `*.yang` (Python proxy for `walk_yang_files`) | **0.12 s** |
| distinct basenames (basename minus `@rev`) | 5 423 |
| basename keys with >1 file | 4 076 (max 295 files for one key; 2 838 keys have >16) |
| basename ↔ declared module/submodule name (2 070-file sample, header regex) | **99.8 %** (5 mismatches: hyphenated variant files declaring a shorter name) |
| candidate header parses for one opened module's immediate imports | standard sample: median 8, p90 406, max 626 · global spread: median 260, p90 661, max 1 761 |
| mean filename candidates per imported name | 66–114 |
| candidates whose filename carries `@date` | most multi-candidate keys have none: 1 335/1 587 groups have no dated file, 14 fully dated, 238 mixed |
| "max filename date (no-date = newest)" heuristic vs true header `revision` | **88.7 %** correct on 300 sampled keys |

Conclusions that shape the design:

1. The walk/index is negligible (~0.1 s); the expensive part is header parsing.
2. Filename-based resolution is **not** correctness-safe for canonical-latest:
   most candidates are undated, and the date heuristic misses ~11 % of winners.
   Therefore a needed name must be resolved by parsing its candidate headers
   (same rule as `CatalogIndex::resolve`: highest revision, parse-clean first).
3. That on-demand parsing is bounded and cheap when parallel: a few hundred to
   ~1 800 header scans per opened module (~0.08 ms/file parallel ⇒ tens of ms,
   ≤~0.2 s worst observed), and results are cached.
4. ~0.2 % of files declare a name that differs from their basename, so a name
   can have zero filename candidates; a bounded fallback is required.

## 3. Design

### 3.1 Startup (`initialize`)

- Walk the workspace and build `HashMap<basename, Vec<PathBuf>>` (plus the
  canonical `file://` url per path). **No parsing.**
- Return capabilities immediately; log `startup index: N files in X ms,
  0 headers parsed`.
- Keep the existing `Repository` (open buffers full-parse) and the closure sync,
  but make the closure resolve names through the lazy catalog below.

### 3.2 On-demand closure resolution

- `didOpen`/`didChange` seeds (imports/includes/belongs-to) drive a BFS as today.
  For a name not yet in `CatalogIndex`:
  1. look up its basename candidate list (cheap);
  2. if candidates exist, parse **their** headers in parallel (reuse yrepo's
     parallel batch catalog scan over the candidate subset, e.g. a new
     `CatalogIndex::resolve_lazy(name, pin, candidates, url_for)` helper or
     `scan_many_files_with` on the subset) and pick the winner with the existing
     `resolve` rule (exact `revision-date` pin first, else highest revision,
     parse-clean first);
  3. insert only the winner into the catalog; cache the lookup (including
     negative results) so repeated opens do not re-parse.
- New names discovered while expanding the winner's own imports repeat the BFS.
- Optional path-context preference (same directory as the importer) before the
  global candidate set, to cut candidate counts; must fall back to the global
  set to stay correct.

### 3.3 Bounded fallback

- Zero filename candidates (the ~0.2 % mismatch case): try the bounded
  **prefix fallback** first (`devs` → `devs-spi.yang`, capped at 256 files); if
  that also finds nothing, record the name as missing and log it — the import
  then surfaces as the existing unresolved-import diagnostic, same as an import
  that is genuinely absent. Verified end-to-end on a small workspace whose
  module `devs` lives in `devs-spi.yang`: 1 candidate header parsed, 0
  diagnostics.
- Candidate sets above the 256 cap are truncated (and the resolution is logged)
  so pathological names stay predictable.

### 3.4 Whole-tree features

`ReferenceIndex` (find-all-references / rename) stays lazy and
progress-visible, unchanged. Startup must not build either index.

## 4. Harness / acceptance

- `scripts/lsp_scan_driver.py` gains a startup-latency mode (time to
  `initialize` response + the `startup index` log) and, for on-demand resolution,
  a `--did-open <file>` mode that reports the closure-resolution log
  (`names resolved, candidate headers parsed, elapsed`).
- Acceptance (from the goal): cold `initialize` on the 165k corpus ≪1 s; opening
  a module yields correct diagnostics/goto (no unresolved-import regressions);
  references/rename unchanged with a measured before/after lazy build time; no
  whole-tree parse at startup (asserted by the log/counters); bounded fallback
  documented and exercised by a forced-mismatch test.
- Correctness guard: the lazy winner uses the same `resolve` rule as a
  whole-tree `CatalogIndex`; unit tests cover pin, highest-revision and
  parse-clean tie-breaks, plus prefix fallback and missing-name reporting.
- Measured (external corpus, release): startup **0.27–0.29 s** /
  `0 headers parsed` (was ~12 s); open closure 205 candidate headers in
  12.2 ms with 0 diagnostics; `ReferenceIndex` build (lazy, unchanged
  behaviour, progress-visible) **24.99 s** with the current parser fixes vs
  **418.60 s** with the published 0.5.0 parser — same 7 559 734 occurrences
  and the same 54 710 references returned.

## 5. Implementation steps

1. yrepo: add the lazy resolution helper (candidate subset → parallel header
   scan → winner via `resolve`), with tests (pin, highest-revision,
   parse-clean-tie, zero-candidate).
2. LS: replace `ensure_scanned()` in `initialize` with the basename index;
   make `sync_open_closure` resolve lazily and grow the catalog; keep
   `ensure_refidx` untouched; add the background fallback scan.
3. Harness/logs + the correctness A/B sample; then measure startup and
   closure-resolution latency on the corpus.
4. Docs/CHANGELOG; commits per repo.
