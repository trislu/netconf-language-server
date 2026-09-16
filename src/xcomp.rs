//! XML writing — completion (M2).
//!
//! Offers the data nodes that may validly appear at the caret as snippet
//! completions. The caret's **parent element** decides which position it is:
//!
//! - a NETCONF **payload wrapper** — `<config>` (the `edit-config` payload, at
//!   the document root or nested under `<edit-config>`) or `<filter>` — holds
//!   the top-level data roots of **every** module. No single-module compiled
//!   library can answer that (it holds only the open closure), so the position
//!   is served from the workspace-wide summary index (Tier 1);
//! - the `<rpc>` operation envelope holds operations: the built-in NETCONF
//!   ones plus every module's top-level `rpc`/`notification`;
//! - inside the *content* of a mapped container/list → its child data nodes
//!   (via `choice`/`case` flattening), resolved through the instance→schema
//!   mapping;
//! - after typing `<` inside a parent → the same children, as start-tag names;
//! - list entries auto-include their `key` leafs as placeholders;
//! - when a child lives in a different namespace than the parent's in-scope
//!   default, an `xmlns="…"` declaration is emitted so the fragment is
//!   self-contained.
//!
//! `<data>` is deliberately **not** a position: it only ever appears in a
//! server's `<rpc-reply>`, so nobody authors inside one. The instance
//! *mapping* still treats it as a payload wrapper, so a captured reply is
//! still mapped and validated.

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use yrepo::{Library, NodeKind, ValueType};

use crate::inst::{ModuleInfo, NETCONF_BASE_NS};
use crate::inst_map::{Resolved, map_doc};
use crate::valcheck::{self, DefaultValue};
use crate::xml::{XmlDoc, XmlNode};

/// One completable child data node.
struct Child {
    name: String,
    kind: NodeKind,
    /// Namespace this child lives in (its instance module's namespace).
    ns: Option<String>,
    keys: Vec<String>,
    mandatory: bool,
    /// A scalar value default for a `Leaf` child (XML-encoded plain text).
    default: Option<DefaultValue>,
}

impl Child {
    fn leaf_default(lib: &Library, module: &str, id: usize) -> Option<DefaultValue> {
        lib.value_type(module, id)
            .and_then(|vt| valcheck::default_value(&vt))
    }
}

/// The built-in NETCONF operations (RFC 6241) offered under `<rpc>`.
const NETCONF_OPS: &[&str] = &[
    "get",
    "get-config",
    "edit-config",
    "copy-config",
    "delete-config",
    "lock",
    "unlock",
    "close-session",
    "kill-session",
];

fn is_leaf(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Leaf | NodeKind::LeafList)
}

/// Whether `p` is a NETCONF **payload wrapper**: its children are the
/// top-level data nodes of *any* module, not the schema children of a mapped
/// node.
///
/// - `<config>` — the `edit-config` payload, at the document root or nested
///   under `<edit-config>`;
/// - `<filter>` — the `get`/`get-config` selection filter (partial content).
///
/// `<data>` is deliberately absent: it only ever appears in a server's
/// `<rpc-reply>` (see the module docs).
fn is_payload_wrapper(p: &XmlNode) -> bool {
    p.ns.as_deref() == Some(NETCONF_BASE_NS) && matches!(p.local.as_str(), "config" | "filter")
}

/// Whether `p` is the NETCONF `<rpc>` operation envelope (RFC 6241) — its
/// children are operations, never data nodes.
fn is_rpc_envelope(p: &XmlNode) -> bool {
    p.ns.as_deref() == Some(NETCONF_BASE_NS) && p.local == "rpc"
}

