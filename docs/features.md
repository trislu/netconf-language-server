# LSP Features

> **Overview & index.** What the language server does, how to read the
> per-feature docs, and where every supported feature is documented. Each
> feature lives in [`docs/features/`](features/README.md) as its own `.md` with
> two clearly separated views:
>
> - **For users** — what it does, when it kicks in, usage and limitations.
> - **For developers** — implementation files, coupling, and tests.
>
> Users can stop at the user view (or the
> [extension README](../clients/vscode/README.md)); only maintainers need the
> developer view, plus [`architecture.md`](architecture.md) (D1–D31 decision
> log), [`serving-large-trees.md`](serving-large-trees.md) (catalog + closure +
> reference-index model), and [`design-ideas.md`](design-ideas.md) (unstable
> ideas).

Two feature families are served by one process, routed per document:

- **YANG authoring** — `*.yang` modules: highlight, folding, formatting,
  diagnostics, goto/hover, find-references/rename, completion.
- **NETCONF instance documents** — `.xml` NETCONF envelopes/payloads and
  RFC 7951 JSON: detection, read (goto/hover/diagnostics), write (templates +
  completion), leaf value validation.

## Feature index

### YANG authoring

| Feature | Doc | User / developer |
| --- | --- | --- |
| Semantic tokens (highlight) | [01-yang-highlight.md](features/01-yang-highlight.md) | both |
| Folding & formatting | [02-yang-fold-and-format.md](features/02-yang-fold-and-format.md) | both |
| Diagnostics | [03-yang-diagnostics.md](features/03-yang-diagnostics.md) | both |
| Go-to-definition & hover | [04-yang-goto-and-hover.md](features/04-yang-goto-and-hover.md) | both |
| Find references & rename (whole-tree) | [05-yang-find-references-rename.md](features/05-yang-find-references-rename.md) | both |
| Completion | [06-yang-completion.md](features/06-yang-completion.md) | both |

### NETCONF instance documents

| Feature | Doc | User / developer |
| --- | --- | --- |
| Read: detection · goto · hover · diagnostics | [07-netconf-instances-read.md](features/07-netconf-instances-read.md) | both |
| Write: templates & completion | [08-netconf-write.md](features/08-netconf-write.md) | both |
| Leaf value validation | [09-value-validation.md](features/09-value-validation.md) | both |

### Extension surface

| Feature | Doc | User / developer |
| --- | --- | --- |
| Commands & settings | [10-commands-and-settings.md](features/10-commands-and-settings.md) | both |

## Status & cross-cutting limits

- Per-feature limitations are listed under each doc's **user** view.
- Not implemented (cross-cutting): `leafref` value *chasing* and fully-semantic
  `instance-identifier`; NETCONF envelope goto-modeling; quick-fixes /
  code actions / comment-out; automatic re-scan when files change on disk
  outside the editor; incremental repository compile; out-of-workspace include
  dirs; multiple workspace folders (planned — see
  [multi-root-workspaces.md](multi-root-workspaces.md)).
- `.yin` (XML) input is out of scope; YANG compile happens through `yrepo`
  (see [`architecture.md`](architecture.md) §14).

## Examples

Hand-test everything against the `examples/` folder:

- `example-demo.yang`, `example-ietf-interfaces.yang` — demo modules (the
  second augments the first);
- `example-netconf-config.xml` — a `<config>` payload demonstrating mapping,
  cross-module augment, goto/hover/diagnostics;
- `example-netconf-data.json` — the RFC 7951 mirror.
