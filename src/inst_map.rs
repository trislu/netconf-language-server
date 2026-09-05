//! Instance → schema mapping for XML documents (M1, decisions D29/D30).
//!
//! Walks an [`XmlDoc`] and maps each element to the effective `yrepo` schema
//! node it instantiates (module + arena `NodeId`), using the D29/D30 instance
//! APIs: `ModuleRecord::data_children` (through `choice`/`case`), `rpc_input`
//! for RPC operation bodies, and per-node **instance module** namespaces.
//!
//! Also produces the two core diagnostics — unknown element and element in the
//! wrong namespace — inside payload contexts. Documents that are not NETCONF
//! (no root module match, or envelope internals without an envelope schema)
//! stay dormant: nothing is mapped and no diagnostics are emitted.

use std::ops::Range;

use yrepo::{Library, NodeKind, SchemaNode};

use crate::depth;
use crate::inst::{NETCONF_BASE_NS, NETCONF_NOTIFICATION_NS};
use crate::xml::XmlDoc;

/// A schema node an element maps to: module + arena id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Module whose arena holds `id` (nodes of `uses`/augment live here too).
    pub module: String,
    /// Arena `NodeId` inside that module.
    pub id: usize,
}

/// A diagnostic over an element name range (source: "netconf").
#[derive(Debug, Clone)]
pub struct InstDiag {
    pub range: Range<usize>,
    pub message: String,
    /// Stable code: `netconf_unknown_node` / `netconf_wrong_ns`.
    pub code: &'static str,
}

#[derive(Debug, Clone)]
enum Slot {
    /// This element maps to a schema node.
    Mapped(Resolved),
    /// Payload wrapper (`<config>`/`<data>`/`<filter>`) — children are
    /// top-level module nodes and are diagnosed.
    Top,
    /// `<rpc>` envelope — children are operation elements: a compiled module's
    /// top-level rpc/notification maps, anything else is silent (built-in
    /// NETCONF ops we do not model yet).
    RpcOp,
    /// Envelope/message internals without a schema yet — subtree silent.
    Silent,
    /// Unknown/dormant element — subtree skipped silently.
    Dead,
    /// `anyxml`/`anydata` content — unknown model, subtree silent.
    Opaque,
}

/// Result of mapping a whole document.
#[derive(Debug, Clone)]
pub struct InstMap {
    slots: Vec<Slot>,
    pub diags: Vec<InstDiag>,
}

impl InstMap {
    /// The schema node an element maps to, if it is a mapped data node.
    pub fn resolved(&self, elem: usize) -> Option<&Resolved> {
        match self.slots.get(elem) {
            Some(Slot::Mapped(r)) => Some(r),
            _ => None,
        }
    }
}

/// Namespace URI that owns `node` in instance data (its instance module's ns).
fn instance_ns(lib: &Library, node: &SchemaNode) -> Option<String> {
    lib.module(node.instance_module())
        .and_then(|m| m.namespace())
        .map(str::to_owned)
}

/// Whether an element is a NETCONF payload wrapper.
fn is_payload_wrapper(local: &str) -> bool {
    matches!(local, "config" | "data" | "filter")
}

/// Whether an element is an (unsupported-for-now) message envelope root.
fn is_message_envelope(ns: Option<&str>, local: &str) -> bool {
    match ns {
        Some(NETCONF_BASE_NS) => matches!(local, "rpc" | "rpc-reply" | "hello"),
        Some(NETCONF_NOTIFICATION_NS) => true,
        _ => false,
    }
}

/// Find the top-level data node of `e` across all modules (payload context).
fn match_top(lib: &Library, doc: &XmlDoc, e: usize) -> Option<Resolved> {
    let node = &doc.nodes[e];
    let ns = node.ns.as_deref()?;
    for m in lib.modules() {
        if m.namespace() != Some(ns) {
            continue;
        }
        for &id in m.top_nodes() {
            let Some(n) = m.node(id) else { continue };
            if n.kind().is_data() && n.name() == node.local {
                return Some(Resolved {
                    module: m.name().to_owned(),
                    id,
                });
            }
        }
    }
    None
}

