# LSP Features (detailed)

> **User-facing behavior guide.** This explains what each LSP feature *does* and
> when it kicks in, with examples. Internal design lives in
> [`architecture.md`](architecture.md) (incl. the D1–D31 decision log); unstable
> design ideas are in [`design-ideas.md`](design-ideas.md).

Two feature families are served by one process, routed per document:

- **YANG authoring** — `*.yang` modules (semantic tokens, folding, formatting,
  diagnostics, goto/hover/completion).
- **NETCONF instance documents** — `.xml` NETCONF envelopes/payloads and RFC 7951
  JSON data (read/write/diagnostics/value validation).

## 1. YANG authoring

| Feature | What it does |
| --- | --- |
| Semantic tokens | Two-pass highlighting over the parsed statement tree (keywords, arguments, types, comments) driven by `yrepo`'s token stream. |
| Folding | Statement-level `{ … }` region folds. |
| Formatting | Full-document reformat from the statement tree (comments spliced back); **skipped when the document has syntax errors**. |
| Diagnostics | Pull-based (`textDocument/diagnostic`); refreshed when a module opens/closes. Source `yang`. |
| Go-to-definition | `LocationLink` from a reference to its definition, incl. cross-file targets. |
| Hover | Defining source snippet + node kind/type + identity ancestry + prefix binding. |
| Completion | `type` and identity `base` arguments: builtins, local + imported `prefix:name` typedefs, identity candidates. |

YANG diagnostics fall into a few areas (wire codes are `yrepo::DiagnosticCode`
kebab-case strings, plus one server-side code):

- **parse / syntax** — recovery diagnostics from the grammar.
- **module graph** — unresolved import/include/belongs-to, duplicate module,
  import & include cycles.
- **symbol resolution** — unresolved prefix, typedef, grouping, identity.
- **schema edges** — augment/deviation target not found.
- **list keys** — missing/invalid key, key-less config list.
- **conflict prefix** (`conflict_prefix`, LS-side) — duplicate local prefix
  declarations within a module.

## 2. Instance documents (XML / RFC 7951 JSON)

### 2.1 Detection & dormant behavior

An `.xml`/`.json` buffer is **content-sniffed** against the compiled YANG
modules (M0/D19). If its root/keys match a NETCONF intent (message envelope,
`<config>` payload wrapper, or a module data tree), the server attaches the
features below. A file that matches **nothing** stays **dormant** — no netconf
diagnostics, goto, hover, or completion — so ordinary XML/JSON editing is
untouched (VS Code's built-in providers keep doing tokens/folding/formatting).

### 2.2 Read — goto / hover / diagnostics

| | XML (M1) | JSON (M3) |
| --- | --- | --- |
| Target | elements | members/keys (`module:name`) |
| Goto | element → defining YANG node | member → defining YANG node |
| Hover | schema snippet + kind/type/keys | same |
| Diagnostics | see 2.3 | see 2.3 |

Both resolve through `choice`/`case` wrappers and across cross-module `augment`s
(D29/D30); `goto`/`hover` land on the node's `origin_module` definition.

### 2.3 Diagnostics (source `netconf`)

| Code | Meaning |
| --- | --- |
| `netconf_unknown_node` / `json_unknown_member` | element/key is not a child of the current schema node |
| `netconf_wrong_ns` / `json_wrong_module` | name exists but in the wrong namespace / module |
| `netconf_missing_node` / `json_missing_member` | present container/list is missing a mandatory node or a list `key` (M4) |
| `netconf_missing_choice` / `json_missing_choice` | present node has a `choice` with no instantiated case (M4) |
| `netconf_bad_value` / `json_bad_value` | leaf value fails scalar validation (M5, §3) |

XML suppresses the depth/value checks inside a `<filter>` subtree (partial
content is legal there). A fully dormant document reports nothing.

### 2.4 Write — templates & completion

- **NETCONF templates** (M2): `NETCONF: Insert …` commands insert `hello`,
  `get-config`, `edit-config`, or a `<config>` payload skeleton at the caret
  (via `workspace/executeCommand` → server applies a `WorkspaceEdit`).
- **Completion**: inside a mapped container/list (or `<config>` payload) the
  element names are offered (with `key` placeholders and auto-`xmlns` when the
  child's namespace differs); under `<rpc>` the built-in NETCONF ops + compiled
  module RPCs. JSON completion (M4) offers RFC 7951 member names at a fresh
  object slot — root members always `module:name`, nested members bare when they
  share the parent's module, `module:name` otherwise.

## 3. Leaf value validation (M5)

Leaf/leaf-list **values** are validated only when the type reduces to a scalar
at compile time (typedef chain → builtin, D31); **`union` is deliberately not
checked** (RFC 7950 §9.12 — a bare value can't be attributed to one member).

| Type | Checked | Notes |
| --- | --- | --- |
| string | length + pattern | both may accumulate along a typedef chain |
| int8…64 / uint8…64 | lexical + width + `range` | exact range membership |
| decimal64 | lexical + `range` | exact fixed-scale compare |
| boolean / empty / binary | lexical | `true`/`false`, no content, base64 |
| enumeration / bits | member set | value must be a listed enum / bit subset |
| identityref | **semantic** | must name an existing identity that is the `base` or derived from it |
| instance-identifier | coarse | non-empty, no whitespace |
| union / leafref / unresolved | none | silent |

`leaf-list`s are checked **per element** (JSON: each scalar array element at its
own range). Completion inserts typed value defaults for checked scalars
(boolean `true`, `empty` → empty element/`[null]`, first enum member, number
`0`; strings/bits/binary get a `"…"` placeholder); `union` keeps the neutral
stub.

## 4. Extension commands & settings

| Command | Action |
| --- | --- |
| `NETCONF: Insert get-config RPC` | insert `<rpc><get-config>` skeleton |
| `NETCONF: Insert edit-config RPC` | insert `<rpc><edit-config>` skeleton |
| `NETCONF: Insert hello` | insert `<hello>` skeleton |
| `NETCONF: Insert config payload` | insert `<config>` payload skeleton |

| Setting | Default | Meaning |
| --- | --- | --- |
| `netconf.indentSize` | `4` | formatter indentation width (2–8) |

## 5. Examples

Hand-test everything against the `examples/` folder:

- `example-demo.yang`, `example-ietf-interfaces.yang` — demo modules (the second
  augments the first);
- `example-netconf-config.xml` — a `<config>` payload demonstrating mapping,
  cross-module augment, goto/hover/diagnostics;
- `example-netconf-data.json` — the RFC 7951 mirror.

## 6. Limitations / not implemented

- `leafref` value *chasing* and fully-semantic `instance-identifier` (D31).
- NETCONF envelope *goto* modeling (built-in ops are known for templates/
  completion, not schema-goto).
- Quick-fixes / `codeAction`, comment-out.
- `didChangeWatchedFiles` re-scan, incremental repository compile,
  out-of-workspace include dirs, multiple workspace folders.
- `.yin` (XML) input; YANG compile happens through `yrepo` (see `architecture.md` §14).
