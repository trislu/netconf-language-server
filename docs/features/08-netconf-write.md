# NETCONF instance documents — write (templates & completion)

Authoring helpers for NETCONF XML and RFC 7951 JSON: skeleton templates plus
schema-driven completion.

## For users

- **NETCONF templates**: run a `NETCONF: Insert …` command from the command
  palette to drop a ready-to-fill skeleton at the caret — `hello`,
  `get-config`, `edit-config`, or a `<config>` payload (see
  [10-commands-and-settings.md](10-commands-and-settings.md)).
- **Completion**
  - XML: inside a recognized container/list, child element names are offered —
    with `key` placeholders and an automatic `xmlns` when a child lives in a
    different namespace. Two NETCONF positions are answered **workspace-wide**
    (from the module-summary index, so they work on a giant tree with no YANG
    file open):
    - a **payload wrapper** — `<config>` (the `edit-config` payload, at the
      document root *or* nested under `<edit-config>`) or `<filter>` — offers
      every module's top-level data roots;
    - the `<rpc>` envelope offers the built-in NETCONF operations plus every
      module's top-level `rpc`/`notification`.

    `<data>` is *not* a completion position: it only appears in a server's
    `<rpc-reply>` (read and validated, never authored).
  - Once a data node is present, completion continues **down the data tree**:
    the server compiles the closure of every module the document references
    (each element's namespace), so children of `<interfaces>`, an `<interface>`
    entry, and so on are offered even inside an `<edit-config>` payload or a
    `<get-config>`/`<filter>` selection. A list entry's `key` leafs are
    inserted as placeholders, and a child in another namespace carries an
    `xmlns`.
  - An **`identityref` leaf** (e.g. `<type>` under an interface) completes its
    **values** from a workspace-wide identity graph. At an `xmlns:` attribute
    name in the leaf's start tag, every module contributing an identity derived
    from the leaf's `base` is offered as `xmlns:<module-prefix>="<namespace>"`
    (with its identities listed); in the leaf's content, the matching
    identities are offered as `<module-prefix>:<identity>`, adding the `xmlns`
    declaration when the prefix is not yet in scope. The module's YANG prefix is
    reused as the XML prefix, and identities from modules outside the open
    closure are included. In JSON the same graph completes an `identityref`
    value inside its string as `module-name:identity-name` (RFC 7951 §6.8), so
    no prefix declaration is involved.
  - A leaf whose type has a **finite value set** completes its value: an
    `enumeration` offers its members, a `bits` its bit names, and a `boolean`
    `true`/`false` — in XML content and in a JSON value (a string for
    enumeration/bits, a literal for boolean). `union` stays silent; no
    workspace index is needed, the values are inline in the leaf's resolved
    type.
  - JSON: RFC 7951 member names at a fresh object slot — root members are
    always `module:name`; nested members are bare when they share the parent
    module, `module:name` otherwise. As in XML, every module the document
    references (its distinct `module:name` qualifiers) is compiled, so a
    document spanning several modules completes in all of them.
- **When it runs / customization**: completion works wherever the read
  features are active (see
  [07-netconf-instances-read.md](07-netconf-instances-read.md)) and templates
  run on demand — there are no settings.
- **Limitations**: no quick-fix / code-action or comment-out helpers yet, and
  the value defaults inserted for completed leaves follow
  [09-value-validation.md](09-value-validation.md).

## For developers

- **Implementation**: templates in `src/template.rs` + `src/server.rs`
  (`netconf/insertTemplate`) with the client-side command wrappers; XML
  completion `src/xcomp.rs`, JSON completion `src/jcomp.rs`, node maps
  `src/inst_map.rs`/`src/jmap.rs`.
- **Coupling**: JSON completion mirrors the read-side RFC 7951 naming rules
  from [`07-netconf-instances-read.md`](07-netconf-instances-read.md).
- **Tests**: unit tests in the completion/map modules.
