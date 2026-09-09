# Highlight customization — design record

> Status: **implemented** (unreleased; shipped behind the new `netconf.semantic`
> settings). This page is the design record for letting users customize how
> YANG is semantically highlighted (classification and theming) without forking
> the server. The original plan sections below describe the *design*; the
> [Implementation](#implementation-status) section records what actually
> landed.

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

## Default state (built-in classification)

- **Legend** (`src/semantic_token.rs`): the **full standard** LSP
  `SemanticTokenType` set (23 types, `namespace`…`decorator`) and the full
  standard `SemanticTokenModifier` set (10). Every standard type/modifier is
  announced, so a role can pick any of them without a client restart; types no
  role maps to simply produce no tokens.
- **Classification** is resolved per *role* from `netconf.semantic` at
  `semantic_tokens_full` time (no recompile). With no configuration it is
  byte-for-byte the historic classification:
  - structural pass: `role_of_kind(StatementKind)` → atomic role per kind;
  - `whole_arg_role` → whole-argument role for numbers/unquoted words/`units`;
  - lexical pass: `yrepo::TokenKind` → role (strings, numbers, booleans,
    `+`, keywords) inside composite arguments; quoted fragments come from
    `yrepo`'s augmented token stream.
- **Config** (`src/config.rs`) now carries `netconf.indentSize` and
  `netconf.semantic` (a per-role `token` + `modifiers` struct), forwarded by
  the client like any other `netconf.*` setting and logged at startup.

## LSP constraint (design anchor)

The **legend is announced once** in the server capability
(`semanticTokensProvider.legend`). Changing it means the server must
re-announce capabilities, which VS Code only honors after a client restart
(via the `NETCONF: Restart Language Server` command). The plan works around
this:

- **Legend is static per session.** Configuration may change *which token type
  a construct maps to*, but not the set of announced types/modifiers — so the
  server announces the **full standard** `SemanticTokenType` /
  `SemanticTokenModifier` sets up front and configuration picks among them,
  needing no restart.
- Pure **color** changes need no server involvement at all (VS Code
  `editor.semanticTokenColorCustomizations`).

## Design

Two complementary layers:

- **Layer 1 — theming (client-side, no server change).** Document a recommended
  `editor.semanticTokenColorCustomizations` block for the announced classes,
  plus per-type theming guidance. Users wanting only a different look
  stop here. (Semantic token *styles* are read by VS Code at the theme layer
  without a language override, so they are opt-in via user settings/theme, not
  a language-scoped extension default.)
- **Layer 2 — classification config (server-side).** `netconf.semantic` is a
  **per-role struct**: every supported role is a member, and its value picks
  the token **type** (any standard `SemanticTokenType`) plus optional
  **modifiers** (any standard `SemanticTokenModifier`, multi-select in the VS
  Code settings UI). Unconfigured roles keep their built-in classification.

```jsonc
// VS Code settings (what actually shipped)
"netconf.semantic": {
  "enumAndBitNames": { "token": "enumMember" },   // enum/bit names → enumMember
  "units": { "token": "variable", "modifiers": ["readonly"] },
  "dataNodeName": { "token": "property" }
}
```

Roles (the config object's members): `moduleName`, `typeRef`, `deviateVerb`,
`dateString`, `rangeLength`, `patternArg`, `keyUniqueAugment`, `dataNodeName`,
`definitionName`, `enumAndBitNames`, `numberArgument`, `referenceWord`,
`vendorExtension`, `units`, `comment`, `stringLiteral`, `numberLiteral`,
`boolean`, `valueKeyword`, `operator`. Token values are any standard
`SemanticTokenType` name; modifiers any standard `SemanticTokenModifier` name.
An unknown role key, or a member whose token type/modifier is unknown, is
ignored (the role keeps its built-in classification).

Two built-ins need no config: `pattern` arguments are `regexp` (their regex
string maps to the standard `regexp` type), and any declaration with a
`status deprecated;` child has the standard **`deprecated`** modifier OR'd into
its whole subtree — including the `status deprecated;` marker's own words
(`obsolete` has no standard modifier and stays unmarked).

Server maps config → classification at `semantic_tokens_full` time (no
recompile): each role resolves to `(class, modifier-bits)`
(`Style::from_settings` in `src/semantic_token.rs`); unconfigured roles
preserve current output exactly.

> **Scope note.** There are **no presets** — the config *is* the per-role struct
> (the earlier `default`/`structured`/`minimal` idea was dropped so users
> compose exactly the combination they want). Lexing concatenation `+` /
> `range`/`length` *boundaries* as separate tokens remains out of scope:
> `range`/`length` are whole strings today because the grammar hides
> signed-number digits inside the argument token. Configuration stays per-role
> (no user-authored matcher engine).

## Implementation status

Landing commit implements Phases 2–5 of the plan below; Phase 1 is **guidance
only** — the recommended `editor.semanticTokenColorCustomizations` block is
documented, not shipped, because VS Code reads that setting at the theme layer
(no language override), so a `[yang]`-scoped extension default would be
silently ignored.

| Phase | What landed | Where |
| ----- | ----------- | ----- |
| 1 — theming layer | color-recipe guidance (no server change) | [`features/01-yang-highlight.md`](features/01-yang-highlight.md) |
| 2 — full standard legend + stable default | announce the full `SemanticTokenType` / `SemanticTokenModifier` sets; unconfigured roles keep built-in output (baseline coverage test green) | `src/semantic_token.rs` `Class::ALL`/`Modifier::ALL` / `capability` |
| 3 — config plumbing | `netconf.semantic` (per-role `token`+`modifiers` struct) in `Config`, forwarded client-side, logged at startup | `src/config.rs`, `clients/vscode/package.json`, `src/server.rs` `semantic_tokens_full` |
| 4 — classification engine | per-*role* classification (`Style`); every role → any standard type + modifiers | `src/semantic_token.rs` (`Class`, `Modifier`, `Role`, `Style`) |
| 5 — tests & docs | per-role/modifier/capability tests; feature doc + README/CHANGELOG updates | this doc + `features/01-yang-highlight.md` |

## Original effort estimate (focused, one developer)

| Area | Change | Effort |
| ------ | -------- | -------- |
| Layer 1 (theming docs + sample) | recommended color rules, README notes | ~0.5 day |
| Full standard legend + stable default | announce the whole standard type/modifier set up front | ~0.5 day |
| Config plumbing | `netconf.semantic` per-role settings; forward like `indentSize` | ~0.5 day |
| Classification engine | parameterize the role tables (`role_of_kind`/`whole_arg_role`/lexical) by config (per-role) | ~1–1.5 days |
| Tests + docs | per-role classification tests; feature doc user view | ~1 day |

**Total ≈ 3–4 focused days.**

## Phased plan (as executed)

1. **Phase 1 — theming layer.** Document
   `editor.semanticTokenColorCustomizations`; add a recommended scheme +
   README section. *Checkpoint:* users recolor via VS Code settings with no
   server change. *(Done as guidance only — a bundled `[yang]` default cannot
   work: the setting is read unscoped at the theme layer.)*
2. **Phase 2 — full standard legend + stable default.** ✅ Announce the full
   standard `SemanticTokenType`/`SemanticTokenModifier` sets; unconfigured
   roles keep the historic classification.
   *Checkpoint:* all existing semantic-token tests pass unchanged — ✅.
3. **Phase 3 — config plumbing.** ✅ Add `netconf.semantic` to `Config`, forward
   it client-side like `indentSize`, log it at startup.
   *Checkpoint:* settings appear in server config log — ✅.
4. **Phase 4 — classification engine + per-role config.** ✅ Parameterize the
   three classification tables; every role is overridable to any standard token
   type with optional modifiers.
   *Checkpoint:* per-role tests; changes apply without a restart (legend is the
   full standard set) — ✅.
5. **Phase 5 — docs & polish.** ✅ Update
   [`features/01-yang-highlight.md`](features/01-yang-highlight.md) user view
   (usage + limitations), extension README, CHANGELOG; regression-run the
   corpus highlight coverage report — baseline re-run green.

## Risks

- **Legend immutability** — adding token types beyond the full standard set
  needs a restart; announcing the full standard set bounds this but is not free
  (some types may go unused by the built-in classification).
- **Scope creep** — full custom rule engines (user-authored matchers) are
  out of scope; configuration stays per-role (token type + modifiers) only.
- **Classification drift** — per-role behavior is pinned by golden tests so
  future highlight work doesn't silently change a role's built-in
  classification.

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
