# Catalog-scan startup regression (giant workspace) — analysis report

- **Date:** 2026-09-11
- **Test data:** https://github.com/YangModels/yang (explicit corpus input; every run records file count + total bytes).
- **Scope:** `netconf-language-server` startup (`fill_catalog`) over a 165 521-file /
  3.3 GB `.yang` tree; the `yrepo` catalog path it calls; the released Linux artifact.
- **Status:** root-caused, reproduced, **fixed and verified locally** (P0+P1+P2 + harness items 1–3; no release yet).
- **Next step:** ship the gnu/mimalloc Linux artifact (needs release approval), then land the §6 P3 CI perf gate.

---

## 1. Summary (TL;DR)

A giant workspace that used to "index in under a minute" (v0.2.0-era benchmark) now takes
**~330–366 s** of `initialize` blocking time before the language client even starts.

Three factors stack up; only two of them are in the Rust code:

| # | Factor | Kind | Measured effect |
| --- | --- | --- | --- |
| **1** | **The released Linux artifact is a static musl build** (`x86_64-unknown-linux-musl`). musl's allocator turns the catalog scan's allocation churn into a syscall storm. | **build/packaging** | **69 % of CPU in kernel** (433.9 s sys vs 197.8 s user on a 514 MB subtree); same work on **glibc = ~4 % sys**. Wall-clock **~10× worse** and parallelism buys almost nothing. |
| **2** | `yrepo 0.3 → 0.4` (v0.3.0) pulled grammar **0.4.x**, which made multi-megabyte vendor modules parse *fully* instead of collapsing to whole-file `ERROR`. That unmasked factor 1 by adding a full statement tree **+ one owned `String` per CST leaf** for those files. | dependency bump | `Catalog::scan` on a 5.9 MB module: old stack did almost no work (whole-file `ERROR`, 0.068 s); new stack pays 0.38–0.43 s (of which only 0.16 s is the parse). |
| **3** | `Catalog::scan` is not header-only: it parses, builds the statement tree, collects comments **and tokens**, collects errors — then keeps ~6 header fields. New in yrepo 0.5.0, `augment_quoted_fragments`→`push_fragment` linear-scans the whole token vector per quoted run / `+` (≈ O(tokens × fragments)). | **code** | Same 5.9 MB module: yrepo **0.4.0 = 0.38–0.43 s**, yrepo **0.5.0 = 4.6 s** (**11×**). Current HEAD is therefore *worse* than the 0.3.0 the user measured. |

Ruled out by measurement (do not chase these again):

- `71262ee` "fill catalog in parallel" — the rayon switch is the **mitigation**, not the cause.
- Grammar **0.4.0 vs 0.4.1** — equal on the giant modules (141 ms vs 140 ms).
- The LS's per-file `canon_url` (`dunce::canonicalize`) — 1.7 s vs 2.6 s for 4 000 files (noise).
- Per-file URL mapping cost generally — `canon-only` over 20 000 files = 0.072 s.

Reality check on the old baseline: even the pre-regression stack was **170 s sequential**
(`catmem`) / **218 s** (LS, `docs/memory-findings.md`) on this tree. The "under 1 min" figure was
the **standalone parallel benchmark** (`serveperf` grid, 23.5 s), not the language server.

---

## 2. Impact

- Users on giant trees (YangModels-style vendor collections) see a **5.5–6 min** dead window on
  every cold start, with no language features available (`initialize` is blocked on the scan by
  design — `src/server.rs::initialize` → `ensure_scanned`).
- The same musl penalty applies to every other parse/ingest-heavy path (open-closure materialise,
  reference index, diagnostics pull), so this is not confined to startup.
- The v0.5.0 token-recovery change means an upgrade will *not* fix it.

---

## 3. Reproduction recipe

All measurements below were taken on this machine (16 cores, 15.5 GB RAM, `/dev/sdd`),
release profile, with the corpus at an explicit path:

```bash
CORPUS=/home/dev/workspace/yang-modules/yang   # 165 521 *.yang, 3.3 GB of source
```

### 3.1 Shipped binary, end-to-end (the user-visible number)

Drive the installed server with a minimal `initialize` and read its `window/logMessage`
notifications (driver to be added as `scripts/lsp_scan_driver.py`; full body in Appendix A):

```bash
BIN=~/.vscode-server/extensions/k19.netconf-0.3.0/resources/bin/netconf-language-server-linux
python3 scripts/lsp_scan_driver.py "$BIN" "$CORPUS"
# → "workspace catalog scan END after <T>s (parallel, rayon pool)"
#   reproduced: 329.694 s (user log: 366.365 s)
```