/// Find a top-level `rpc`/`notification` of `e` across all modules (the
/// operation element directly under `<rpc>`).
fn match_top_op(lib: &Library, doc: &XmlDoc, e: usize) -> Option<Resolved> {
    let node = &doc.nodes[e];
    let ns = node.ns.as_deref()?;
    for m in lib.modules() {
        if m.namespace() != Some(ns) {
            continue;
        }
        for &id in m.top_nodes() {
            let Some(n) = m.node(id) else { continue };
            if matches!(n.kind(), NodeKind::Rpc | NodeKind::Notification) && n.name() == node.local
            {
                return Some(Resolved {
                    module: m.name().to_owned(),
                    id,
                });
            }
        }
    }
    None
}

/// Candidate schema children an element may match under `(module,id)`.
fn candidates(lib: &Library, module: &str, id: usize) -> Vec<(usize, String, Option<String>)> {
    // (node id, name, instance ns)
    let rec = match lib.module(module) {
        Some(r) => r,
        None => return Vec::new(),
    };
    let Some(node) = rec.node(id) else {
        return Vec::new();
    };
    let ids: Vec<usize> = match node.kind() {
        // RPC/action operation bodies sit under `input` (request direction).
        NodeKind::Rpc | NodeKind::Action => rec
            .rpc_input(id)
            .map(|i| rec.data_children(i))
            .unwrap_or_default(),
        _ => rec.data_children(id),
    };
    ids.into_iter()
        .filter_map(|cid| {
            let n = rec.node(cid)?;
            Some((cid, n.name().to_owned(), instance_ns(lib, n)))
        })
        .collect()
}

/// Map a child element against schema children of its parent; emits a
/// diagnostic when it is unknown or in the wrong namespace.
fn match_child(
    lib: &Library,
    doc: &XmlDoc,
    e: usize,
    module: &str,
    id: usize,
    diags: &mut Vec<InstDiag>,
) -> Slot {
    let elem = &doc.nodes[e];
    let by_name: Vec<_> = candidates(lib, module, id)
        .into_iter()
        .filter(|(_, name, _)| name == &elem.local)
        .collect();
    if let Some((cid, _, _)) = by_name
        .iter()
        .find(|(_, _, ins)| ins.as_deref() == elem.ns.as_deref())
    {
        return Slot::Mapped(Resolved {
            module: module.to_owned(),
            id: *cid,
        });
    }
    // A name exists but in a different namespace here.
    if let Some((_, _, expected)) = by_name.first() {
        let exp = expected.as_deref().unwrap_or("<no namespace>");
        let got = elem.ns.as_deref().unwrap_or("<no namespace>");
        diags.push(InstDiag {
            range: elem.name_range.clone(),
            message: format!(
                "element `{}` is in namespace `{}`; expected `{}` here",
                elem.local, got, exp
            ),
            code: "netconf_wrong_ns",
        });
    } else {
        diags.push(InstDiag {
            range: elem.name_range.clone(),
            message: format!(
                "unknown element `<{}>` (not a child of this schema node)",
                elem.local
            ),
            code: "netconf_unknown_node",
        });
    }
    Slot::Dead
}

/// The node a mapped element refers to (for goto/hover), cloned `Location`.
pub fn defining_of(lib: &Library, res: &Resolved) -> Option<yrepo::Location> {
    let rec = lib.module(&res.module)?;
    let node = rec.node(res.id)?;
    Some(node.defining().clone())
}

