# Extension commands & settings

The user-facing surface of the VS Code extension.

## For users

**Commands** — run from the command palette:

| Command | What it does |
| --- | --- |
| `NETCONF: Insert get-config RPC` | insert an `<rpc><get-config>` skeleton at the caret |
| `NETCONF: Insert edit-config RPC` | insert an `<rpc><edit-config>` skeleton at the caret |
| `NETCONF: Insert hello` | insert a `<hello>` skeleton at the caret |
| `NETCONF: Insert config payload` | insert a `<config>` payload skeleton at the caret |
| `NETCONF: Restart Language Server` | restart the server (re-runs the workspace scan) |

**Settings** — under the `netconf.*` scope in VS Code configuration; changes
apply to the running server:

| Setting | Default | Meaning |
| --- | --- | --- |
| `netconf.indentSize` | `4` | formatter indentation width (2–8) |

`netconf.indentSize` is today the only setting; it drives the formatter (see
[02-yang-fold-and-format.md](02-yang-fold-and-format.md)).

## For developers

- **Implementation**: commands registered in
  `clients/vscode/src/extension.ts`; templates handled server-side
  (`netconf/insertTemplate`, `src/template.rs`); settings forwarded to the
  server via `workspace/didChangeConfiguration`.
- **Notes**: the extension currently requires a single workspace folder —
  multi-root support is planned (see
  [`../multi-root-workspaces.md`](../multi-root-workspaces.md)).
