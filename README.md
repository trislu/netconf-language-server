# NETCONF Language Server

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

> A [language server](https://microsoft.github.io/language-server-protocol/) for
reading and writing **NETCONF / YANG** files.

## Why

<p align="left">
<b>🎯 Semantic Oriented</b> — 🧠 read with insight · ✍️ write with ease<br>
<b>🦀 Native Rust</b> — 0️⃣ zero runtime · 🚀 just launch<br>
<b>⚡ Blazing Fast</b> — 🚅 all 3.6 GB of <a href="https://github.com/YangModels/yang">YangModels/yang</a>  parsed in ~30s · 💾 2.1 GB peak RAM
</p>

## Features

§1. From [vscode](clients/vscode), the **read** (diagnostics & hover)
and **write** (completion):

<p align="left">
  <img src="clients/vscode/resources/images/netconf-vscode-yang-20260905.png" alt="YANG diagnostics & hover" width="720">
</p>

<p align="left">
  <img src="clients/vscode/resources/images/netconf-vscode-xml-20260905.png" alt="XML (NETCONF) diagnostics & hover" width="720">
</p>

<p align="left">
  <img src="clients/vscode/resources/images/netconf-vscode-json-20260905.png" alt="RFC 7951 JSON diagnostics & hover" width="720">
</p>

<p align="left">
  <img src="clients/vscode/resources/images/netconf-vscode-xcomp-20260905.png" alt="XML completion" width="720">
</p>

<p align="left">
  <img src="clients/vscode/resources/images/netconf-vscode-jcomp-20260905.png" alt="JSON completion" width="720">
</p>

§2. From [zed](clients/zed), same **read** (diagnostics & hover)
and **write** (completion):

<p align="left">
  <img src="assets/images/netconf-zed-yang-20260906.png" alt="YANG diagnostics & hover (Zed)" width="720">
</p>

<p align="left">
  <img src="assets/images/netconf-zed-xml-20260906.png" alt="XML (NETCONF) diagnostics & hover (Zed)" width="720">
</p>

<p align="left">
  <img src="assets/images/netconf-zed-json-20260906.png" alt="RFC 7951 JSON diagnostics & hover (Zed)" width="720">
</p>

<p align="left">
  <img src="assets/images/netconf-zed-xcomp-20260906.png" alt="XML completion (Zed)" width="720">
</p>

<p align="left">
  <img src="assets/images/netconf-zed-jcomp-20260906.png" alt="JSON completion (Zed)" width="720">
</p>

> For more details, see the [LSP features guide](docs/features.md) in the repo.

## Repository layout

- [`src/`](src) — the language server binary (`netconf-language-server`).
- [`clients/vscode`](clients/vscode) — *vscode* extension.
- [`clients/zed`](clients/zed) — *zed* extension.
- [`examples/`](examples) — Sample *yang*/*xml*/*json*
  for manual testing.
- [`docs/architecture.md`](docs/architecture.md) — the design document and decision record.

## Contributing

Issues and PRs are raised in the same place — pick the repo by what the problem
is about:

- **[tree-sitter-yang](https://github.com/trislu/tree-sitter-yang)** — YANG doesn't *parse* (grammar/syntax gaps).
- **[yrepo](https://github.com/trislu/yrepo)** — YANG *parses* but resolves or compiles wrong (semantics, diagnostics, library API).
- **[this repo](https://github.com/trislu/netconf-language-server)** — editor/extension behavior on top (features, instance docs, vscode/zed clients).

Rule of thumb: parse → `tree-sitter-yang`, resolve → `yrepo`, editor → here.
Not sure? Open it here.

## License

This project is licensed under the [MIT License](LICENSE).