/// Map every element of `doc` to a schema node (best effort) and collect the
/// payload-context diagnostics.
pub fn map_doc(doc: &XmlDoc, lib: &Library) -> InstMap {
    let mut slots: Vec<Slot> = vec![Slot::Dead; doc.nodes.len()];
    let mut diags = Vec::new();

    for i in 0..doc.nodes.len() {
        let elem = &doc.nodes[i];
        let outcome = match elem.parent {
            None => {
                // Root: payload wrapper, message envelope, or a module data node.
                if elem.ns.as_deref() == Some(NETCONF_BASE_NS) && is_payload_wrapper(&elem.local) {
                    Slot::Top
                } else if elem.ns.as_deref() == Some(NETCONF_BASE_NS) && elem.local == "rpc" {
                    // Operation elements under <rpc> can be module RPCs.
                    Slot::RpcOp
                } else if is_message_envelope(elem.ns.as_deref(), &elem.local) {
                    Slot::Silent
                } else {
                    match match_top(lib, doc, i) {
                        Some(r) => Slot::Mapped(r),
                        None => Slot::Dead,
                    }
                }
            }
            Some(p) => match slots.get(p) {
                Some(Slot::Mapped(r)) => {
                    let node = lib
                        .module(&r.module)
                        .and_then(|rec| rec.node(r.id))
                        .map(|n| n.kind());
                    match node {
                        // anyxml/anydata content has no (known) schema.
                        Some(NodeKind::Anyxml) | Some(NodeKind::Anydata) => Slot::Opaque,
                        _ => match_child(lib, doc, i, &r.module, r.id, &mut diags),
                    }
                }
                Some(Slot::Top) => match match_top(lib, doc, i) {
                    Some(r) => Slot::Mapped(r),
                    None => {
                        diags.push(InstDiag {
                            range: elem.name_range.clone(),
                            message: format!(
                                "unknown element `<{}>` (no compiled module has this \
                                 top-level data node in namespace `{}`)",
                                elem.local,
                                elem.ns.as_deref().unwrap_or("<none>")
                            ),
                            code: "netconf_unknown_node",
                        });
                        Slot::Dead
                    }
                },
                // Under <rpc>: a compiled module's operation maps; built-in ops
                // (get-config, edit-config, …) have no schema yet and stay silent.
                Some(Slot::RpcOp) => match match_top_op(lib, doc, i) {
                    Some(r) => Slot::Mapped(r),
                    None => Slot::Silent,
                },
                Some(Slot::Silent) => Slot::Silent,
                Some(Slot::Dead) | None => Slot::Dead,
                Some(Slot::Opaque) => Slot::Opaque,
            },
        };
        slots[i] = outcome;
    }

    // Which elements sit inside a `<filter>` subtree — partial content is
    // legal there, so the mandatory/key/choice checks below are suppressed.
    let in_filter = filter_mask(doc);

    // M4 diagnostics depth: for every present container/list, report its
    // missing required direct children (mandatory nodes, list keys) and any
    // `choice` with no instantiated case (best effort, D27).
    for i in 0..doc.nodes.len() {
        if in_filter[i] {
            continue;
        }
        let Slot::Mapped(r) = &slots[i] else {
            continue;
        };
        let Some(rec) = lib.module(&r.module) else {
            continue;
        };
        let Some(node) = rec.node(r.id) else {
            continue;
        };
        if !matches!(node.kind(), NodeKind::Container | NodeKind::List) {
            continue;
        }
        // Schema ids of this element's children that resolved (all live in the
        // same module arena as the parent, augment-born nodes included).
        let present: Vec<usize> = (0..doc.nodes.len())
            .filter(|&j| doc.nodes[j].parent == Some(i))
            .filter_map(|j| match slots.get(j) {
                Some(Slot::Mapped(c)) if c.module == r.module => Some(c.id),
                _ => None,
            })
            .collect();
        let report = depth::analyze(lib, &r.module, r.id, &|s| present.contains(&s));
        if !report.missing.is_empty() {
            let names = report.missing.join(", ");
            let word = if report.missing.len() == 1 {
                "element"
            } else {
                "elements"
            };
            diags.push(InstDiag {
                range: doc.nodes[i].name_range.clone(),
                message: format!("missing required {word}: {names}"),
                code: "netconf_missing_node",
            });
        }
        for gap in &report.choices {
            diags.push(InstDiag {
                range: doc.nodes[i].name_range.clone(),
                message: format!(
                    "no case of choice `{}` is present (expected one of: {})",
                    gap.name,
                    gap.options.join(", ")
                ),
                code: "netconf_missing_choice",
            });
        }
    }

    InstMap { slots, diags }
}

/// Which elements sit inside a `<filter>` subtree — partial content is legal
/// there, so the mandatory/key/choice and value checks are suppressed.
fn filter_mask(doc: &XmlDoc) -> Vec<bool> {
    let mut in_filter = vec![false; doc.nodes.len()];
    for i in 0..doc.nodes.len() {
        let elem = &doc.nodes[i];
        let own = elem.parent.is_none()
            && elem.ns.as_deref() == Some(NETCONF_BASE_NS)
            && elem.local == "filter";
        in_filter[i] = own || elem.parent.map(|p| in_filter[p]).unwrap_or(false);
    }
    in_filter
}

