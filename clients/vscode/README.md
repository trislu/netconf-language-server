# netconf-language-server (VS Code)

VS Code extension for writing NETCONF **YANG** modules with the
[netconf-language-server](../../README.md) language server.

## Features

Authoring **YANG modules** (semantic highlighting, folding, formatting,
goto/hover, completion, pull diagnostics) plus **NETCONF instance documents** —
XML envelopes/`<config>` payloads and RFC 7951 JSON — with content-sniffed
recognition (unmatched files stay dormant), diagnostics on elements/members,
leaf *value* validation, and RFC 7951/XML completion. The editor also offers
`NETCONF: Insert …` commands for `hello` / `get-config` / `edit-config` /
`<config>` payloads.

See the [detailed LSP features guide](../docs/features.md) in the repo.

### Demo

**Read — diagnostics & hover** over **YANG**, **XML (NETCONF)**, and **RFC 7951
JSON**:

<p align="center">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/clients/vscode/resources/images/netconf-vscode-yang-20260905.png" alt="YANG diagnostics & hover" width="720">
</p>

<p align="center">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/clients/vscode/resources/images/netconf-vscode-xml-20260905.png" alt="XML (NETCONF) diagnostics & hover" width="720">
</p>

<p align="center">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/clients/vscode/resources/images/netconf-vscode-json-20260905.png" alt="RFC 7951 JSON diagnostics & hover" width="720">
</p>

**Write — completion** on XML elements and JSON members:

<p align="center">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/clients/vscode/resources/images/netconf-vscode-xcomp-20260905.png" alt="XML completion" width="720">
</p>

<p align="center">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/clients/vscode/resources/images/netconf-vscode-jcomp-20260905.png" alt="JSON completion" width="720">
</p>

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
