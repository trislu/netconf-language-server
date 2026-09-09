# NETCONF instance documents — read (detect · goto · hover · diagnostics)

Serving `.xml` NETCONF envelopes/payloads and RFC 7951 JSON as *instance
documents*: detection, then read-side goto/hover/diagnostics against the
compiled YANG library.

## For users

- **When it kicks in (detection)**: an `.xml`/`.json` file is treated as a
  NETCONF *instance document* based on its content, not its file name — when
  the root element / top-level keys look like a NETCONF message envelope, a
  `<config>` payload, or a module's data tree. A file that matches nothing
  stays **dormant**: no NETCONF diagnostics/goto/hover/completion, so
  ordinary XML/JSON editing is completely untouched (the editor's built-in
  providers keep doing tokens/folding/formatting).
- **Read features** — on an element (`<module:name>`, XML) or member key
  (`"module:name"`, JSON):
  - **goto** to the defining YANG node, resolving through `choice`/`case`
    wrappers and cross-module `augment`s; **hover** shows the schema snippet
    and the node's kind/type/keys.
  - **diagnostics** (Problems panel, `netconf` source) flag e.g. a node that
    isn't a valid child of its parent, a name that exists only in a different
    module/namespace, a missing mandatory node or list `key`, a `choice` with
    no case instantiated, and a leaf value that fails validation (see
    [09-value-validation.md](09-value-validation.md)). Inside a `<filter>`
    subtree, XML suppresses the depth/value checks — partial content is legal
    there.
- **When it runs / how it performs**: detection happens on open, and read
  features are computed on demand against the compiled YANG modules —
  checking an instance file parses only that file; editing elsewhere is
  unaffected.
- **Limitations**: inside a NETCONF *envelope*, goto is limited — the built-in
  RPCs are known for templates/completion, not schema-goto.

## For developers

- **Implementation**: `src/xml.rs`, `src/json.rs`, `src/inst*.rs`,
  `src/inst_map.rs`/`src/jmap.rs`, `src/inst.rs` (classification in
  `src/server.rs`); resolution through `choice`/`case` wrappers and
  cross-module `augment`s (D29/D30) to the node's `origin_module`.
- **Serving model**: instance parsing is on demand against the compiled YANG
  library — see [`../serving-large-trees.md`](../serving-large-trees.md).
- **Tests**: netconf unit tests across the instance modules.
