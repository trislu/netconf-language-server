# Feasibility study — replacing `tree-sitter-yang` in `yrepo`

> Status: **draft for review** (2026-09-10). Decision document, not a plan of
> record.
>
> Question under study: is it worth replacing the `tree-sitter-yang` grammar +
> tree-sitter runtime as the YANG parser inside `yrepo` — motivated by the
> claim that tree-sitter's **error recovery is unpredictable** — and if so,
> with what? Candidate options analysed: keep-and-harden tree-sitter, a
> **hand-written scanner + recursive-descent parser**, **PEG**/combinator
> parsers, **LALR** (lalrpop), and **binding an existing YANG parser**
> (libyang / pyang semantics). Rough verdicts and a phased path are included.

---

## 1. TL;DR (for a fast decision)

- The parser is **already an internal detail of a single file** —
  `yrepo/src/syntax.rs` — and the tree-sitter CST is **already dropped
  eagerly** after an eager walk into `yrepo`'s own `Statement`/`Token` model.
  `netconf-language-server` never touches the YANG grammar directly.
- So the **only real lever** a replacement buys is *predictable, controllable
  error recovery* (and, secondarily, dropping the tree-sitter runtime + the
  codegen dependency). It will **not** move the RSS needle by itself — the
  CST is not retained, and the dominant retention is the derived model +
  source (see `yrepo/docs/memory-findings.md`).
- The **hardest requirement to reproduce** is not "parse valid YANG"; it is
  the current *recovery granularity*: per-site `ParseError`s, no
  whole-file collapses (`examples/inspect.rs` counts a collapse when a
  parse-error covers `≥ 95%` of the file from byte 0), graceful `ERROR` /
  `MISSING` handling, and the `Statement.range`-overshoot / terminator
  conventions downstream code depends on.
- **Recommendation:** do **not** chase this as "fix error recovery in
  tree-sitter" (limited) and do **not** leap straight to a PEG grammar
  (recovery story is weak). The strongest option is a **hand-written scanner
  + recursive-descent statement parser with explicit statement-boundary
  synchronization**, implemented *inside* `syntax.rs`'s contract, with
  tree-sitter-yang kept as an **A/B oracle** for corpus parity during the
  transition. It gives recovery that is *reasoned about in one place*, and it
  pays for itself twice over by also serving the **lean header scanner** the
  catalog/closure work already wants.
- Recommended sequence: **(0)** corpus parity harness → **(1)** lean header
  scanner (catalog path) → **(2)** full statement parser behind a flag →
  **(3)** flip default once parity + no-collapse invariants hold → **(4)**
  drop the tree-sitter-yang dependency.

---

## 2. Background — how YANG parsing works today

### 2.1 Call chain

```
Repository::upsert(url, source)                     yrepo/src/lib.rs
  └─ Yang::new_opt(text, light)                     yrepo/src/yang.rs
       └─ syntax::parse_opt(source, light)          yrepo/src/syntax.rs   ← THE touch-point
            ├─ tree_sitter::Parser::parse()         (LANGUAGE from tree-sitter-yang)
            ├─ collect_errors / collect_comments / collect_tokens
            ├─ text_statement_ranges (light mode)
            ├─ find_top_module → build_statement     → Statement tree
            ├─ drop(tree)                            ← CST dropped here on purpose
            └─ → ParsedDoc { text, root, comments, tokens, parse_errors }
Yang keeps only ParsedDoc; Repository keeps Vec<Yang> (no CST anywhere).
```

