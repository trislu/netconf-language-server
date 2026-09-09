# NETCONF instance documents — write (templates & completion)

Authoring helpers for NETCONF XML and RFC 7951 JSON: skeleton templates plus
schema-driven completion.

## For users

- **NETCONF templates**: run a `NETCONF: Insert …` command from the command
  palette to drop a ready-to-fill skeleton at the caret — `hello`,
  `get-config`, `edit-config`, or a `<config>` payload (see
  [10-commands-and-settings.md](10-commands-and-settings.md)).
- **Completion**
  - XML: inside a recognized container/list (or a `<config>` payload), child
    element names are offered — with `key` placeholders and an automatic
    `xmlns` when a child lives in a different namespace; under `<rpc>` the
    built-in NETCONF operations and your compiled module RPCs.
  - JSON: RFC 7951 member names at a fresh object slot — root members are
    always `module:name`; nested members are bare when they share the parent
    module, `module:name` otherwise.
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
