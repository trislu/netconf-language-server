# NETCONF Language Support

<p align="left">
  <!-- marketplace-readme:remove-start -->
  <a href="https://marketplace.visualstudio.com/items?itemName=k19.netconf"><img src="https://img.shields.io/badge/VS%20Code%20Marketplace-Install-007ACC?logo=visualstudiocode&logoColor=white&style=for-the-badge" alt="Install from VS Code Marketplace"></a>
  <!-- marketplace-readme:remove-end -->
  <img src="https://img.shields.io/github/v/release/trislu/netconf-language-server?style=for-the-badge&label=Version" alt="Version" />
  <img src="https://vsmarketplacebadges.dev/installs-short/k19.netconf.svg?style=for-the-badge" alt="Installs" />
</p>

VS Code extension for writing NETCONF **YANG** modules with the
[NETCONF Language Server](../../README.md).

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
