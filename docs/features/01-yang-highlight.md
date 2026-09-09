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
- **Can it be customized?**: only partly. Which class a construct maps to is
  fixed in the server and not configurable yet, but you can recolor those
  classes in VS Code (theme or `editor.semanticTokenColorCustomizations`) and
  in Zed. Configurable presets are planned — see
  [highlight-customization.md](../highlight-customization.md).
- **How to turn it off?**
  - VS Code: disable semantic highlighting for `[yang]`
    ([`editor.semanticHighlighting.enabled`](https://code.visualstudio.com/api/language-extensions/semantic-highlight-guide#enablement-of-semantic-highlighting)).
  - Zed: set [`languages.YANG.semantic_tokens`](https://zed.dev/docs/semantic-tokens)
    to `off`.
- **Limitations**
  - YANG-only: `.xml`/`.json` instance files keep the editor's own coloring.
  - A single colored token may span lines (multi-line strings/comments); VS
    Code renders it line by line.

## For developers

- **Implementation**: `src/semantic_token.rs` (two passes + legend/encoding),
  `src/server.rs` `semantic_tokens_full`. Token stream comes from
  `yrepo::Repository::tokens` (see `yrepo` `src/syntax.rs` `collect_tokens` +
  fragment augmentation for concatenated quoted arguments).
- **Coupling**: relies on the `Statement` tree + `yrepo` token stream
  ([`../architecture.md`](../architecture.md) §8.3 / D4/D15).
- **Tests**: unit tests in `src/semantic_token.rs`, incl. a corpus coverage
  report generator (`generate_highlight_report`).
