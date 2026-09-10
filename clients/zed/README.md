# NETCONF Language Support

<p align="left">
  <a href="https://crates.io/crates/netconf-language-server"><img src="https://img.shields.io/crates/v/netconf-language-server.svg?style=for-the-badge&label=crates.io" alt="Version" /></a>
  <img src="https://img.shields.io/crates/l/netconf-language-server.svg?style=for-the-badge&label=LICENSE" alt="Version" />
</p>

> Write NETCONF/YANG files with the [NETCONF Language Server](../../README.md).

## Why

<p align="left">
<b>🎯 Semantic Oriented</b> — 🧠 read with insight · ✍️ write with ease<br>
<b>🦀 Native Rust</b> — 0️⃣ zero runtime · 🚀 just launch<br>
<b>⚡ Blazing Fast</b> — 🚅 165k-file <a href="https://github.com/YangModels/yang">YangModels/yang</a> workspace ready in ~0.3 s (no whole-tree parse) · 📊 <a href="../../docs/benchmarks.md">benchmarks</a>
</p>

## Features

**Read** — diagnostics & hover:

<p align="left">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/assets/images/netconf-zed-yang-20260906.png" alt="YANG diagnostics & hover" width="720">
</p>

<p align="left">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/assets/images/netconf-zed-xml-20260906.png" alt="XML (NETCONF) diagnostics & hover" width="720">
</p>

<p align="left">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/assets/images/netconf-zed-json-20260906.png" alt="RFC 7951 JSON diagnostics & hover" width="720">
</p>

**Write** — completion:

<p align="left">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/assets/images/netconf-zed-xcomp-20260906.png" alt="XML completion" width="720">
</p>

<p align="left">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/assets/images/netconf-zed-jcomp-20260906.png" alt="JSON completion" width="720">
</p>

> For more details, see the [LSP features guide](../docs/features.md) in the repo.

## Installation

1. Via cargo (_recommended_): `cargo install netconf-language-server`
2. Build from source: <https://github.com/trislu/netconf-language-server>
3. The extension can auto-download a prebuilt binary from the project's
   [GitHub releases](https://github.com/trislu/netconf-language-server/releases/).

## Settings

Language server settings live under `lsp."netconf-language-server"`. The only
server setting today is the YANG formatter indentation width (`indentSize`,
default `4`):

```json
{
  "lsp": {
    "netconf-language-server": {
      "binary": {
        "path": "/path/to/netconf-language-server"
      },
      "settings": {
        "indentSize": 4
      }
    }
  }
}
```

`binary.path` is optional — when omitted the extension looks the server up on
`$PATH` and then tries GitHub releases.

### Semantic highlighting

Semantic tokens are off by default in Zed and must be enabled per language.
YANG is colored by the tree-sitter grammar either way; turning semantic tokens
on adds the server's statement-aware coloring:

```json
{
  "languages": {
    "YANG": {
      "semantic_tokens": "combined"
    }
  }
}
```

`"combined"` layers the server's tokens over the tree-sitter highlights;
`"full"` uses only the server's tokens.

## References

- [YANG](https://www.rfc-editor.org/info/rfc6020)
- [NETCONF](https://www.rfc-editor.org/info/rfc6241)
- [YANG 1.1](https://www.rfc-editor.org/info/rfc7950)
- [YANG JSON Encoding](https://www.rfc-editor.org/info/rfc7951)
- [YANG Library](https://www.rfc-editor.org/info/rfc8525/)
