# netconf-language-server (VS Code)

VS Code extension for writing NETCONF **YANG** modules with the
[netconf-language-server](../../README.md) language server.

## Features

- semantic highlighting, statement-level folding, document formatting
- go-to-definition, hover, completion
- pull-based diagnostics (errors/warnings from `yrepo`)

## Configuration

- `netconf.indentSize` (default `4`): spaces per indentation level when
  formatting.

## Development

The server binary is resolved from:

1. `__DEBUG_LSP_SERVER` env var (used by `.vscode/launch.json`), or
2. the bundled per-platform binary under `resources/bin/`
   (`netconf-language-server-linux`, `netconf-language-server-darwin`, `netconf-language-server-win32.exe`).

```bash
cargo build                                        # build the server
npm install                                        # client deps
npm run compile                                    # tsc + eslint + esbuild
npm run build-server-and-client                    # cargo build + npm compile
```

Press F5 from the repo root to launch the Extension Development Host against
`target/debug/netconf-language-server`.