Thread/CPU/RSS sampling during the scan (`scripts/lspsample.py`, §Appendix B):

```bash
python3 scripts/lspsample.py "$BIN" "$CORPUS/<large vendor subtree>"   # 9 797 files / 514 MB
# → threads=35  user=197.8s  sys=433.9s  (69 % sys)  RSS peak ≈ 1.7 GB
```

### 3.2 Library level (what the code *should* cost)

No in-repo harness exists yet for this today — the numbers below come from the throwaway
`modes` binary described in §7 item 1 (a `yrepo` example that runs one batch through
`CatalogIndex::scan_many_files` and once through `scan_many_files_with`), built with the
`parallel` feature and resolving grammar 0.4.1:

```bash
# throwaway harness used for this report (same code as the proposed yrepo/examples/scanbench.rs)
cargo build --release --features parallel
./target/release/modes "$CORPUS"     # sequential + parallel + parallel-with-canonical-urls
```

Recorded results:

| run | wall | note |
| --- | --- | --- |
| sequential, full corpus | **177.1 s** (1.07 ms/file) | matches docs' 170 s / 218 s |
| parallel, full corpus | **23.3 s** (7.59×), HWM 1 566 MB | matches the 23.5 s grid baseline |
| sequential, one large vendor subtree (9 797 files / 490 MB) | 28.7 s | |
| parallel, same | **4.8 s (5.9×)**, HWM 2.72 GB | |

### 3.3 Grammar A/B (raw parse, no yrepo)

```bash
# build the same tiny parse harness against =0.3.0, =0.4.0, 0.4.1
<harness> "$CORPUS" --skip 4600 --limit 4000   # slowest files reported
```

| grammar | slowest multi-MB vendor module |
| --- | --- |
| 0.3.0 | 57 ms — but `root=ERROR, has_error=true` (whole-file collapse) |
| 0.4.0 | 141 ms |
| 0.4.1 | 140 ms |

---

## 4. Measurements used for the verdict

| # | object | condition | result |
| --- | --- | --- | --- |
| 1 | shipped `0.3.0` binary (musl), full corpus | 165 521 files | **329.694 s**, 5 371 names |
| 2 | same binary, one large vendor subtree | 9 797 files / 514 MB | **54.8 s**, 35 threads, **user 197.8 s / sys 433.9 s**, RSS ≈ 1.7 GB |
| 3 | glibc harness, same subtree | seq + par + par-canon | seq **28.7 s**, par **4.8 s (5.9×)**; `/usr/bin/time`: user 161.59 s / **sys 6.18 s** |
| 4 | glibc harness, full corpus | yrepo 0.4.0 + grammar 0.4.1 | seq **177.1 s**, par **23.3 s (7.59×)** |
| 5 | `Catalog::scan`, 5.9 MB vendor module | yrepo 0.4.0 | **0.38–0.43 s** |
| 6 | `Catalog::scan`, same file | yrepo 0.5.0 (HEAD) | **4.6 s** (11×) |
| 7 | raw parse, same file | grammar 0.3.0 / 0.4.0 / 0.4.1 | 0.068 s (`ERROR`) / 0.159 s / 0.159 s |
| 8 | per-file URL mapping | 20 000 files, sequential | 0.072 s total (`canon`), 0.005 s (`Uri` only) |

Derived: identical work costs **~20× more CPU** in the shipped binary (632 s vs ~57 s per pass on
the subtree), and its parallelism is ineffective in wall-clock terms (10.6 cores busy ⇒ only
1.9× the throughput of one glibc core).

---

## 5. Root-cause chain

```
v0.3.0 dependency bump  yrepo 0.3 -> 0.4   (LS c62a1738 / 71262ee window)
        │  grammar 0.3.0 -> 0.4.x
        ▼
multi-MB vendor modules:  whole-file ERROR (cheap)  ->  full clean parse (expensive)
        │  yrepo catalog path does a FULL document extraction
        ▼
allocation churn:  Statement per node + one owned String per CST leaf
        │  shipped Linux artifact is a STATIC MUSL build
        ▼
musl allocator → syscall storm (69 % sys CPU) → ~10x wall time, no parallel benefit
        │
        ▼
366 s dead window on cold start
```

Contributing, independent of the above:

- `Catalog::scan` (`yrepo/src/catalog.rs`) builds `syntax::parse`'s full `ParsedDoc`
  (statements + comments + **tokens** + errors) and discards all but the header fields.
- yrepo 0.5.0 `syntax::augment_quoted_fragments` / `push_fragment` (`src/syntax.rs`):
  `push_fragment` does `tokens.iter().any(...)` **per recovered fragment** → quadratic in token
  count; it runs for every non-light parse, including catalog scans that will never read tokens.

