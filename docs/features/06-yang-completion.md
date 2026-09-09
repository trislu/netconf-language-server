# Completion (YANG)

Part of the YANG authoring family. In-editor completion for YANG authoring —
names for `type`, `uses`, identity `base`, and node-path arguments — drawn
from your own modules and the ones you import. (Completion inside NETCONF
instance documents is covered in
[`08-netconf-write.md`](08-netconf-write.md).)

## For users

- **What it offers**
  - `type` argument: the builtin types plus local and imported `prefix:name`
    typedefs;
  - `uses` argument: local and imported groupings;
  - identity `base` argument: local and imported identities;
  - `augment`/`deviation` target paths and a `leafref`'s `path` argument:
    schema node names along the path, offered as you type it.
- **How to trigger it**: invoke completion with the caret inside one of those
  arguments (VS Code <kbd>Ctrl</kbd>+<kbd>Space</kbd> / <kbd>Tab</kbd>). Candidates are schema-based rather
  than word-based: they re-filter as you type, and a typed `prefix:` narrows
  the list to that module's names.
- **Limitations / customization**: completion is limited to the argument
  positions above, and candidates always reflect the modules in your
  workspace — there are no settings.

## For developers

- **Implementation**: `src/completion.rs`; candidates from
  `yrepo::Library::type_candidates` / `grouping_candidates` /
  `identity_candidates`, plus in-server path completion for `augment`/
  `deviation`/`leafref` (see [`../architecture.md`](../architecture.md) §6.2 /
  yrepo README).
- **Coupling**: shares the compiled `Library` snapshot with goto/hover; XML/JSON
  completion lives with instance writing.
- **Tests**: unit tests in `src/completion.rs`.