Only two tree-sitter dependencies exist (`yrepo/Cargo.toml`):
`tree-sitter = "0.26"` (runtime) and `tree-sitter-yang = "0.4.1"`
(grammar bindings; `yrepo` uses only its `LANGUAGE` re-export — not
`NODE_TYPES`, not the codegen'd `NodeKind`).

`netconf-language-server` does **not** depend on `tree-sitter-yang`; its
`tree-sitter` usage is for RFC 7951 XML / JSON *instance* documents only.
YANG parsing is fully behind `yrepo`.

### 2.2 What the server/`yrepo` actually consume

Everything downstream operates on the **derived model**, in byte ranges
(`text.rs` canonicalises byte offsets):

| Consumer | Uses | Notes |
| --- | --- | --- |
| `semantic_token.rs` | `Statement` preorder (`keyword`, `arg.range`) + `tokens()` | two-pass; byte→UTF-16 via rope |
| `goto/hover/completion/references` | `Statement::narrowest_at(byte)` | caret resolved to byte first |
| `format.rs` | `comments()` + `StatementEnd` spans | statement terminators matter |
| `yrepo compile/schema` | `Statement.arg` (`.name()`, `.logical`) + `range` | diagnostics in bytes |
| catalog / refidx | header fields from a transient parse | no CST kept |

The `ParsedDoc` model to reproduce (`yrepo/src/syntax.rs`):

- **`Statement`** — `kind: StatementKind` (one variant per YANG keyword +
  `Unknown(String)` for vendor extensions), `range` (**whole grammar node —
  may overshoot the terminator over trailing whitespace**), `keyword:
  Option<Range>`, `arg: Option<Argument>` (`range` + dequoted/`+`-joined
  `logical` text), `end: Option<StatementEnd>` (`;` or `{`/`}` with spans,
  `None` when the terminator was not recovered), `children: Vec<Statement>`.
- **`Comment`** — `range`, `kind: Line|Block`, text with markers.
- **`Token`** — `kind: Comment|Keyword|Identifier|String|Number|Boolean|
  Operator|Other`, `range`, raw `text`. Quoted strings are **monolithic**
  (quotes included, never split). A text-based **fragment augmentation**
  (`augment_quoted_fragments`) re-scans raw argument text to recover hidden
  quoted runs / `+` operators the grammar lexed invisibly.
- **`ParseError`** — `range`, message. Per-site (`ERROR` = "unexpected",
  `MISSING` = "missing"). Never fatal.
- **`text_light` mode** — drops `description`/`reference`/`organization`/
  `contact` from both the tree and the token stream. Already byte-range
  based, not CST based — portable.

### 2.3 Key facts that shrink the problem

1. **No CST is retained.** The `drop(tree)` is deliberate memory policy
   (`syntax.rs`), and nothing outside `syntax.rs` holds a `Node`. So "switch
   parser to save memory" is **not** the pitch.
2. **No incremental parsing.** Every request is a full `parse(&source, None)`;
   the server re-upserts the whole changed text on each `did_change`. A
   replacement does not need incrementality.
3. **Single-file swap surface.** Grep confirms no direct
   `Node`/`Cursor`/`TSNode` use outside `syntax.rs`.
4. **Native tree-sitter consumers are unaffected.** Zed, Neovim
   (nvim-treesitter) and `tree-sitter highlight` read
   `tree-sitter-yang/queries/highlights.scm` directly and keep using the
   grammar crate regardless of what `yrepo` does.

---

## 3. Why tree-sitter error recovery is "unpredictable" here

### 3.1 The mechanism (at the level that matters for this study)

tree-sitter is a GLR parser whose recovery is **implicit** — there are no
recovery actions an author writes. When parsing fails at a point, the parser
uses error costs and lookahead to do one or both of:

- open an **`ERROR` node** over the region it cannot match, then skip input
  until it can resynchronise; and/or
- **insert a `MISSING` node** for a token it believes was omitted, so
  parsing can continue as if that token were present.

Both are decisions the grammar author does not directly steer, and their
*extent* is emergent from the grammar rules + the input. This shows up in
`yrepo` as:

- **`ERROR`/`MISSING` spans that are hard to predict.** A single bad token
  near the top of a module can make the parser skip a large region (an
  `ERROR` spanning most of the file) or synthesise `MISSING` terminators that
  re-parent following statements under the wrong ancestor. `yrepo` localises
  per site (`is_error`/`is_missing`), but *where* and *how big* each site is
  is tree-sitter's call.
- **`MISSING`-node phantom structure.** `build_statement` must already skip
  `is_missing()`/`is_error()` children when recovering keywords, args and
  terminators (`statement_end` returns `None` if only one brace was
  recovered). The downstream code is written *defensively against* the
  recovery output — evidence the output is not cleanly shaped.
- **The observed failure modes.** Historical whole-module collapses
  (`parse-error` covering `≥ 95%` of the file from byte 0 —
  `examples/inspect.rs`) cascaded into `not-a-yang-document` and unresolved
  import/typedef/grouping noise in every importer. The PHASE-0
  error-localisation work and the grammar fixes (`044`…`048`,
  `038`…`043`) were all *chasing* recovery granularity: either the grammar
  rejected a valid construct (fixed in `tree-sitter-yang`) or recovery
  swallowed a subtree.
- **Residual corpus.** ~60 whole-corpus parse-errors remain, overwhelmingly
  content corruption in `experimental/ietf-extracted-YANG-modules` (broken
  quotes, orphan statements, MIB transcripts) rather than grammar gaps — but
  each still risks a subtree collapse instead of a crisp one-statement error.

### 3.2 Framing

The practical complaint is not "tree-sitter can't parse YANG"; it is:

> When a file is *slightly* wrong, we cannot guarantee *how much* damage one
> bad statement does, and we have no single place to reason about "recover to
> the next statement at the same nesting level".

That is the property a replacement should guarantee.

---

## 4. The contract a replacement must honour (acceptance criteria)

Derived from §2.2 — treat these as non-negotiable:

**Correctness / parity**
1. Parse-clean corpus → **identical** `ParsedDoc` (statement tree, ranges,
   tokens, comments) to today's tree-sitter output. This is the A/B parity
   bar; it is what makes a transition verifiable.
2. RFC 7950 §14 faithfulness + vendor/`Unknown` tolerance, **including every
   edge case already fixed in the grammar**: `max-elements unbounded;`, bare
   symbol arguments (`m^-X`, `meter^2.second-1`), bare `enum` names with
   symbols (`n+1`), `default` bare symbols (`00:00:15.0`, `syslogtypes:local7`),
   arbitrary backslash escapes in quoted strings (`\*`, `\S`, `\.`), and
   concatenated / trailing-whitespace quoted arguments for
   `key`/`unique`/`range`/`length`.
3. Range conventions: byte ranges everywhere; `Statement.range` may overshoot
   the terminator over trailing whitespace; `keyword`/`arg`/`end` carry exact
   spans; `StatementEnd` is `None` when the terminator is unrecovered.
4. Token granularity quirks: monolithic quoted-string tokens (quotes
   included), `+` operators, skip of missing/error leaves, and the
   fragment-augmentation outputs for hidden quoted runs.

**Recovery (the whole point)**
5. Per-site `ParseError`s; **no whole-file collapse** on malformed input; a
   bad statement damages at most itself and its direct descendants, never a
   sibling or the module header.
6. HTML/XML-mislabeled `*.yang` (first byte `<`) still yields the single
   `not-a-yang-document` warning with no error spam.

**Operational**
7. Whole-document parse per change (no incrementality).
8. `text_light` remains a parse-time option with identical semantics.
9. Parse speed and retention at parity or better (baseline: whole-corpus
   `inspect` ≈ 0.45–0.6 s; per-file retention dominated by derived model +
   source, *not* CST).
10. No API change outside `syntax.rs`; keep the header-scan (catalog) and
    full paths both on the new parser.

---

## 5. Candidate options

### Option A — Stay on tree-sitter-yang; harden recovery *in the grammar*

Try to make recovery predictable by adding explicit error/`ERROR`
productions to `grammar.js` (recovery rules around statement boundaries) and
further tightening `syntax.rs`'s defensive walking.

- Pros: incremental; zero model/range risk; the grammar crate stays the
  single source for Zed/Neovim/CLI too; sub-second corpus parse already
  demonstrated; the many grammar-edge fixes are already landed.
- Cons: tree-sitter gives **no author-level control over recovery shape** —
  you can add rules that reduce `ERROR` span sizes, but you are still
  fighting the GLR cost model; each fix is a new special case with no
  guarantee of a bound (an "at most one statement" bound is simply not
  expressible); grammar grows hacky (see `parser-regen` notes); you remain
  coupled to the codegen pipeline and the runtime.
- Verdict: **good for reducing symptoms, not for the goal.** A stopgap, not
  a fix. Keep the grammar fixes regardless (they benefit the .scm
  consumers).

### Option B — Hand-written scanner + recursive-descent statement parser (recommended)

Lex YANG with a purpose-built scanner that already emits the exact `Token`
stream semantics (`String` monolithic with quotes, `+`, numbers, booleans,
operators, comments; raw-text fragment pass becomes unnecessary because the
lexer *is* the source of truth). Then parse statements with a recursive
descent over the YANG keyword table, where **every statement is a single
recovery unit**:

- each statement: parse `keyword`, optional `argument` (by argument form:
  unquoted word / quoted string / number / schema-nodeid / `;` / `{` block),
  then children or `;`;
- **recovery rule (the guarantee):** on an unexpected token inside a
  statement, emit one `ParseError` for that statement's span, then
  synchronise to the next `;` or a matching brace at the *same nesting level*
  and continue with the next sibling. A malformed statement therefore cannot
  consume a sibling, the header, or the rest of the file — an upper bound
  that is *written down in one function* instead of emergent from a GLR
  automaton;
- `StatementEnd::None` arises naturally (unterminated block → error +
  recovery at EOF);
- vendor `Unknown` statements parse as `prefix identifier` + permissive
  argument, matching today's tolerance;
- the lean **header scanner** (catalog path: name, revision, prefix,
  namespace, imports/includes) is the same parser stopped after the header —
  one parser, two depths.

- Pros: recovery is **local, explicit, and bounded by construction**; one
  place to reason about errors; no tree-sitter runtime or codegen dependency;
  can be *faster* (no CST allocation + walk + drop); the header scanner the
  memory work already wants comes nearly free; text-light and catalog logic
  (byte-range based) are untouched.
- Cons: the largest *authoring* effort — you re-derive RFC 7950 §14 +
  tolerance + every historical edge case by hand; range-overshoot and
  `StatementEnd` conventions must be copied exactly; needs a strong parity
  suite before flipping. Maintenance burden moves from `grammar.js` (shared
  with .scm consumers) to a YANG-specific Rust module.
- Verdict: **best fit for the stated goal** (predictable recovery) and the
  cleanest long-term maintenance story for `yrepo`; cost is concentrated up
  front and de-risked by keeping tree-sitter-yang as an A/B oracle.

### Option C — PEG via a parser-combinator / PEG crate

Representative: `pest` (true PEG), `nom` (combinators, ~recursive descent +
backtracking), `chumsky` (combinators with labelled errors and *built-in
recovery strategies*), `pom`.

- Pros: less boilerplate than full hand-writing; `chumsky` ships recovery
  (`skip`, `skip_then_retry_until`, nested recovery) — the closest to the
  "skip one statement" primitive; good error *messages*.
- Cons: PEG ordered-choice makes "try statement forms, else recover" awkward
  (no real ambiguity handling; a PEG's implicit backtracking can *mask* errors
  or pick surprising parses); recovery strategies are crate-shaped, and you
  still write the sync logic yourself; token/ranges quirks (monolithic
  strings, exact byte conventions, `logical` dequoted text) require care that
  a generic parser doesn't give you; another dependency with its own learning
  curve; `pest`'s error recovery in particular is weak and mostly error
  *reporting*.
- Verdict: viable middle ground **only if** you accept writing the recovery
  machinery yourself anyway — at which point Option B (no crate, full
  control) is usually cheaper than fighting a combinator's shape. `chumsky`
  is the interesting one if you want labelled errors + a token-free
  streaming model; treat as "Option B with a combinator scaffold".

### Option D — LALR / table-driven (lalrpop)

- Pros: deterministic, fast, good at unambiguous YANG.
- Cons: error recovery in LALR means writing **error productions** for every
  statement form (the same work as Option B but in a formalism with less
  runtime flexibility); backtracking and `Unknown`/vendor tolerance are
  painful; conflicts on YANG's keyword-as-keyword design are solvable but
  tedious; the grammar is essentially context-free but the recovery story is
  the worst of the hand options.
- Verdict: not recommended for a *recovery*-driven goal.

### Option E — Bind an existing mature YANG parser

- **libyang** (C, used by sysrepo/netopeer): mature, fast, real per-statement
  error handling, RFC 7950 1.0/1.1, tolerant of vendor extensions. Would need
  a C FFI layer (or the `libyang`/`yang2` Rust bindings), and its *model* is
  its own — you would map libyang's schema tree onto `yrepo`'s
  `Statement`/`Token` model, or change consumers. License BSD-3.
- **pyang**: the reference for tolerant parsing semantics, but Python —
  runtime-hostile for this Rust LSP; usable only as a *cross-check oracle*
  (already effectively what `pyang` is used for in grammar checks).
- Pros: battle-tested parser semantics; libyang's recovery is genuinely
  predictable per statement.
- Cons: heavy integration (FFI/build), model impedance, licence/audit, and it
  does not produce yrepo's byte-ranged `Statement`+`Token` model — the
  mapping cost lands on the same conventions in §4. Removes tree-sitter but
  adds a bigger external dependency.
- Verdict: the strongest *semantic* oracle; the least *fit* as an internal
  parser given the custom derived model. Revisit only if a real YANG schema
  library becomes a product goal rather than a parsing goal.

### Option F — Hybrid / staged (what is actually recommended)

**B + A together, staged:** keep tree-sitter-yang as the A/B oracle and for
the native-.scm consumers; implement Option B as a new `yrepo` parse path
behind a flag; run both over the corpus until `ParsedDoc` parity holds on
parse-clean files and no-collapse holds on the corrupt set; then flip the
default and delete the tree-sitter-yang dependency from `yrepo` (the
`tree-sitter-yang` repo stays, for Zed/Neovim/CLI and for cross-checking).

---

## 6. Comparison matrix

Criterion weights: recovery-predictability and integration cost dominate;
memory is a *weak* factor (CST already dropped).

| Criterion | A keep+patch grammar | B hand-written RD | C PEG/combinator | D LALR | E libyang FFI |
| --- | :-: | :-: | :-: | :-: | :-: |
| Recovery predictable & bounded | ✗ (emergent) | ✓✓ (one sync fn) | ~ (crate-shaped) | ✗ (error prods) | ✓ (per-stmt) |
| No whole-file collapse, guaranteed | ✗ | ✓ | ~ | ~ | ✓ |
| RFC §14 + vendor tolerance effort | 0 (done) | high (re-derive) | high | high | low (reuse) |
| Exact `Statement`/`Token`/range model fit | native | full control | friction | friction | impedance+FFI |
| `.scm` consumers (Zed/Neovim/CLI) | shared | n/a (separate) | n/a | n/a | n/a |
| Drop tree-sitter dep | no | yes | yes | yes | yes (adds C) |
| Header/catalog scanner synergy | no | yes (free) | ~ | no | no |
| Authoring/maintenance effort | low now, rising | high now, low later | medium | medium | medium |
| Risk to existing behavior | low | high (needs parity gate) | high | high | high |
| Performance | proven | likely ≥ | likely ≥ | likely ≥ | proven |
| Ecosystem/audit | small | none | one crate | one crate | C + licence |

**Reading:** B wins on the *reasons* you are doing this (recovery) and on
long-term fit; it loses only on up-front authoring effort, which the A/B
parity gate de-risks. E is the best "borrow someone else's semantics" option
if the derived-model mapping is acceptable. A alone does not satisfy the goal.

---

## 7. Recommended path (Option F concrete)

0. **Parity harness (do first, cheap).** A corpus runner that parses every
   `*.yang` with both the current tree-sitter path and the new path and diffs
   `ParsedDoc` (tree shape + ranges + tokens + comments + errors) on
   parse-clean files, and diffs the *collapse* histogram on the corrupt set.
   This is the definition of done for every later step.
1. **Lean header scanner (small, independent win).** Implement the catalog
   scan on the new hand-written scanner (stop after the module header). This
   lands the "7 KB/file catalog" direction (`yrepo/docs/memory-findings.md`)
   and exercises the lexer/statement skeleton on real RFC modules without
   needing full fidelity.
2. **Full statement parser behind a flag.** `syntax::parse_opt` gains a
   `ParserBackend` (or `features = ["hand"]`) switch; default stays
   tree-sitter. Iterate until §4 parity on parse-clean files and
   no-collapse + per-statement damage bound on the corrupt set.
3. **Flip default; fuzz.** Make the hand-written parser the default; keep the
   tree-sitter path compiled (feature) for one release as the oracle; run the
   vendored corpus + `scripts/audit.sh` + semantic-token baseline
   (`testdata/highlight/baseline.json`) to confirm zero behavioural drift.
4. **Remove the tree-sitter-yang dependency** from `yrepo` once the flip has
   soaked; `tree-sitter-yang` remains published for its native consumers.

**Suggested effort (one developer, focused):**
parity harness ~0.5 day; lexer ~1–2 days; statement parser (RFC 7950 §14 +
tolerance + edge cases) ~3–5 days; recovery/sync + error model ~1–2 days;
text-light + light/full unification ~1 day; corpus convergence + audit
re-runs ~2–3 days → **≈ 8–14 focused days** to flip, not counting the
parity soak. This is comparable to the original grammar-hardening effort and
buys a property (bounded recovery) that grammar hardening cannot.

---

## 8. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Silent divergence on valid files | A/B `ParsedDoc` diff over the whole corpus; semantic-token baseline gate |
| Re-introducing whole-file collapses on corrupt files | Explicit sync recovery (§3/§4.5); collapse histogram in the parity harness |
| Range/terminator convention drift breaks format/fold/highlight | Copy conventions verbatim from `syntax.rs`; assert identical ranges in tests |
| Vendor-`Unknown` and edge-case regressions (`038`–`048` cases) | Re-encode every historical grammar fix as a parser test up front |
| Cost overrun / abandonment mid-way | Keep tree-sitter path live behind the flag the whole time; the header scanner ships value independently |
| Over-engineering recovery | Start with one rule (sync to next `;` at same depth); add only what corpus shows is needed |

## 9. Open questions (for the decision)

1. Is a *bounded* recovery guarantee (a bad statement never damages a
   sibling/header) worth ~10 days now, versus continuing to patch grammar
   special cases as the corpus demands?
2. Should `tree-sitter-yang` remain the canonical grammar for the native
   `.scm` consumers even after `yrepo` stops using it? (Probably yes — that
   keeps Zed/Neovim/CLI on one maintained grammar, with `yrepo` divergence
   risk only on tolerance edges.)
3. Do we ever want a real YANG *schema* dependency (libyang-level semantics)?
   If yes, Option E deserves a deeper spike before committing to B.
4. Should the new parser live in `yrepo` (replacing `syntax.rs`) or in a new
   `yang-parser` crate so `tree-sitter-yang` could later be *built on top of*
   it? (Crate boundary affects the maintenance split between the two repos.)

## 10. Appendix — conventions the replacement must copy (checklist)

- `Statement.range` = whole statement node; may overshoot the terminator over
  trailing whitespace (`keyword`/`arg`/`end` are exact).
- `StatementEnd`: `Semicolon{..}` / `Braces{open,close}`; `None` when the
  terminator was not recovered.
- `Argument.logical`: dequoted, `+`-joined text; `name()` trims; `path()`
  strips quotes/whitespace.
- `Token`: quoted strings are one `String` token, quotes included, never
  split; emit `+` as `Operator`; fragment augmentation folded into the lexer.
- `text_light`: drop `description`/`reference`/`organization`/`contact` from
  both tree and token stream; skip fragment augmentation.
- Errors: per-site `(range, message)`, `unexpected` vs `missing`; the
  `<`-first-byte file yields exactly one `not-a-yang-document` warning.
- Duplicate-module dedupe prefers a parse-clean copy: `parse_errors.is_empty()`
  must keep meaning "clean".