---

## 6. Fix plan

### P0 — packaging (biggest lever, ~10×)

- [x] Keep the **static `x86_64-unknown-linux-musl`** artifact and install
      **`mimalloc` with its `override` feature** (Rust global allocator + C-level
      `malloc`/`free` interposition). `#[global_allocator]` alone was not enough:
      the tree-sitter parser's C allocations still hit musl's allocator (22.1 s vs
      1.9 s on a 9 797-file vendor subtree, 67 % sys). With `override`, static musl
      ≈ glibc (12.2 s vs 11.8 s full scan, ~3 % sys) and no glibc version floor is
      implied. `.github/workflows/github-release.yml` asserts the artifact is static.
- [x] Add a **gnu vs musl** A/B job to CI (manual `perf-ab.yml` skeleton) (same scan, same corpus sample) and publish both
      timings, so this cannot silently regress.

### P1 — make the catalog path actually header-only

- [x] `Catalog::scan` must not collect tokens/comments/errors and must not build the whole
      statement tree. Introduce a parse mode (e.g. `ParseMode::HeaderOnly`) used by
      `Catalog::scan`, keeping full extraction for `Repository` only.
- [ ] Acceptance: `Catalog::scan` on a 5.9 MB module ≤ 0.10 s (parse-bound) at yrepo HEAD.

### P2 — remove the quadratic token recovery

- [x] `push_fragment`: replace the linear membership scan with a `HashSet<(usize, usize)>`
      (or a merged, sorted pass), and skip the recovery entirely when the caller does not want
      the token stream (catalog/closure paths).
- [ ] Acceptance: yrepo HEAD `Catalog::scan` ≤ 0.45 s on the 5.9 MB module (i.e. no regression
      vs 0.4.0).

### P3 — durability

- [ ] Perf gate in CI: run the full-corpus catalog scan, compare against a stored baseline,
      fail on > 20 % regression (time and peak RSS).
- [ ] Re-baseline `docs/perf/giant-scale-2026-09/summary.csv` and `docs/memory-findings.md`
      after the fix — both currently record machine-local glibc numbers while users run musl.
- [ ] Optional: bound per-file work (tree-sitter progress callback / timeout) so one pathological
      module cannot stall the scan.

### Acceptance criteria (end-to-end)

| metric | today (shipped) | target |
| --- | --- | --- |
| full-corpus catalog scan, shipped Linux artifact | 330–366 s | **≤ 60 s** (expected 25–40 s) |
| sys share of CPU during the scan | 69 % | **< 30 %** |
| worst single-file `Catalog::scan` (≤ 6 MB module) | ~4.6 s (HEAD) | **≤ 0.45 s** |
| parallel speedup over sequential (library) | 1.9× effective | **≥ 5×** |

---

## 7. Harness work items (for the follow-up improvement round)

These are the tools the next round should add so the fix is measurable and guarded:

1. **[done]** **`yrepo/examples/scanbench.rs`** (promote the ad-hoc harness used here):
   - modes: `seq`, `par` (`CatalogIndex::scan_many_files`), `par-canon`
     (`scan_many_files_with` + LS-style canonical urls);
   - flags: `<dir> --skip N --limit M`, optional `--file-list`;
   - output: files, MB, ms/file, MB/s, user/sys CPU (via `/proc/self/stat`), VmHWM, and the
     top-N slowest files;
   - optional `mimalloc` feature to A/B the allocator without changing the target.
2. **[done]** **`netconf-language-server/scripts/lsp_scan_driver.py`**: spawn a server binary, send
   `initialize` with `rootUri`, print `window/logMessage` lines with timestamps, exit on the
   initialize response. This is what produces the user-visible number (§3.1).
3. **[done]** **`netconf-language-server/scripts/lspsample.py`**: sample `/proc/<pid>/status|stat`
   (threads, user/sys, RSS, ctx switches) while the scan runs — the tool that produced the
   69 %-sys evidence.
4. **Corpus manifest**: the corpus is an explicit input (workspace convention) — take it from
   `--corpus DIR` / `$YANG_CORPUS`; record file count + total bytes in every report so numbers
   stay comparable. Reference fixture used here: 165 521 files, 3 319 MB.
5. **CI perf job**: `gnu` vs `musl` × `{seq, par}` over a fixed subtree (one large vendor subtree,
   9 797 files / 514 MB) with stored baselines; fail on regression.

Reference harnesses used for this report (throwaway, under `/tmp`):
`/tmp/tsbench` (raw parse, grammar pinned),
`/tmp/ybench` (`one` / `region` / `modes` bins against yrepo 0.4.0 with and without `parallel`),
`/tmp/lspdriver.py`, `/tmp/lspsample.py`.

