# Benchmarks

Measured performance of `netconf-language-server` on a large real workspace.
Every number here is reproducible from the in-repo probes listed under
"Reproducing".

## Test environment

| | |
| --- | --- |
| CPU | AMD Ryzen 7 9700X (8 cores / 16 threads) |
| RAM | 15.9 GB |
| OS / kernel | Linux 6.6 (`microsoft-standard-WSL2`) |
| Build | `--release`, yrepo `parallel` (rayon, 16 workers) |
| Date | 2026-09-11 |

## Corpus

[YangModels/yang](https://github.com/YangModels/yang), taken as an explicit
input: **165 521 `.yang` files**, 3 319 MiB of source (4.8 GB on disk).

## Startup (the number a user feels)

| metric | measured | notes |
| --- | --- | --- |
| `initialize` response | **0.27–0.29 s** | parse-free: directory walk + basename index (`NameIndex`) + empty catalog |
| headers parsed at startup | **0** | logged as `startup index: … (0 headers parsed)` |
| first open of a module (closure + diagnostics) | **12.2 ms / 205 candidate headers**, 0 diagnostics | only the imported names' candidates are parsed; results cached |

The startup index used to be a whole-tree header scan (~12 s on this corpus with
the current parser; 330–366 s in the pre-fix static-musl artifact). It is now
deferred: whole-tree parsing happens only for explicitly whole-tree features
(below). See `design-lazy-startup-catalog.md`.

## Whole-tree features (explicitly requested, progress-visible)

| feature | measured | notes |
| --- | --- | --- |
| find-all-references / rename (first request) | **25.0 s** | lazy `ReferenceIndex`: 165 180 docs, 7 559 734 occurrences; `$/progress` notifications shown |
| same, with the published 0.5.0 parser | 418.6 s | same occurrences and same 54 710 references returned — the parser/retention fixes are a 16.7× speed-up, not a behaviour change |

## Bulk catalog scan (tooling baseline)

`examples/serveperf` / `examples/scanbench` (yrepo) can still measure a
whole-tree catalog scan when a tool needs one: 12.2 s static-musl +
mimalloc `override` (11.8 s glibc), sequential 98 s — see yrepo
`docs/memory-findings.md`. The language server **does not** do this at startup
any more.

## Reproducing

```bash
# startup latency + server log timestamps (stdlib python only)
python3 scripts/lsp_scan_driver.py <server-binary> /path/to/YangModels/yang

# CPU/RSS sampling during a scan
python3 scripts/lspsample.py <server-binary> /path/to/YangModels/yang

# library-level catalog scan (seq / par / par-canon), from yrepo
target/release/examples/scanbench /path/to/YangModels/yang --mode all --top 5
```

Details and per-step reports: `docs/design-lazy-startup-catalog.md`,
`docs/perf/catalog-scan-regression-2026-09-11.md`,
`docs/perf/giant-scale-2026-09/`, and yrepo `docs/memory-findings.md`.
