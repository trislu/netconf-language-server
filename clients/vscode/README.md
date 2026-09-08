# NETCONF Language Support

<p align="left">
  <img src="https://img.shields.io/crates/v/netconf-language-server.svg?style=for-the-badge&label=crates.io" alt="Version" />
  <img src="https://img.shields.io/crates/l/netconf-language-server.svg?style=for-the-badge&label=LICENSE" alt="Version" />
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
<b>⚡ Blazing Fast</b> — 🚅 all 3.6 GB of <a href="https://github.com/YangModels/yang">YangModels/yang</a>  parsed in ~30s · 💾 2.1 GB peak RAM
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

> For more details, see the [LSP features guide](../docs/features.md) in the repo.

## Configuration

- `netconf.indentSize` (default `4`): spaces per indentation level when
  formatting.

## References

- [YANG](https://www.rfc-editor.org/info/rfc6020)
- [NETCONF](https://www.rfc-editor.org/info/rfc6241)
- [YANG 1.1](https://www.rfc-editor.org/info/rfc7950)
- [YANG JSON Encoding](https://www.rfc-editor.org/info/rfc7951)
- [YANG Library](https://www.rfc-editor.org/info/rfc8525/)