---

## 8. Open questions / caveats

- My local harness resolves **tree-sitter-yang 0.4.1** for `yrepo 0.4.0` (`"0.4.0"` allows both);
  the shipped Sep-07/08 artifact resolved **0.4.0**. Measured equal on the giant modules, so it
  does not change the conclusion — but pin the grammar explicitly in any future benchmark.
- The corpus is machine-local and mutable; the vendor subtree (4.7 GB of 4.9 GB) dominates all
  numbers. Always report file count + bytes.
- CPU/sys split and RSS were sampled at ~0.5 s granularity; peak values may be slightly higher.
- The 3.7× *user*-time difference between musl and glibc (197.8 s for one pass vs ~54 s per pass)
  is consistent with musl's simpler `memcpy`/`strlen`/`memmove` implementations as well; the sys
  gap is the dominant term and the one the packaging fix removes.
- Nothing was committed: LS and yrepo working trees are clean (temporary
  `examples/scanbench.rs` and the scratch yrepo worktree were removed).

---

## 9. Fix verification (local, 2026-09-11)

Implemented: yrepo `1d40695` (header-only `ParseMode` + linear fragment recovery +
`examples/scanbench.rs`), LS `7021ee6` (gnu target + mimalloc + `scripts/` +
manual A/B workflow). All numbers below are release builds on the machine
described in §3, corpus 165 521 files / 3 319 MiB.

### 9.1 Worst single file (same `scanbench` harness before/after)

| `Catalog::scan`, largest vendor module (5.96 MB) | wall |
| --- | --- |
| pre-fix HEAD (stash, same harness) | **4.966 s** |
| post-fix | **0.187 s** |

26.6× faster; target ≤0.45 s met. The ≤0.10 s stretch is **not reachable** at
0.4.1 grammar: the raw parse alone is 0.14–0.16 s (§3.3), and the scan is now
parse-bound (top-10 slowest files are all 5.5–6.0 MB NX modules at 0.19 s).
Note: the 111 s pre-fix figure from the first vendor-tree round was not
reproducible with this harness; the stash A/B above is the authoritative
baseline.

### 9.2 Full corpus (one process per mode, clean VmHWM)

| mode | wall | ms/file | VmHWM | user / sys |
| --- | --- | --- | --- | --- |
| `seq` | 98.0 s | 0.59 | 174 MB | 96.4 / 1.7 s |
| `par` | 12.7 s | 0.077 | 914 MB | 198.4 / 3.5 s |
| `par-canon` | 13.0 s | 0.079 | 919 MB | 197.9 / 7.0 s |

Parallel speedup 7.7× over sequential (target ≥5×). Pre-regression reference
(§3.2: seq 177 s, par 23.3 s) is now beaten on both axes.

### 9.3 Shipped-artifact path (gnu + mimalloc + fixed yrepo, local build)

`scripts/lsp_scan_driver.py` / `scripts/lspsample.py` against the full corpus:

```
[  0.253s] workspace catalog scan START: 165521 yang files (parallel, rayon pool)
[ 13.255s] workspace catalog scan END after 13.002s (parallel, rayon pool): scanned 165521/165521 …
[ 13.255s] workspace catalog ready in 13.250884s
t=12.8s threads=35 user=192.9s sys=7.2s rss=450 MB
```

End-to-end **13.0 s** (was 330–366 s) and **3.6 % sys** (was 69 %) — both
acceptance criteria met with large margin.

Allocator / libc follow-up (same subtree, 9 797 files; and full corpus):

| build | subtree wall | full scan | sys (full) |
| --- | --- | --- | --- |
| glibc, mimalloc as Rust allocator only | 2.44 s | 11.8–14.6 s | ~3 % |
| static musl, mimalloc as Rust allocator only | 22.06 s | — | 67 % |
| glibc, mimalloc `override` | 1.94 s | 11.8 s | ~3 % |
| **static musl, mimalloc `override` (shipped)** | **1.93 s** | **12.18 s** | **3.5 %** |

**Decision:** ship the static musl artifact with mimalloc `override` — glibc-level
performance with no glibc version floor, so the portable artifact is kept.

### 9.4 Acceptance status

| metric | before | target | measured |
| --- | --- | --- | --- |
| full-corpus catalog scan, shipped artifact (static musl + mimalloc override) | 330–366 s | ≤60 s | **12.2 s** |
| sys share during the scan | 69 % | <30 % | **3.5 %** |
| worst single-file `Catalog::scan` (≤6 MB) | 4.97 s | ≤0.45 s | **0.19 s** (stretch 0.10 not met: parse-bound) |
| parallel speedup (library) | 1.9× effective | ≥5× | **7.7×** |

