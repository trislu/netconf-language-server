# NETCONF Language Server

[![Rust CI](https://github.com/trislu/netconf-language-server/actions/workflows/rust-ci.yml/badge.svg)](https://github.com/trislu/netconf-language-server/actions/workflows/rust-ci.yml)
[![Latest Version](https://img.shields.io/crates/v/netconf-language-server.svg)](https://crates.io/crates/netconf-language-server)
[![License](https://img.shields.io/crates/l/netconf-language-server.svg)](LICENSE)

A [language server](https://microsoft.github.io/language-server-protocol/) for
reading and writing **NETCONF / YANG** text documents. Which is:

<p align="left">
<b>🎯 Semantic Oriented</b> — 🧠 boost reading · 🔧 enhance writing<br>
<b>🦀 Native Rust</b> — 0️⃣ zero runtime · 🚀 just launch<br>
<b>⚡ Blazing Fast</b> — ⚙️ compile <b>2143</b> YANG files in <b>0.54&nbsp;s</b><br>
<sub>pyang ≈ <b>55.96&nbsp;s</b> @ 16 procs &nbsp;·&nbsp; 705 <a href="https://github.com/YangModels/yang/tree/main/standard">standard</a> + 1438 <a href="https://github.com/YangModels/yang/tree/main/experimental">experimental</a></sub>
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

## License

This project is licensed under the [MIT License](LICENSE).
