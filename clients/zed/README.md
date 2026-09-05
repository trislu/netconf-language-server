# NETCONF (Zed extension)

_This is a Zed extension for reading and writing YANG/NETCONF/RESTCONF files._

The extension registers the `netconf-language-server` for three languages:

- **YANG** — `*.yang` modules (authoring language provided by this extension).
- **XML** — attach to the **XML** extension's language. Install the
  marketplace extension "XML" so `.xml` files get a language (and its
  highlighting); the server content-sniffs each `.xml` buffer and stays
  dormant unless it is a NETCONF envelope/`<config>` payload.
- **JSON** — Zed's built-in JSON language, same dormant behavior for
  RFC 7951 instance documents.

## Install `netconf-language-server`

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

## Features

Authoring **YANG modules** (semantic highlighting, folding, formatting,
goto/hover, completion, pull diagnostics) plus **NETCONF instance documents** —
XML envelopes/`<config>` payloads and RFC 7951 JSON — with content-sniffed
recognition (unmatched files stay dormant), diagnostics on elements/members,
leaf _value_ validation, and RFC 7951/XML completion.

See the [detailed LSP features guide](../docs/features.md) in the repo.

## References

- [YANG](https://www.rfc-editor.org/info/rfc6020)
- [NETCONF](https://www.rfc-editor.org/info/rfc6241)
- [YANG 1.1](https://www.rfc-editor.org/info/rfc7950)
- [YANG JSON Encoding](https://www.rfc-editor.org/info/rfc7951)
- [YANG Library](https://www.rfc-editor.org/info/rfc8525/)