/// M5 leaf *value* validation over a whole document (best effort, D31): for
/// every mapped **leaf**/leaf-list element whose type reduces to a checked
/// scalar, evaluate its text content against the resolved [`yrepo::ValueType`].
/// Suppressed inside a `<filter>` (partial content is legal there).
pub fn value_diags(doc: &XmlDoc, text: &str, lib: &Library) -> Vec<InstDiag> {
    let map = map_doc(doc, lib);
    let in_filter = filter_mask(doc);
    let mut diags = Vec::new();
    for (i, _n) in doc.nodes.iter().enumerate() {
        if in_filter[i] {
            continue;
        }
        let Some(res) = map.resolved(i).cloned() else {
            continue;
        };
        let Some(rec) = lib.module(&res.module) else {
            continue;
        };
        let Some(node) = rec.node(res.id) else {
            continue;
        };
        if !matches!(node.kind(), NodeKind::Leaf | NodeKind::LeafList) {
            continue;
        }
        let Some(vt) = lib.value_type(&res.module, res.id) else {
            continue;
        };
        let Some((range, value)) = doc.leaf_text(text, i) else {
            continue;
        };
        if let Some(msg) = crate::valcheck::check_value(lib, &res.module, &vt, &value) {
            diags.push(InstDiag {
                range,
                message: format!("invalid value: {msg}"),
                code: "netconf_bad_value",
            });
        }
    }
    diags
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use yrepo::Repository;

    use super::*;
    use crate::xml::parse;

    const MOD_IF: &str = r#"module ietf-interfaces {
  yang-version 1.1;
  namespace "urn:if";
  prefix if;
  revision 2026-01-01;
  container interfaces {
    list interface {
      key "name";
      leaf name { type string; }
      leaf enabled { type boolean; }
      choice speed {
        case fixed { leaf fixed-mbps { type uint32; } }
        leaf auto { type empty; }
      }
    }
  }
  rpc get {
    input { leaf with-defaults { type string; } }
  }
}"#;

    fn compile() -> Arc<Library> {
        let mut repo = Repository::new();
        repo.upsert("/if.yang", MOD_IF);
        repo.compile().library.expect("library")
    }

    fn find(doc: &XmlDoc, local: &str) -> usize {
        doc.nodes
            .iter()
            .position(|n| n.local == local)
            .unwrap_or_else(|| panic!("no element {local}"))
    }

    #[test]
    fn maps_data_tree_down_through_choice_case() {
        let lib = compile();
        let text = r#"<interfaces xmlns="urn:if">
  <interface>
    <name>eth0</name>
    <fixed-mbps>1000</fixed-mbps>
  </interface>
</interfaces>"#;
        let doc = parse(text).expect("parse");
        let map = map_doc(&doc, &lib);
        assert!(map.diags.is_empty(), "{:?}", map.diags);
        // container interfaces
        let c = find(&doc, "interfaces");
        let r = map.resolved(c).expect("interfaces maps");
        assert_eq!(
            lib.module(&r.module).unwrap().node(r.id).unwrap().name(),
            "interfaces"
        );
        // list entry interface
        let entry = find(&doc, "interface");
        assert!(map.resolved(entry).is_some());
        // leaf name (key)
        let name = find(&doc, "name");
        assert_eq!(
            lib.module(&map.resolved(name).unwrap().module)
                .unwrap()
                .node(map.resolved(name).unwrap().id)
                .unwrap()
                .name(),
            "name"
        );
        // leaf under a choice case resolves through choice/case wrappers.
        let fixed = find(&doc, "fixed-mbps");
        assert!(map.resolved(fixed).is_some(), "case leaf must map");
        // A compiled module RPC under <rpc> maps (children via its input); the
        // built-in <get-config> op has no schema yet and stays silent.
        let rpc_text = r#"<rpc xmlns="urn:ietf:params:xml:ns:netconf:base:1.0"><get xmlns="urn:if"><with-defaults>all</with-defaults></get></rpc>"#;
        let rdoc = parse(rpc_text).expect("parse rpc");
        let rmap = map_doc(&rdoc, &lib);
        assert!(rmap.diags.is_empty());
        assert!(rmap.resolved(find(&rdoc, "rpc")).is_none());
        assert!(rmap.resolved(find(&rdoc, "get")).is_some());
        assert!(rmap.resolved(find(&rdoc, "with-defaults")).is_some());

        let builtin_text = r#"<rpc xmlns="urn:ietf:params:xml:ns:netconf:base:1.0"><get-config xmlns="urn:ietf:params:xml:ns:netconf:base:1.0"><source><running/></source></get-config></rpc>"#;
        let bdoc = parse(builtin_text).expect("parse builtin");
        let bmap = map_doc(&bdoc, &lib);
        assert!(bmap.diags.is_empty());
        assert!(bmap.resolved(find(&bdoc, "get-config")).is_none());
    }

    #[test]
    fn config_payload_maps_top_level_across_modules_and_flags_unknowns() {
        let lib = compile();
        let text = r#"<config xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
  <interfaces xmlns="urn:if">
    <interface><name>x</name><bogus/></interface>
  </interfaces>
  <system xmlns="urn:demo"/>
</config>"#;
        let doc = parse(text).expect("parse");
        let map = map_doc(&doc, &lib);
        // <system> is not a top node of a compiled module → unknown; <bogus>
        // is not a child of interface.
        let unknown: Vec<&InstDiag> = map
            .diags
            .iter()
            .filter(|d| d.code == "netconf_unknown_node")
            .collect();
        assert_eq!(unknown.len(), 2, "{:?}", map.diags);
        // <interfaces> itself maps (namespace + top node).
        assert!(map.resolved(find(&doc, "interfaces")).is_some());
    }

    #[test]
    fn wrong_namespace_is_reported() {
        let lib = compile();
        // `name` is a child of `interface` only in urn:if.
        let text = r#"<interfaces xmlns="urn:if">
  <interface xmlns="urn:if">
    <name xmlns="urn:other">x</name>
  </interface>
</interfaces>"#;
        let doc = parse(text).expect("parse");
        let map = map_doc(&doc, &lib);
        assert!(
            map.diags.iter().any(|d| d.code == "netconf_wrong_ns"),
            "{:?}",
            map.diags
        );
    }

    #[test]
    fn foreign_and_message_docs_stay_dormant() {
        let lib = compile();
        // Root namespace not compiled → dormant, no diagnostics.
        let doc = parse(r#"<widget xmlns="urn:other"><x/></widget>"#).expect("parse");
        let map = map_doc(&doc, &lib);
        assert!(map.diags.is_empty());
        assert!(map.resolved(find(&doc, "widget")).is_none());
        // hello envelope → silent subtree.
        let doc = parse(
            r#"<hello xmlns="urn:ietf:params:xml:ns:netconf:base:1.0"><capabilities><capability>urn:x</capability></capabilities></hello>"#,
        )
        .expect("parse");
        let map = map_doc(&doc, &lib);
        assert!(map.diags.is_empty());
    }

    /// The real `examples/` modules: `example-ietf-interfaces` plus the
    /// augmenting `example-demo`.
    fn compile_demo() -> Arc<Library> {
        let mut repo = Repository::new();
        repo.upsert(
            "/example-ietf-interfaces.yang",
            include_str!("../examples/example-ietf-interfaces.yang"),
        );
        repo.upsert(
            "/example-demo.yang",
            include_str!("../examples/example-demo.yang"),
        );
        repo.compile().library.expect("library")
    }

    #[test]
    fn maps_cross_module_augmented_node_and_defines_to_its_module() {
        let lib = compile_demo();
        // example-demo augments /if:interfaces/if:interface with `admin-down`
        // (namespace urn:example:demo); example-ietf-interfaces owns the rest.
        let text = r#"<interfaces xmlns="urn:example:interfaces">
  <interface>
    <name>eth0</name>
    <enabled>true</enabled>
    <admin-down xmlns="urn:example:demo">false</admin-down>
  </interface>
</interfaces>"#;
        let doc = parse(text).expect("parse");
        let map = map_doc(&doc, &lib);
        assert!(map.diags.is_empty(), "{:?}", map.diags);
        for (local, in_demo) in [
            ("interfaces", false),
            ("interface", false),
            ("name", false),
            ("admin-down", true),
        ] {
            let e = find(&doc, local);
            let res = map
                .resolved(e)
                .unwrap_or_else(|| panic!("{local} unmapped"));
            let loc = defining_of(&lib, res).unwrap();
            let url: &str = &loc.url;
            if in_demo {
                assert!(url.ends_with("example-demo.yang"), "{url}");
            } else {
                assert!(url.ends_with("example-ietf-interfaces.yang"), "{url}");
            }
        }
    }

    const MOD_DEPTH: &str = r#"module d {
  yang-version 1.1;
  namespace "urn:d";
  prefix d;
  revision 2026-01-01;
  container server {
    leaf host { type string; mandatory true; }
    choice proto {
      case tcp { leaf port { type uint16; } }
      case udp { leaf sport { type uint16; } }
    }
    list iface {
      key "name";
      leaf name { type string; }
      leaf mtu { type uint32; mandatory true; }
    }
  }
}"#;

    fn compile_depth() -> Arc<Library> {
        let mut repo = Repository::new();
        repo.upsert("/d.yang", MOD_DEPTH);
        repo.compile().library.expect("library")
    }

    fn has_code(map: &InstMap, code: &str) -> bool {
        map.diags.iter().any(|d| d.code == code)
    }

    #[test]
    fn flags_missing_mandatory_keys_and_empty_choice() {
        let lib = compile_depth();
        // server present but missing mandatory `host` and any `proto` case; the
        // iface entry has its key but misses mandatory `mtu`.
        let text = r#"<server xmlns="urn:d">
  <iface>
    <name>eth0</name>
  </iface>
</server>"#;
        let doc = parse(text).expect("parse");
        let map = map_doc(&doc, &lib);
        assert!(has_code(&map, "netconf_missing_node"), "{:?}", map.diags);
        assert!(has_code(&map, "netconf_missing_choice"), "{:?}", map.diags);
        let missing: Vec<&str> = map
            .diags
            .iter()
            .filter(|d| d.code == "netconf_missing_node")
            .map(|d| d.message.as_str())
            .collect();
        let joined = missing.join("\n");
        assert!(joined.contains("host"), "{joined}");
        assert!(joined.contains("mtu"), "{joined}");
        let empty: Vec<&InstDiag> = map
            .diags
            .iter()
            .filter(|d| d.code == "netconf_missing_choice")
            .collect();
        assert_eq!(empty.len(), 1, "{:?}", empty);
        assert!(empty[0].message.contains("proto"), "{:?}", empty[0]);
    }

    #[test]
    fn complete_subtree_has_no_depth_diagnostics() {
        let lib = compile_depth();
        let text = r#"<server xmlns="urn:d">
  <host>h1</host>
  <port>443</port>
  <iface>
    <name>eth0</name>
    <mtu>1500</mtu>
  </iface>
</server>"#;
        let doc = parse(text).expect("parse");
        let map = map_doc(&doc, &lib);
        assert!(map.diags.is_empty(), "{:?}", map.diags);
    }

    #[test]
    fn filter_subtree_suppresses_depth_diagnostics() {
        let lib = compile_depth();
        // Partial content inside <filter> is legal → no depth diagnostics.
        let text = r#"<filter xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
  <server xmlns="urn:d">
    <iface>
      <name>x</name>
    </iface>
  </server>
</filter>"#;
        let doc = parse(text).expect("parse");
        let map = map_doc(&doc, &lib);
        assert!(
            !map.diags
                .iter()
                .any(|d| d.code.starts_with("netconf_missing")),
            "{:?}",
            map.diags
        );
    }

    const MOD_VALUE: &str = r#"module v {
  yang-version 1.1;
  namespace "urn:v";
  prefix v;
  revision 2026-01-01;
  container box {
    leaf port { type uint16 { range "1..65535"; } }
    leaf name { type string { length "1..3"; } }
    leaf flag { type boolean; }
    leaf color { type enumeration { enum red; enum green; } }
  }
}"#;

    fn compile_value() -> Arc<Library> {
        let mut repo = Repository::new();
        repo.upsert("/v.yang", MOD_VALUE);
        repo.compile().library.expect("library")
    }

    #[test]
    fn flags_invalid_leaf_values() {
        let lib = compile_value();
        let text = r#"<box xmlns="urn:v">
  <port>99999</port>
  <name>toolong</name>
  <flag>yes</flag>
  <color>blue</color>
</box>"#;
        let doc = parse(text).expect("parse");
        let diags = value_diags(&doc, text, &lib);
        let bad: Vec<&InstDiag> = diags
            .iter()
            .filter(|d| d.code == "netconf_bad_value")
            .collect();
        assert_eq!(bad.len(), 4, "{:?}", diags);
        let joined: Vec<&str> = bad.iter().map(|d| d.message.as_str()).collect();
        assert!(
            joined.iter().any(|m| m.contains("out of range")),
            "{joined:?}"
        );
        assert!(joined.iter().any(|m| m.contains("length")), "{joined:?}");
        assert!(joined.iter().any(|m| m.contains("true")), "{joined:?}");
        assert!(joined.iter().any(|m| m.contains("one of")), "{joined:?}");
    }

    #[test]
    fn valid_leaf_values_produce_no_value_diagnostics() {
        let lib = compile_value();
        let text = r#"<box xmlns="urn:v">
  <port>8080</port>
  <name>ok</name>
  <flag>true</flag>
  <color>red</color>
</box>"#;
        let doc = parse(text).expect("parse");
        let diags = value_diags(&doc, text, &lib);
        assert!(
            !diags.iter().any(|d| d.code == "netconf_bad_value"),
            "{:?}",
            diags
        );
    }

    const MOD_LEAFLIST: &str = r#"module ll {
  yang-version 1.1;
  namespace "urn:ll";
  prefix ll;
  revision 2026-01-01;
  container box {
    leaf-list port { type uint16 { range "1..10"; } }
  }
}"#;

    fn compile_leaflist() -> Arc<Library> {
        let mut repo = Repository::new();
        repo.upsert("/ll.yang", MOD_LEAFLIST);
        repo.compile().library.expect("library")
    }

    #[test]
    fn leaf_list_entries_are_value_checked_per_element() {
        let lib = compile_leaflist();
        let text = r#"<box xmlns="urn:ll">
  <port>1</port>
  <port>20</port>
</box>"#;
        let doc = parse(text).expect("parse");
        let diags = value_diags(&doc, text, &lib);
        let bad: Vec<&InstDiag> = diags
            .iter()
            .filter(|d| d.code == "netconf_bad_value")
            .collect();
        assert_eq!(bad.len(), 1, "{:?}", diags);
        assert!(bad[0].message.contains("20"), "{:?}", bad[0]);
    }

    #[test]
    fn identityref_values_checked_semantically() {
        let src = r#"module iv {
  yang-version 1.1;
  namespace "urn:iv";
  prefix iv;
  revision 2026-01-01;
  identity base;
  identity child { base base; }
  identity other;
  container c {
    leaf ref { type identityref { base base; } }
  }
}"#;
        let mut repo = Repository::new();
        repo.upsert("/iv.yang", src);
        let lib = repo.compile().library.expect("library");

        // Derived identity → no value diagnostic.
        let text = r#"<c xmlns="urn:iv"><ref>iv:child</ref></c>"#;
        let doc = parse(text).expect("parse");
        let diags = value_diags(&doc, text, &lib);
        assert!(
            !diags.iter().any(|d| d.code == "netconf_bad_value"),
            "{:?}",
            diags
        );
        // Not derived from `base`.
        let text = r#"<c xmlns="urn:iv"><ref>iv:other</ref></c>"#;
        let doc = parse(text).expect("parse");
        let diags = value_diags(&doc, text, &lib);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("not `base` or derived")),
            "{:?}",
            diags
        );
        // Unknown identity.
        let text = r#"<c xmlns="urn:iv"><ref>iv:nope</ref></c>"#;
        let doc = parse(text).expect("parse");
        let diags = value_diags(&doc, text, &lib);
        assert!(
            diags.iter().any(|d| d.message.contains("unknown identity")),
            "{:?}",
            diags
        );
    }
}
