# netconf-language-server

A [language server](https://microsoft.github.io/language-server-protocol/) for
authoring **NETCONF / YANG** modules (`*.yang`), implemented in Rust, with a
VS Code extension.

## Repository layout

- [`src/`](src) — the language server binary (`netconf-language-server`).
  Semantic engine: [`yrepo`](https://github.com/trislu/yrepo) (YANG
  parse/resolve/query). Grammar: `tree-sitter-yang`.
- [`clients/vscode`](clients/vscode) — VS Code extension (language id `yang`,
  settings section `netconf`).
- [`examples/`](examples) — sample YANG modules for manual testing.
- [`docs/architecture.md`](docs/architecture.md) — the design document
  (decisions D1–D17).

## LSP features

| Feature | Status |
| --- | --- |
| Semantic tokens (highlight) | ✅ |
| Statement-level folding | ✅ |
| Document formatting (full-doc) | ✅ |
| Diagnostics (pull, incl. import cycles / conflict prefix) | ✅ |
| Go-to-definition (`LocationLink`) | ✅ |
| Hover (type chains / identity ancestry / prefix bindings) | ✅ |
| Completion (`type` / identity `base`) | ✅ |

## Building

```bash
cargo build          # server -> target/debug/netconf-language-server
cargo clippy         # lint
```

VS Code client:

```bash
cd clients/vscode
npm install
npm run compile      # tsc + eslint + esbuild -> dist/extension.js
```

To debug the extension end-to-end (F5 from this repo), `.vscode/launch.json`
runs the Extension Development Host against `target/debug/netconf-language-server` (see
`__DEBUG_LSP_SERVER`) with `examples/` openable as the test workspace.

## Settings

- `netconf.indentSize` (default `4`): spaces per indentation level used by the
  formatter.

## License

MIT — see [`LICENSE`](LICENSE).
