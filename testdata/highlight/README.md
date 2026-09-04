# testdata/highlight — vendored YANG corpus for the highlight guard

A small, self-contained, parse-clean set of YANG modules used by two unit tests in
`src/semantic_token.rs`:

- `highlight_known_shapes_are_covered` — asserts the shapes we explicitly fixed stay
  highlighted (negative numbers/decimals, `deviate` verbs, quoted augment targets,
  structural keyword/type/variable coloring).
- `highlight_coverage_matches_baseline` — asserts the corpus still produces **exactly**
  `baseline.json` (uncovered word-token count per statement bucket). It is the regression
  net for everything else: any growth, new family, or (after a fix) shrinkage fails until the
  baseline is re-blessed.

## Fixtures and what each reproduces

| file | covers |
| --- | --- |
| `numbers.yang` | negative int/decimal `default` & `value`, `fraction-digits` (colored) |
| `deviations.yang` | `deviation` / `deviate` add·replace·delete·not-supported (colored) |
| `refs.yang` | `key`/`unique` members, `if-feature`, bare `units`, quoted vs **unquoted** augment |
| `keywords.yang` | `status`, `ordered-by`, `range`/`length` `min`·`max` (value keywords) |
| `quoted.yang` | quoted `config "false"` / `mandatory "true"` vs unquoted booleans |
| `dates.yang` | `revision` dates |
| `vendor-unknown.yang` | vendor extension (`vendor:`) content — accepted-by-design |

Each module is standalone (no imports) and compiles to **0 diagnostics**.

## Re-blessing after an intentional change

```bash
cargo test -- --ignored bless_highlight_baseline   # rewrites baseline.json
```

Run `bless` after fixing a highlight family (e.g. coloring keywords inside composite args)
or adding/removing fixtures. Never edit `baseline.json` by hand.