Remaining: §6 P3 (CI regression gate with stored baselines) and re-baselining
`docs/perf/giant-scale-2026-09/` (grid re-run in progress at the time of
writing) plus `yrepo/docs/memory-findings.md`.

## Appendix A — LSP scan driver (recreate as `scripts/lsp_scan_driver.py`)

```python
#!/usr/bin/env python3
"""Send `initialize` for a rootUri, print server log notifications, report duration."""
import json, subprocess, sys, time

binpath, target = sys.argv[1], sys.argv[2]
timeout = float(sys.argv[3]) if len(sys.argv) > 3 else 900.0
uri = target if "://" in target else "file://" + target

def frame(obj):
    b = json.dumps(obj).encode()
    return b"Content-Length: %d\r\n\r\n" % len(b) + b

proc = subprocess.Popen([binpath], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.DEVNULL)
proc.stdin.write(frame({"jsonrpc": "2.0", "id": 1, "method": "initialize",
                        "params": {"processId": None, "rootUri": uri, "capabilities": {}}}))
proc.stdin.flush()

t0, buf = time.time(), b""
def read_msg():
    global buf
    while True:
        i = buf.find(b"\r\n\r\n")
        if i >= 0:
            n = int([h.split(":", 1)[1] for h in buf[:i].decode(errors="replace").split("\r\n")
                     if h.lower().startswith("content-length")][0])
            if len(buf) >= i + 4 + n:
                body, buf = buf[i + 4:i + 4 + n], buf[i + 4 + n:]
                return json.loads(body)
        chunk = proc.stdout.read1(65536)
        if not chunk:
            return None
        buf += chunk

while time.time() - t0 < timeout:
    msg = read_msg()
    if msg is None:
        break
    el = time.time() - t0
    if msg.get("method") == "window/logMessage":
        print(f"[{el:8.3f}s] LOG: {msg['params'].get('message')}")
    elif "id" in msg and "method" not in msg:
        print(f"[{el:8.3f}s] initialize done")
        break
proc.kill()
```

## Appendix B — CPU/RSS sampler (recreate as `scripts/lspsample.py`)

Same framing as Appendix A; after sending `initialize`, loop with `select([stdout], [], [], 0.5)`
while calling this every iteration:

```python
def proc_stat(pid):
    with open(f"/proc/{pid}/stat") as f:
        p = f.read().split()                      # fields 14/15 = utime/stime (jiffies)
    with open(f"/proc/{pid}/status") as f:
        s = f.read()
    g = lambda k: next((l.split()[1] for l in s.splitlines() if l.startswith(k)), "0")
    return g("Threads:"), g("VmRSS:"), int(p[13]), int(p[14]), \
           g("voluntary_ctxt_switches:"), g("nonvoluntary_ctxt_switches:")
```

Sample output that identified the allocator problem:

```
t=  56.0s threads= 35 user=  196.5s sys=  433.4s rss=1254.1 MB vol_ctx=9 nonvol_ctx=1
[  56.749s] LOG: workspace catalog scan END after 54.849s (parallel, rayon pool): scanned 9797/9797 …
```

## Appendix C — references

| what | where |
| --- | --- |
| LS parallel-scan switch (**not** the cause) | `netconf-language-server` `71262ee chore: fill catalog in parallel` |
| LS v0.3.0 release (dependency bump to `yrepo 0.4`) | `62a1738` |
| LS v0.2.0 benchmark commit ("under 1 min") | `69b2904` |
| CI musl target | `.github/workflows/github-release.yml` (`x86_64-unknown-linux-musl`) |
| LS scan implementation | `src/server.rs` `fill_catalog` / `ensure_scanned` / `initialize` |
| Catalog API | `yrepo/src/catalog.rs` (`Catalog::scan`, `CatalogIndex::scan_many_files_with`) |
| Parse/extraction | `yrepo/src/syntax.rs` (`parse`, `collect_tokens`, `augment_quoted_fragments`, `push_fragment`) |
| Quadratic token recovery introduced | `yrepo` `cb67a88 fix: emit tokens for concatenated quoted fragments` (0.5.0) |
| Prior baselines | `netconf-language-server/docs/perf/giant-scale-2026-09/`, `yrepo/docs/memory-findings.md` |
| Dependency chain | `yrepo` v0.3.0→ts-yang 0.3.0 · v0.4.0→ts-yang 0.4.0 · v0.5.0→ts-yang 0.4.1 |