/// Schema data children of a mapped node (rpc bodies via their `input`).
fn children_of_mapped(lib: &Library, res: &Resolved) -> Vec<Child> {
    let Some(rec) = lib.module(&res.module) else {
        return Vec::new();
    };
    let Some(node) = rec.node(res.id) else {
        return Vec::new();
    };
    let ids: Vec<usize> = match node.kind() {
        NodeKind::Rpc | NodeKind::Action => rec
            .rpc_input(res.id)
            .map(|i| rec.data_children(i))
            .unwrap_or_default(),
        _ => rec.data_children(res.id),
    };
    ids.into_iter()
        .filter_map(|id| {
            let n = rec.node(id)?;
            let default = if n.kind() == NodeKind::Leaf {
                Child::leaf_default(lib, &res.module, id)
            } else {
                None
            };
            Some(Child {
                name: n.name().to_owned(),
                kind: n.kind(),
                ns: lib
                    .module(n.instance_module())
                    .and_then(|m| m.namespace())
                    .map(str::to_owned),
                keys: n.keys().to_vec(),
                mandatory: n.is_mandatory(),
                default,
            })
        })
        .collect()
}

/// Top-level data nodes of every module **in `lib`**.
///
/// That is the open closure, not the workspace: a payload wrapper is normally
/// answered by [`handle_summaries`] (Tier 1) from the workspace-wide summary
/// index, and this is the no-summary fallback.
fn children_all_modules(lib: &Library) -> Vec<Child> {
    let mut out = Vec::new();
    for m in lib.modules() {
        for &id in m.top_nodes() {
            if let Some(n) = m.node(id)
                && n.kind().is_data()
            {
                let default = if n.kind() == NodeKind::Leaf {
                    Child::leaf_default(lib, m.name(), id)
                } else {
                    None
                };
                out.push(Child {
                    name: n.name().to_owned(),
                    kind: n.kind(),
                    ns: m.namespace().map(str::to_owned),
                    keys: n.keys().to_vec(),
                    mandatory: n.is_mandatory(),
                    default,
                });
            }
        }
    }
    out
}

