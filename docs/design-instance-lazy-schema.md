# Lazy schema for instance documents (design)

Status: implemented (2026-09-11); yrepo 0.7.1 released, LS 0.7.0 in release. Goal: editing NETCONF instance documents
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
have changed. The two XML positions whose children are the **workspace-wide**
set are served from these summaries — they can never be answered by one
module's closure:

- a **payload wrapper** (`<config>`, `<filter>`; at the document root or nested
  under `<edit-config>`/`<get-config>`) → every summarized module's top-level
  data roots;
- the root `<rpc>` envelope → the built-in NETCONF operations plus every
  summarized module's top-level `rpc`/`notification` (the summary index carries
  those names too, so `ModuleInfo` has `top_data`/`rpcs`/`notifications`).

Any other (mapped) position without a compiled schema returns nothing; `<data>`
is not a position at all (server-side `<rpc-reply>` output). A payload wrapper's
subtree is mapped whichever way its envelope resolves:

- when the envelope module (`ietf-netconf`) **is** compiled, the wrapper maps to
  its schema node, whose kind is **`anyxml`** (`edit-config`'s `config`,
  `get`/`get-config`'s `filter`); the mapping treats that node's content as a
  data tree (top-level nodes of any module) instead of opaque XML;
- when it is **not** compiled, the envelope is silent and the mapping promotes
  the wrapper (`<config>`/`<filter>` under `<edit-config>`/`<get-config>`,
  `<data>` under `<rpc-reply>`) to a data position.

A `<filter>` is recognized as partial content wherever it sits (document root or
nested under `<get>`/`<get-config>`), so the now-mapped subtree is exempt from
mandatory/key/choice checks.

**Tier 2 — per-document module closure (everything else).**
The modules a document actually references are resolved through the summary
index — XML: the distinct element namespaces; JSON: the distinct `module:name`
member qualifiers — and **each one's closure** is materialized into the
repository (existing lazy catalog resolution over `NameIndex` candidates) in one
reconcile, then the snapshot is compiled and
diagnostics/hover/goto/nested completion use the existing `Library`-based code
paths. The document's module state is therefore a **set** per document: a NETCONF
message spans several modules (the envelope, each data root's module, every
augmenting module that contributes a node), and seeding only the root's module
left nested positions unmappable. No whole-tree compile.

## Integration points (netconf-language-server)

| piece | change |
| --- | --- |
| `schema_idx::module_summaries(lib)` | keep; add `module_summaries_from_summary(&SummaryIndex) -> Vec<ModuleInfo>` (same shape: name/namespace/top_data) |
| `Server.classify` | when `snapshot().lib` is `None`, fall back to the summary index so valid NETCONF docs are no longer reported `NotNetconf` |
| `xcomp::handle` / `jcomp::handle` | add a summaries-only entry (payload-wrapper + `<rpc>` children); `Library` path unchanged (its closure-only roots are the no-summary fallback) |
| `xml_ctx` / `json_ctx` | seed **every** module the document references through Tier 1 (XML: distinct element namespaces; JSON: distinct member qualifiers), materialize their closures (Tier 2) in one reconcile, then proceed |
| `Server` state | `summary: RwLock<Option<Arc<SummaryIndex>>>`, `summary_build: Mutex<()>` (mirror `refidx`), invalidation next to `invalidate_refidx` |
| references/rename | unchanged: they keep their own lazy `ReferenceIndex` and are YANG-only |

## Triggers

- completion in an XML/JSON instance doc → ensure summary index (progress on
  first use); Tier 2 only when a namespace is determinable.
- diagnostics/hover/goto for an instance doc → ensure summary index; if the doc
  has a root namespace, Tier 2; otherwise stay dormant (no unfounded errors).
- YANG-only actions never touch the summary index or `ReferenceIndex` beyond
  their existing paths.

## Measured (release build, external corpus 165 521 files, by URL)

| metric | measured |
| --- | --- |
| Tier-1 summary scan (first instance completion) | 12.05 s, 165 521 modules; server RSS 51 → 481 MB |
| XML root completion, first → second (`rpc`/`config` + `<`) | 13.38 s → 0.41 s, 79 371 items (cached) |
| JSON root completion (`{"`) | 0.39 s, 79 362 items |
| Tier-2 per-module closure (real `ietf-interfaces` doc) | 406 candidate headers / 17.7 ms; diagnostics 0.24 s |
| classify | `DataTree("ietf-interfaces")` (was `NotNetconf`) |
| find-all-references during instance work | **not triggered** (summary/instance paths never call `ensure_refidx`) |

In-progress text is tolerated: XML ending in a lone `<` and JSON ending in a
lone `"` both repair to a parseable document (`src/incomplete.rs`) before the
root completion runs.
