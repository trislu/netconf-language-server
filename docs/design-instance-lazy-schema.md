# Lazy schema for instance documents (design)

Status: implementing (2026-09-11). Goal: editing NETCONF instance documents
(XML/RPC, RFC 7951 JSON) must work on a giant workspace **with no YANG file
open**, without paying a whole-tree reference index (that stays lazy and is
never triggered here).

## Two tiers

**Tier 1 — module-summary index (data roots).**
`initialize` only builds the parse-free name index, so nothing knows a module's
`namespace` or top-level data nodes. The first instance-document completion
(XML `<`, JSON `"` in the root object) or the first diagnostic for a document
that already has a root triggers a whole-tree parse-level **summary scan**
(yrepo `SummaryIndex`): module name, namespace, revision, top-level
data/rpc/notification names. It is single-flight, progress-visible
(`client::Progress`), cached, and invalidated like `refidx` when disk content may
have changed. Completion for the root position is served from these summaries;
nested positions without a compiled schema return nothing.

**Tier 2 — per-module open closure (everything else).**
Once a root namespace is known (the user typed/picked a root, or the parsed
document already has one), resolve it through the summary index to a module
name/url, materialize **only that module's closure** into the repository
(existing lazy catalog resolution over `NameIndex` candidates), compile the
snapshot, and serve diagnostics/hover/goto/nested completion with the existing
`Library`-based code paths. No whole-tree compile.

## Integration points (netconf-language-server)

| piece | change |
| --- | --- |
| `schema_idx::module_summaries(lib)` | keep; add `module_summaries_from_summary(&SummaryIndex) -> Vec<ModuleInfo>` (same shape: name/namespace/top_data) |
| `Server.classify` | when `snapshot().lib` is `None`, fall back to the summary index so valid NETCONF docs are no longer reported `NotNetconf` |
| `xcomp::handle` / `jcomp::handle` | add a summaries-only entry (root data roots + NETCONF operations); `Library` path unchanged |
| `xml_ctx` / `json_ctx` | if `lib` is `None`, parse the document root, resolve its namespace via Tier 1, materialize that module (Tier 2), then proceed |
| `Server` state | `summary: RwLock<Option<Arc<SummaryIndex>>>`, `summary_build: Mutex<()>` (mirror `refidx`), invalidation next to `invalidate_refidx` |
| references/rename | unchanged: they keep their own lazy `ReferenceIndex` and are YANG-only |

## Triggers

- completion in an XML/JSON instance doc → ensure summary index (progress on
  first use); Tier 2 only when a namespace is determinable.
- diagnostics/hover/goto for an instance doc → ensure summary index; if the doc
  has a root namespace, Tier 2; otherwise stay dormant (no unfounded errors).
- YANG-only actions never touch the summary index or `ReferenceIndex` beyond
  their existing paths.

## Measurements to record (acceptance)

Summary scan wall + VmHWM on the external corpus (165 521 files, by URL);
first vs second root-completion latency; Tier-2 closure materialization latency
and diagnostics on a real module; confirmation that find-all-references still
triggers only `ReferenceIndex`.