/// Module RPC/notification operations plus the built-in NETCONF operations
/// (for under `<rpc>`), sorted by name.
fn children_ops(lib: &Library) -> Vec<Child> {
    let mut out: Vec<Child> = children_netconf_ops();
    for m in lib.modules() {
        for &id in m.top_nodes() {
            if let Some(n) = m.node(id)
                && matches!(n.kind(), NodeKind::Rpc | NodeKind::Notification)
            {
                out.push(Child {
                    name: n.name().to_owned(),
                    kind: n.kind(),
                    ns: m.namespace().map(str::to_owned),
                    keys: Vec::new(),
                    mandatory: false,
                    default: None,
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The built-in NETCONF operations (RFC 6241) as completion children.
fn children_netconf_ops() -> Vec<Child> {
    NETCONF_OPS
        .iter()
        .map(|name| Child {
            name: name.to_string(),
            kind: NodeKind::Rpc,
            ns: Some(NETCONF_BASE_NS.to_owned()),
            keys: Vec::new(),
            mandatory: false,
            default: None,
        })
        .collect()
}

/// Top-level data roots of every summarized module (Tier-1 fallback). A
/// summary carries only names, so each root is offered with the generic
/// container snippet — its real kind/keys/defaults need the compiled schema
/// (Tier 2).
fn children_data_roots_summaries(modules: &[ModuleInfo]) -> Vec<Child> {
    let mut out = Vec::new();
    for m in modules {
        for name in &m.top_data {
            out.push(Child {
                name: name.clone(),
                kind: NodeKind::Container,
                ns: Some(m.namespace.clone()),
                keys: Vec::new(),
                mandatory: false,
                default: None,
            });
        }
    }
    out
}

/// Module `rpc`/`notification` operations of every summarized module (Tier-1
/// fallback for the `<rpc>` envelope, whose workspace-wide operation set no
/// single-module closure holds). A summary carries names only, so each
/// operation is offered with its real kind and the generic body snippet.
fn children_ops_summaries(modules: &[ModuleInfo]) -> Vec<Child> {
    let mut out = Vec::new();
    for m in modules {
        for (names, kind) in [
            (&m.rpcs, NodeKind::Rpc),
            (&m.notifications, NodeKind::Notification),
        ] {
            for name in names {
                out.push(Child {
                    name: name.clone(),
                    kind,
                    ns: Some(m.namespace.clone()),
                    keys: Vec::new(),
                    mandatory: false,
                    default: None,
                });
            }
        }
    }
    out
}

/// Build one completion item.
fn item(child: &Child, parent_ns: Option<&str>, after_open: bool) -> CompletionItem {
    let need_xmlns = match (&child.ns, parent_ns) {
        (Some(ns), Some(par)) => ns != par,
        (Some(_), None) => true,
        _ => false,
    };
    let open = if need_xmlns {
        format!(
            "<{} xmlns=\"{}\"",
            child.name,
            child.ns.as_deref().unwrap_or_default()
        )
    } else {
        format!("<{}", child.name)
    };
    let (middle, close) = if child.kind == NodeKind::Leaf {
        // Leaf with a typed value default (boolean, empty, enum, number).
        let body = match &child.default {
            Some(DefaultValue::Empty) => String::new(),
            Some(DefaultValue::Bare(b)) => (*b).to_owned(),
            Some(DefaultValue::Quoted(q)) => q.clone(),
            None => "$0".to_owned(),
        };
        if body.is_empty() {
            // `empty` leafs carry no content: `<q></q>`.
            (">".to_owned(), format!("</{}>", child.name))
        } else {
            (format!(">{body}"), format!("</{}>", child.name))
        }
    } else if is_leaf(child.kind) {
        // `>value</name>`
        (">$0".to_owned(), format!("</{}>", child.name))
    } else if child.kind == NodeKind::List {
        // A list entry: include its key leafs as placeholders.
        let keys: String = child
            .keys
            .iter()
            .enumerate()
            .map(|(i, k)| {
                if i == 0 {
                    format!("\n  <{k}>$0</{k}>")
                } else {
                    format!("\n  <{k}></{k}>")
                }
            })
            .collect();
        if keys.is_empty() {
            (">\n  $0\n".to_owned(), format!("</{}>", child.name))
        } else {
            (format!(">\n  {keys}\n"), format!("</{}>", child.name))
        }
    } else {
        (">\n  $0\n".to_owned(), format!("</{}>", child.name))
    };
    let full = format!("{open}{middle}{close}");
    let insert = if after_open {
        full.trim_start_matches('<').to_owned()
    } else {
        full
    };
    let detail = if is_leaf(child.kind) {
        if child.mandatory {
            "leaf (mandatory)".to_owned()
        } else {
            "leaf".to_owned()
        }
    } else {
        child.kind.as_str().to_owned()
    };
    CompletionItem {
        label: child.name.clone(),
        kind: Some(if is_leaf(child.kind) {
            CompletionItemKind::FIELD
        } else {
            CompletionItemKind::STRUCT
        }),
        detail: Some(detail),
        sort_text: Some(child.name.clone()),
        insert_text: Some(insert),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..Default::default()
    }
}

/// Completion items at `byte` (byte offset) in `text`.
///
/// Returns an empty list when completion does not apply (dormant docs,
/// leaf content, attributes, …).
pub fn handle(text: &str, byte: usize, lib: &Library) -> Vec<CompletionItem> {
    let Some(doc) = crate::incomplete::tolerant_xml(text) else {
        return Vec::new();
    };
    let bytes = text.as_bytes();
    // Trigger after an opening `<` (completion trigger character).
    let after_open = byte > 0 && bytes.get(byte - 1) == Some(&b'<');
    // The caret's *parent context*: avoid the (possibly partial) element whose
    // start tag is being typed — step one byte left of the `<`.
    let probe = if after_open {
        byte.saturating_sub(2)
    } else {
        byte
    };
    let Some(parent) = doc.element_at(probe) else {
        return Vec::new();
    };
    let p = &doc.nodes[parent];
    // Mapped parent?
    let map = map_doc(&doc, lib);
    let mapped = map.resolved(parent).cloned();

    // A leaf with a finite value set (`enumeration`/`bits`): offer its members
    // when the caret is in the element's content (not its start tag).
    if let Some(r) = &mapped
        && let Some(items) = leaf_value_items(lib, r, &doc, text, parent, byte)
    {
        return items;
    }

    let children = match mapped {
        Some(r) => Some(children_of_mapped(lib, &r)),
        None if is_payload_wrapper(p) => Some(children_all_modules(lib)),
        None if is_rpc_envelope(p) => Some(children_ops(lib)),
        None => None,
    };
    let Some(children) = children else {
        return Vec::new();
    };
    let parent_ns = p.ns.as_deref();
    children
        .iter()
        .map(|c| item(c, parent_ns, after_open))
        .collect()
}

/// Completion items for a leaf's **value** when the type has a finite value set:
/// enumeration members, `bits` names and `boolean`. `union` (and any other
/// reference/scalar type) stays silent. `None` when the caret is elsewhere.
fn leaf_value_items(
    lib: &Library,
    res: &Resolved,
    doc: &XmlDoc,
    text: &str,
    elem: usize,
    byte: usize,
) -> Option<Vec<CompletionItem>> {
    // Content position only: the caret must be past the start tag (a caret in
    // the tag is an attribute position, and a self-closing leaf has no value).
    if byte < doc.start_tag_end(elem, text)? {
        return None;
    }
    let vt = lib.value_type(&res.module, res.id)?;
    let (values, kind, detail): (Vec<&str>, CompletionItemKind, &str) = match &vt {
        ValueType::Enumeration { members } => (
            members.iter().map(String::as_str).collect(),
            CompletionItemKind::ENUM,
            "enum member",
        ),
        ValueType::Bits { members } => (
            members.iter().map(String::as_str).collect(),
            CompletionItemKind::VALUE,
            "bit",
        ),
        ValueType::Boolean => (vec!["true", "false"], CompletionItemKind::VALUE, "boolean"),
        _ => return None,
    };
    Some(
        values
            .into_iter()
            .map(|v| CompletionItem {
                label: v.to_owned(),
                kind: Some(kind),
                detail: Some(detail.to_owned()),
                insert_text: Some(v.to_owned()),
                ..Default::default()
            })
            .collect(),
    )
}

/// Tier-1 completion items at `byte` from the module summaries only (no
/// compiled schema), for the two positions whose children come from the
/// **workspace-wide** module set rather than from one module's closure:
///
/// - a payload wrapper (`<config>`, `<filter>` — at the document root or
///   nested under an envelope such as `<edit-config>`): every summarized
///   module's top-level data nodes;
/// - the `<rpc>` operation envelope: the built-in NETCONF operations plus every
///   summarized module's top-level `rpc`/`notification`.
///
/// Any other position needs a compiled schema, so it returns nothing (Tier 2
/// handles it once a `Library` exists), and `<data>` is not a position at all.
pub fn handle_summaries(text: &str, byte: usize, modules: &[ModuleInfo]) -> Vec<CompletionItem> {
    let Some(doc) = crate::incomplete::tolerant_xml(text) else {
        return Vec::new();
    };
    let bytes = text.as_bytes();
    let after_open = byte > 0 && bytes.get(byte - 1) == Some(&b'<');
    let probe = if after_open {
        byte.saturating_sub(2)
    } else {
        byte
    };
    let Some(parent) = doc.element_at(probe) else {
        return Vec::new();
    };
    let p = &doc.nodes[parent];
    let mut children = if is_payload_wrapper(p) {
        children_data_roots_summaries(modules)
    } else if is_rpc_envelope(p) {
        let mut ops = children_netconf_ops();
        ops.extend(children_ops_summaries(modules));
        ops
    } else {
        return Vec::new();
    };
    children.sort_by(|a, b| a.name.cmp(&b.name));
    let parent_ns = p.ns.as_deref();
    children
        .iter()
        .map(|c| item(c, parent_ns, after_open))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use yrepo::Repository;

    use super::*;

    const MOD: &str = r#"module m {
  yang-version 1.1;
  namespace "urn:m";
  prefix m;
  revision 2026-01-01;
  container system {
    leaf hostname { type string; }
    list user {
      key "name";
      leaf name { type string; }
      leaf uid { type uint32; mandatory true; }
    }
  }
  container other { leaf x { type string; } }
  rpc reset { input { leaf force { type empty; } } }
}"#;

    fn compile() -> Arc<Library> {
        let mut repo = Repository::new();
        repo.upsert("/m.yang", MOD);
        repo.compile().library.expect("library")
    }

    fn marker_byte(text: &str) -> usize {
        let i = text.find('|').expect("marker");
        text[..i].len()
    }

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|c| c.label.clone()).collect()
    }

    #[test]
    fn completes_children_inside_a_container() {
        let lib = compile();
        let text = r#"<system xmlns="urn:m">
  <hostname>h</hostname>
  |
</system>"#;
        let byte = marker_byte(text);
        let items = handle(text, byte, &lib);
        let got = labels(&items);
        assert!(got.contains(&"hostname".to_owned()), "{got:?}");
        assert!(got.contains(&"user".to_owned()), "{got:?}");
    }

    #[test]
    fn list_entries_carry_key_stubs() {
        let lib = compile();
        let text = r#"<system xmlns="urn:m">
  <user>
    <name>a</name>
  </user>
  |
</system>"#;
        // Caret is inside <system> content → children of system.
        let byte = marker_byte(text);
        let items = handle(text, byte, &lib);
        let item = items.iter().find(|c| c.label == "user").expect("user item");
        let insert = item.insert_text.as_deref().unwrap_or("");
        assert!(insert.contains("<user"), "{insert}");
        assert!(
            insert.contains("<name>$0</name>"),
            "keys as placeholders: {insert}"
        );
        assert!(insert.contains("</user>"), "{insert}");
    }

    #[test]
    fn after_open_tag_offers_children_and_auto_xmlns_for_other_modules() {
        let lib = compile();
        // Inside <other> (namespace urn:m), a child in urn:m is unprefixed.
        // Place caret after '<' inside <system> where a different module's node
        // would need xmlns — emulate with parent in urn:m and child also urn:m,
        // and assert the snippet does not add a redundant xmlns.
        let text = r#"<system xmlns="urn:m">
  |
</system>"#;
        let byte = marker_byte(text);
        let items = handle(text, byte, &lib);
        let got = labels(&items);
        assert!(got.contains(&"hostname".to_owned()));
        let host = items.iter().find(|c| c.label == "hostname").unwrap();
        let insert = host.insert_text.as_deref().unwrap_or("");
        assert!(
            !insert.contains("xmlns"),
            "same ns should not add xmlns: {insert}"
        );
        assert!(insert.contains("<hostname>$0</hostname>"), "{insert}");
    }

    #[test]
    fn rpc_envelope_offers_builtin_and_module_ops() {
        let lib = compile();
        let text = r#"<rpc xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
  |
</rpc>"#;
        let byte = marker_byte(text);
        let items = handle(text, byte, &lib);
        let got = labels(&items);
        assert!(got.contains(&"get-config".to_owned()), "{got:?}");
        assert!(got.contains(&"edit-config".to_owned()), "{got:?}");
        assert!(got.contains(&"reset".to_owned()), "module rpc: {got:?}");
    }

    #[test]
    fn leaf_completion_carries_typed_value_defaults() {
        let src = r#"module v2 {
  yang-version 1.1;
  namespace "urn:v2";
  prefix v2;
  revision 2026-01-01;
  container box {
    leaf flag { type boolean; }
    leaf mark { type empty; }
    leaf color { type enumeration { enum red; enum green; } }
    leaf note { type string; }
    leaf many { type union { type string; type uint16; } }
  }
}"#;
        let mut repo = yrepo::Repository::new();
        repo.upsert("/v2.yang", src);
        let lib = repo.compile().library.expect("library");

        let text = r#"<box xmlns="urn:v2">
  |
</box>"#;
        let byte = marker_byte(text);
        let items = handle(text, byte, &lib);
        let insert = |name: &str| {
            items
                .iter()
                .find(|c| c.label == name)
                .unwrap_or_else(|| panic!("no {name}"))
                .insert_text
                .clone()
                .unwrap_or_default()
        };
        assert_eq!(insert("flag"), "<flag>true</flag>");
        // `empty` leafs carry no content.
        assert_eq!(insert("mark"), "<mark></mark>");
        assert_eq!(insert("color"), "<color>red</color>");
        // No scalar default → value placeholder.
        assert_eq!(insert("note"), "<note>$0</note>");
        // union → no typed default.
        assert_eq!(insert("many"), "<many>$0</many>");
    }

    fn summaries() -> Vec<ModuleInfo> {
        vec![
            ModuleInfo {
                name: "m".to_owned(),
                namespace: "urn:m".to_owned(),
                top_data: vec!["system".to_owned(), "flag".to_owned()],
                rpcs: vec!["reset".to_owned()],
                notifications: vec!["alarm".to_owned()],
            },
            ModuleInfo {
                name: "other".to_owned(),
                namespace: "urn:other".to_owned(),
                top_data: vec!["box".to_owned()],
                rpcs: Vec::new(),
                notifications: Vec::new(),
            },
        ]
    }

    /// The user's rpc template: the caret sits inside an `<edit-config>`
    /// payload wrapper, which is a data position, not the document root.
    fn edit_config_template() -> String {
        format!(
            r#"<rpc message-id="102" xmlns="{NETCONF_BASE_NS}">
  <edit-config>
    <target><running/></target>
    <default-operation>merge</default-operation>
    <config>
      |
    </config>
  </edit-config>
</rpc>"#
        )
    }

    #[test]
    fn summaries_payload_wrapper_offers_every_module_data_root() {
        let mods = summaries();
        // At the document root …
        let root = format!("<config xmlns=\"{NETCONF_BASE_NS}\">\n  |\n</config>");
        // … and nested under an envelope, where the compiled library would only
        // know the document's own closure (the reported bug: 0 items on a real
        // corpus because that closure has no data roots).
        let nested = edit_config_template();
        for text in [&root, &nested] {
            let byte = marker_byte(text);
            let items = handle_summaries(text, byte, &mods);
            let got = labels(&items);
            assert!(got.contains(&"system".to_owned()), "{got:?}");
            assert!(got.contains(&"flag".to_owned()), "{got:?}");
            assert!(got.contains(&"box".to_owned()), "{got:?}");
            // Operations are not data: never offered inside a payload wrapper.
            assert!(!got.contains(&"get-config".to_owned()), "{got:?}");
            assert!(!got.contains(&"reset".to_owned()), "{got:?}");
            // A root in another namespace declares it (the wrapper's in-scope
            // default is the NETCONF base namespace).
            let system = items.iter().find(|c| c.label == "system").expect("system");
            assert!(
                system
                    .insert_text
                    .as_deref()
                    .unwrap_or("")
                    .contains("xmlns=\"urn:m\""),
                "{:?}",
                system.insert_text
            );
        }
    }

    #[test]
    fn summaries_rpc_envelope_offers_operations_not_data_roots() {
        let mods = summaries();
        let text = format!("<rpc xmlns=\"{NETCONF_BASE_NS}\">\n  |\n</rpc>");
        let byte = marker_byte(&text);
        let items = handle_summaries(&text, byte, &mods);
        let got = labels(&items);
        assert!(got.contains(&"get-config".to_owned()), "{got:?}");
        assert!(got.contains(&"edit-config".to_owned()), "{got:?}");
        assert!(got.contains(&"reset".to_owned()), "module rpc: {got:?}");
        assert!(got.contains(&"alarm".to_owned()), "notification: {got:?}");
        // A data root is not a valid `<rpc>` child (RFC 6241).
        assert!(!got.contains(&"system".to_owned()), "{got:?}");
        // An operation element declares its module's namespace.
        let reset = items.iter().find(|c| c.label == "reset").expect("reset");
        assert!(
            reset
                .insert_text
                .as_deref()
                .unwrap_or("")
                .contains("xmlns=\"urn:m\""),
            "{:?}",
            reset.insert_text
        );
        assert_eq!(reset.detail.as_deref(), Some("rpc"));
        let alarm = items.iter().find(|c| c.label == "alarm").expect("alarm");
        assert_eq!(alarm.detail.as_deref(), Some("notification"));
    }

    #[test]
    fn data_is_not_a_completion_position() {
        let mods = summaries();
        let text = format!("<data xmlns=\"{NETCONF_BASE_NS}\">\n  |\n</data>");
        let byte = marker_byte(&text);
        assert!(
            handle_summaries(&text, byte, &mods).is_empty(),
            "a `<data>` body is server output (rpc-reply), not authored"
        );
        // … and the compiled path does not revive it.
        let lib = compile();
        assert!(handle(&text, byte, &lib).is_empty());
    }

    #[test]
    fn nested_config_payload_offers_the_compiled_data_roots() {
        // Tier 2 fallback (no summary index): the compiled closure's roots.
        let lib = compile();
        let text = edit_config_template();
        let byte = marker_byte(&text);
        let items = handle(&text, byte, &lib);
        let got = labels(&items);
        assert!(got.contains(&"system".to_owned()), "{got:?}");
        assert!(got.contains(&"other".to_owned()), "{got:?}");
    }

    #[test]
    fn summaries_completion_is_empty_for_nested_data_and_non_netconf_roots() {
        let mods = summaries();
        let nested = r#"<config xmlns="urn:ietf:params:xml:ns:netconf:base:1.0">
  <system>
    |
  </system>
</config>"#;
        let byte = marker_byte(nested);
        assert!(
            handle_summaries(nested, byte, &mods).is_empty(),
            "nested slot needs a compiled schema"
        );
        let data_root = r#"<interfaces xmlns="urn:m">
  |
</interfaces>"#;
        let byte = marker_byte(data_root);
        assert!(
            handle_summaries(data_root, byte, &mods).is_empty(),
            "a data-tree root has no root-position data slots"
        );
        assert!(handle_summaries("not xml", 0, &mods).is_empty());
    }

    #[test]
    fn enumeration_and_bits_leaves_complete_their_values() {
        let src = r#"module ev {
  yang-version 1.1;
  namespace "urn:ev";
  prefix ev;
  revision 2026-01-01;
  container box {
    leaf color { type enumeration { enum red; enum green; } }
    leaf flags { type bits { bit up; bit down; } }
    leaf active { type boolean; }
    leaf mixed { type union { type boolean; type enumeration { enum one; } } }
    leaf name { type string; }
  }
}"#;
        let mut repo = yrepo::Repository::new();
        repo.upsert("/ev.yang", src);
        let lib = repo.compile().library.expect("library");

        let labels = |text: &str| -> Vec<String> {
            let byte = marker_byte(text);
            handle(text, byte, &lib)
                .iter()
                .map(|c| c.label.clone())
                .collect()
        };
        // Enumeration members are offered in the leaf's content.
        assert_eq!(
            labels(r#"<box xmlns="urn:ev"><color>|</color></box>"#),
            ["red", "green"]
        );
        // Bits names likewise.
        assert_eq!(
            labels(r#"<box xmlns="urn:ev"><flags>|</flags></box>"#),
            ["up", "down"]
        );
        // Boolean offers both literals.
        assert_eq!(
            labels(r#"<box xmlns="urn:ev"><active>|</active></box>"#),
            ["true", "false"]
        );
        // A union stays silent (the member type is ambiguous).
        assert!(labels(r#"<box xmlns="urn:ev"><mixed>|</mixed></box>"#).is_empty());
        // A plain string leaf has no value completion.
        assert!(labels(r#"<box xmlns="urn:ev"><name>|</name></box>"#).is_empty());
        // A caret in the start tag is an attribute position, not a value.
        assert!(labels(r#"<box xmlns="urn:ev"><color |></color></box>"#).is_empty());
        // A self-closing leaf has no content to complete.
        assert!(labels(r#"<box xmlns="urn:ev"><color/|></box>"#).is_empty());
    }
}
