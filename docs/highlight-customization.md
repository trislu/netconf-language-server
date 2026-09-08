# Highlight customization — plan

> Status: **proposal / not started**. Near-term actionable (not an aspirational
> `design-ideas.md` entry): plan for letting users customize how YANG is
> semantically highlighted (classification and theming) without forking the
> server.

## Motivation

Highlighting in the extension is implemented with LSP
[semantic tokens](https://code.visualstudio.com/api/language-extensions/semantic-highlight-guide),
but the classification is **fixed**: the server always emits the same eight
token classes plus one modifier. Users who want a different look — constants
emphasized, vendor `unknown` statements colored, `range`/`length` internals as
numbers — have no server-side lever. They *can* recolor the existing classes in
VS Code, but they cannot change *what is classified as what*.

## Relation to the `tree-sitter-yang` highlight query (`.scm`)

Coloring in this extension is driven **only** by the LSP semantic tokens the
server emits; it does **not** read `tree-sitter-yang`'s
`queries/highlights.scm`.

That query file is a *separate* classification surface for tree-sitter-native
consumers of the grammar — the Zed extension (`clients/zed`), Neovim via
nvim-treesitter, and the tree-sitter CLI (`tree-sitter highlight`) read it
directly from the grammar repo
(`../tree-sitter-yang/queries/highlights.scm`).

Consequences for this plan:

- Everything below customizes the **server / semantic-token surface** (VS Code).
- It does **not** touch the `.scm` query; a user who moves to a
  tree-sitter-native client would lose the custom classes until that surface
gets its own mechanism.
- Keeping the two surfaces in sync is therefore an optional, separate
  follow-up — out of scope here.

## Current state (fixed classification)

- **Legend** (`src/semantic_token.rs`): `Keyword`, `Namespace`, `Type`,
  `Variable`, `String`, `Number`, `Comment`, `Operator`; one modifier
  (`readonly`, used for constant `units` values).
- **Classification** is hard-coded:
  - structural pass: `arg_semantics(StatementKind)` → atomic class per kind;
  - `whole_arg` → whole-argument coloring for numbers/unquoted words/`units`;
  - lexical pass: `yrepo::TokenKind` → class (strings, numbers, booleans,
    `+`, keywords) inside composite arguments; quoted fragments come from
    `yrepo`'s augmented token stream.
- **Config** (`src/config.rs`) currently has only `netconf.indentSize`;
  `semantic_tokens_full` takes no config, so the legend/classification is
  identical for everyone.

## LSP constraint (design anchor)

The **legend is announced once** in the server capability
(`semanticTokensProvider.legend`). Changing it means the server must
re-announce capabilities, which VS Code only honors after a client restart
(via the `NETCONF: Restart Language Server` command). The plan works around
this:

- **Legend is static per session.** Presets/overrides may change *which class
  a construct maps to*, but not the set of announced types/modifiers — unless
  we ship a **superset legend** (announce more types/modifiers up front) and
  let configuration pick among them. Recommend the superset approach so
  presets need no restart.
- Pure **color** changes need no server involvement at all (VS Code
  `editor.semanticTokenColorCustomizations`).

## Design

Two complementary layers:

- **Layer 1 — theming (client-side, no server change).** Document and ship a
  recommended `editor.semanticTokenColorCustomizations` block for the announced
  classes, plus per-type theming guidance. Users wanting only a different look
  stop here.
- **Layer 2 — classification config (server-side).** New `netconf.semantic`
  settings that map constructs to the (superset) legend:

```jsonc
// VS Code settings
"netconf.semantic": {
  "preset": "default" | "structured" | "minimal",
  "types": {
    // promote an existing class to a more specific announced type
    "units": "variable" | "readonly",
    "rangeLength": "string" | "operator",
    "vendorExtension": "type" | "comment",
    "enumAndBitNames": "enumMember",   // superset-announced
  },
  "toggles": {
    "highlightKeywords": true,
    "highlightStrings": true
  }
}
```

Server maps config → classification at `semantic_tokens_full` time (no
recompile): classification tables (`arg_semantics`, `whole_arg`, lexical) pick
classes from the config + preset; a stable default preserves current output.

### Presets (examples, no restart required via superset legend)

- `default` — exactly today's classification.
- `structured` — emphasize `enum`/`bit` names (`enumMember`), mark
  constant `units` (`readonly`), color concatenation `+` and `range`/`length`
  boundaries as `operator`/`number`.
- `minimal` — drop noise: only keywords, strings, comments, data-node names.

## Effort estimate (focused, one developer)

| Area | Change | Effort |
| ------ | -------- | -------- |
| Layer 1 (theming docs + sample) | recommended color rules, README notes | ~0.5 day |
| Superset legend + stable default | announce extra types/modifiers up front | ~0.5 day |
| Config plumbing | `netconf.semantic` settings; forward like `indentSize` | ~0.5 day |
| Classification engine | parameterize `arg_semantics`/`whole_arg`/lexical by config + preset | ~1–1.5 days |
| Tests + docs | golden/per-doc classification tests per preset; feature doc user view | ~1 day |

**Total ≈ 3–4 focused days.**

## Phased plan

1. **Phase 1 — theming layer.** Document
   `editor.semanticTokenColorCustomizations`; add a recommended scheme +
   README section. *Checkpoint:* users recolor via VS Code settings with no
   server change.
2. **Phase 2 — superset legend + stable default.** Announce a superset legend;
   keep classification byte-for-byte identical under the `default` preset.
   *Checkpoint:* all existing semantic-token tests pass unchanged.
3. **Phase 3 — config plumbing.** Add `netconf.semantic` to `Config`, forward
   it client-side like `indentSize`, log it at startup.
   *Checkpoint:* settings appear in server config log.
4. **Phase 4 — classification engine + presets.** Parameterize the three
   classification tables; implement `structured`/`minimal`.
   *Checkpoint:* golden tests per preset; switching presets changes classes
   without a restart (legend is superset).
5. **Phase 5 — docs & polish.** Update
   [`features/01-yang-highlight.md`](features/01-yang-highlight.md) user view
   (usage + limitations), extension README, CHANGELOG; regression-run the
   corpus highlight coverage report.

## Risks

- **Legend immutability** — adding real token types beyond the superset needs a
  restart; the superset approach bounds this but not free (some types may go
  unused in `default`).
- **Scope creep** — full custom rule engines (user-authored matchers) are
  out of scope; configuration stays preset + per-construct override only.
- **Classification drift** — presets must be pinned by golden tests so future
  highlight work doesn't silently change a preset.

## References

- [`features/01-yang-highlight.md`](features/01-yang-highlight.md) — current
  highlight behavior (user/developer views).
- [`architecture.md`](architecture.md) §8.3 / D4/D15 — two-pass model and
  token stream.
- `src/config.rs`, `src/semantic_token.rs` — config + classification.
- yrepo `Repository::tokens` fragment augmentation (string/`+` tokens).
- `tree-sitter-yang` `queries/highlights.scm` — the grammar-native highlight
  query (Zed/Neovim/tree-sitter CLI), separate from this server's semantic
  tokens (see the ".scm" section above).
