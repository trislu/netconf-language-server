# Semantic tokens (YANG highlighting)

Part of the YANG authoring family. Structure-aware coloring for `*.yang`
modules: keywords, arguments, names, types, strings, numbers, booleans and
comments are highlighted by what they are, not just as fixed keywords.

## For users

- **How to use it / when it runs**: nothing to trigger — coloring is applied
  automatically to every open or edited `*.yang` module, following the
  module's parsed structure (a name is colored by what it is: a definition, a
  reference, a type, …). Pure punctuation and whitespace stay uncolored by
  design.
- **Can it be customized?**: yes.
  - *Recolor* the existing classes with a theme or
    [`editor.semanticTokenColorCustomizations`](https://code.visualstudio.com/api/language-extensions/semantic-highlight-guide#customizing-semantic-highlighting)
    (client-side, no server change), in VS Code and in Zed.
  - *Change what is classified as what* with `netconf.semantic` (VS Code): a
    per-role object. Each member is one construct family (role) and its value
    picks a semantic token **type** from any standard `SemanticTokenType` plus
    optional **modifiers** (multi-select) — so any classification the
    [semantic highlight guide](https://code.visualstudio.com/api/language-extensions/semantic-highlight-guide)
    supports can be composed. Example:

    ```jsonc
    "netconf.semantic": {
      "enumAndBitNames": { "token": "enumMember" },
      "dataNodeName": { "token": "property" },
      "units": { "token": "variable", "modifiers": ["readonly"] }
    }
    ```

    Roles: `moduleName`, `typeRef`, `deviateVerb`, `dateString`, `rangeLength`,
    `patternArg`, `keyUniqueAugment`, `dataNodeName`, `definitionName`,
    `enumAndBitNames`, `numberArgument`, `referenceWord`, `vendorExtension`,
    `units`, `comment`, `stringLiteral`, `numberLiteral`, `boolean`,
    `valueKeyword`, `operator`.

  Built-in highlights with no `netconf.semantic` entry needed:
  - `pattern` arguments (regex strings) use the standard **`regexp`** type;
  - any declaration with a `status deprecated;` child marks its **whole
    subtree** with the standard **`deprecated`** modifier — including the
    `status deprecated;` marker's own words.
  Deprecated nodes only render **struck through** when a rule styles the
  `*.deprecated` semantic token with `strikethrough`. That rule cannot be
  shipped as a language-scoped extension default (VS Code reads
  `editor.semanticTokenColorCustomizations` at the theme layer, without a
  language override), so add it in your settings to opt in. The selector below
  is explicitly scoped to YANG (`:yang`) so it never affects other languages:

  ```jsonc
  "editor.semanticTokenColorCustomizations": {
    "rules": { "*.deprecated:yang": { "strikethrough": true } }
  }
  ```

  Changes apply on the next highlight with no server restart (the server
  announces the full standard token type + modifier legend). See
  [highlight-customization.md](../highlight-customization.md) for the design.
- **How to turn it off?**
  - VS Code: disable semantic highlighting for `[yang]`
    ([`editor.semanticHighlighting.enabled`](https://code.visualstudio.com/api/language-extensions/semantic-highlight-guide#enablement-of-semantic-highlighting)).
  - Zed: set [`languages.YANG.semantic_tokens`](https://zed.dev/docs/semantic-tokens)
    to `off`.
- **Limitations**
  - Only the construct families listed above are configurable (no user-authored
    matcher rules); `range`/`length` stay whole strings (their boundaries
    aren't lexed).
  - YANG-only: `.xml`/`.json` instance files keep the editor's own coloring.
  - A single colored token may span lines (multi-line strings/comments); VS
    Code renders it line by line.

## For developers

- **Implementation**: `src/semantic_token.rs` — two passes + legend/encoding,
  with classification resolved per *role* from the `netconf.semantic` config
  (`Role` / `Style::from_settings`, full standard legend `Class::ALL` /
  `Modifier::ALL`);
  `src/config.rs` carries the settings and `src/server.rs`
  `semantic_tokens_full` passes them in. Token stream comes from
  `yrepo::Repository::tokens` (see `yrepo` `src/syntax.rs` `collect_tokens` +
  fragment augmentation for concatenated quoted arguments).
- **Coupling**: relies on the `Statement` tree + `yrepo` token stream
  ([`../architecture.md`](../architecture.md) §8.3 / D4/D15).
- **Tests**: unit tests in `src/semantic_token.rs`, incl. a corpus coverage
  report generator (`generate_highlight_report`).
