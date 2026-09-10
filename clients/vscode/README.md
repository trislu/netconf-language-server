# NETCONF Language Support

<p align="left">
  <a href="https://crates.io/crates/netconf-language-server"><img src="https://img.shields.io/crates/v/netconf-language-server.svg?style=for-the-badge&label=crates.io" alt="Version" /></a>
  <img src="https://img.shields.io/crates/l/netconf-language-server.svg?style=for-the-badge&label=LICENSE" alt="Version" />
  <br>
  <!-- marketplace-readme:remove-start -->
  <a href="https://marketplace.visualstudio.com/items?itemName=k19.netconf"><img src="https://img.shields.io/badge/VSCode%20Marketplace-Install-007ACC?logo=visualstudiocode&logoColor=white&style=for-the-badge" alt="Install from VS Code Marketplace"></a>
  <img src="https://vsmarketplacebadges.dev/installs-short/k19.netconf.svg?style=for-the-badge" alt="Installs" />
  <br>
  <a href="https://open-vsx.org/extension/k19/netconf"><img src="https://img.shields.io/badge/Open%20VSX%20Registry-Install-6A4FB6?style=for-the-badge" alt="Install from Open VSX"></a>
  <a href="https://open-vsx.org/extension/k19/netconf"><img src="https://img.shields.io/badge/dynamic/json?url=https%3A%2F%2Fopen-vsx.org%2Fapi%2Fk19%2Fnetconf&query=downloadCount&label=Downloads&color=green&style=for-the-badge" alt="Open VSX downloads" /></a>
  <br>
  <!-- marketplace-readme:remove-end -->
</p>

> Write NETCONF/YANG files with the [NETCONF Language Server](../../README.md) — available for Visual Studio Code and Open VSX–compatible editors (VSCodium, Cursor, …).

## Why

<p align="left">
<b>🎯 Semantic Oriented</b> — 🧠 read with insight · ✍️ write with ease<br>
<b>🦀 Native Rust</b> — 0️⃣ zero runtime · 🚀 just launch<br>
<b>⚡ Blazing Fast</b> — 🚅 165k-file <a href="https://github.com/YangModels/yang">YangModels/yang</a> &lt;1 s startup, ~25 s lazy find-all-references (16-thread host) · 📊 <a href="../../docs/benchmarks.md">benchmarks</a>
</p>

## Features

**Read** — diagnostics & hover:

<p align="left">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/clients/vscode/resources/images/netconf-vscode-yang-20260905.png" alt="YANG diagnostics & hover" width="720">
</p>

<p align="left">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/clients/vscode/resources/images/netconf-vscode-xml-20260905.png" alt="XML (NETCONF) diagnostics & hover" width="720">
</p>

<p align="left">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/clients/vscode/resources/images/netconf-vscode-json-20260905.png" alt="RFC 7951 JSON diagnostics & hover" width="720">
</p>

**Write** — completion:

<p align="left">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/clients/vscode/resources/images/netconf-vscode-xcomp-20260905.png" alt="XML completion" width="720">
</p>

<p align="left">
  <img src="https://raw.githubusercontent.com/trislu/netconf-language-server/master/clients/vscode/resources/images/netconf-vscode-jcomp-20260905.png" alt="JSON completion" width="720">
</p>

> For more details, see the [LSP features guide](../../docs/features.md) in the repo.

## Configuration

- `netconf.indentSize` (default `2`): spaces per indentation level when
  formatting.

- `netconf.semantic` (default `{}`): per-role YANG semantic-highlight
  classification. Each member is one construct family (`moduleName`, `typeRef`,
  `dataNodeName`, `definitionName`, `enumAndBitNames`, `units`, `rangeLength`,
  `patternArg`, `vendorExtension`, … — 20 roles total) and its value is an
  object picking the semantic token **type** (`"enumMember"`, `"property"`, … —
  any standard type) and the **modifiers** (multi-select: `"readonly"`,
  `"declaration"`, …).
  A role left out keeps its built-in classification (notably `pattern` args are
  `regexp`, and `status deprecated;` declarations get the `deprecated` modifier
  on their whole subtree). The server announces the full standard legend, so
  changes apply live — no server restart. Example:

  ```jsonc
  "netconf.semantic": {
    "enumAndBitNames": { "token": "enumMember" },
    "units": { "token": "variable", "modifiers": ["readonly"] }
  }
  ```

To render `status deprecated;` declarations **struck through**, add the rule
to your settings (VS Code reads `editor.semanticTokenColorCustomizations` at
the theme layer, so this is opt-in rather than an extension default). The
selector is explicitly scoped to YANG so it never affects other languages:

```jsonc
"editor.semanticTokenColorCustomizations": {
  "rules": { "*.deprecated:yang": { "strikethrough": true } }
}
```

## References

- [YANG](https://www.rfc-editor.org/info/rfc6020)
- [NETCONF](https://www.rfc-editor.org/info/rfc6241)
- [YANG 1.1](https://www.rfc-editor.org/info/rfc7950)
- [YANG JSON Encoding](https://www.rfc-editor.org/info/rfc7951)
- [YANG Library](https://www.rfc-editor.org/info/rfc8525/)
