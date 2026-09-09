# Leaf value validation

Validating `leaf`/`leaf-list` **values** in NETCONF instance documents against
the module schema, and inserting typed defaults when you complete a leaf.

## For users

- **What it checks**: `leaf`/`leaf-list` values you type in an instance
  document are validated against the module's schema as soon as their type
  resolves (through any `typedef` chain) to a single builtin type:

| Type | Checked | Notes |
| --- | --- | --- |
| string | length + pattern | both may accumulate along a typedef chain |
| int8…64 / uint8…64 | lexical + width + `range` | exact range membership |
| decimal64 | lexical + `range` | exact fixed-scale compare |
| boolean / empty / binary | lexical | `true`/`false`, no content, base64 |
| enumeration / bits | member set | value must be a listed enum / bit subset |
| identityref | **semantic** | must name an identity that is the `base` or derived from it |
| instance-identifier | coarse | non-empty, no whitespace |
| union / leafref / unresolved | none | silent |

- **Deliberately not checked**: `union` values (RFC 7950 §9.12 — a bare value
  can't be attributed to one member), `leafref`s, and types that don't resolve
  to a scalar builtin.
- **When it runs**: as you type, alongside the instance diagnostics (see
  [07-netconf-instances-read.md](07-netconf-instances-read.md)). `leaf-list`s
  are checked per element — JSON checks each array element against its own
  range.
- **Related**: the same type knowledge drives the defaults completion inserts
  for checked scalars (boolean `true`, `empty` → empty element / `[null]`,
  first enum member, number `0`; strings/bits/binary get a `"…"` placeholder;
  `union` keeps the neutral stub) — see
  [08-netconf-write.md](08-netconf-write.md).

## For developers

- **Implementation**: `src/valcheck.rs` (scalar classification/facets from
  `yrepo` — `TypeFacets`/`ValueType`, `resolve_type`); wired into instance
  diagnostics (`netconf_bad_value`/`json_bad_value`) and completion defaults.
- **Coupling**: shares `yrepo` type resolution with YANG completion and
  goto/hover.
- **Tests**: `src/valcheck.rs` unit tests + `yrepo` type/value tests.
