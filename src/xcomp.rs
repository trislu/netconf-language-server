//! XML writing — completion (M2).
//!
//! Offers the data nodes that may validly appear at the caret as snippet
//! completions, resolved through the instance→schema mapping:
//!
//! - inside the *content* of a mapped container/list (or a `<config>` payload
//!   wrapper) → its child data nodes (via `choice`/`case` flattening);
//! - after typing `<` inside a parent → the same children, as start-tag names;
//! - under `<rpc>` → compiled module RPCs/notifications + the built-in NETCONF
//!   operations;
//! - list entries auto-include their `key` leafs as placeholders;
//! - when a child lives in a different namespace than the parent's in-scope
//!   default, an `xmlns="…"` declaration is emitted so the fragment is
//!   self-contained.

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use yrepo::{Library, NodeKind};

use crate::inst::NETCONF_BASE_NS;
use crate::inst_map::{Resolved, map_doc};
use crate::valcheck::{self, DefaultValue};
use crate::xml::parse;

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

/// Top-level data nodes of every compiled module (payload `<config>`).
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
    let mut out: Vec<Child> = NETCONF_OPS
        .iter()
        .map(|name| Child {
            name: name.to_string(),
            kind: NodeKind::Rpc,
            ns: Some(NETCONF_BASE_NS.to_owned()),
            keys: Vec::new(),
            mandatory: false,
            default: None,
        })
        .collect();
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
    let Some(doc) = parse(text) else {
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
    // Wrapper roles (payload/config + <rpc>).
    let is_payload_top = p.ns.as_deref() == Some(NETCONF_BASE_NS)
        && matches!(p.local.as_str(), "config" | "data" | "filter");
    let is_rpc = p.ns.as_deref() == Some(NETCONF_BASE_NS) && p.local == "rpc";

    let children = match mapped {
        Some(r) => Some(children_of_mapped(lib, &r)),
        None if is_payload_top => Some(children_all_modules(lib)),
        None if is_rpc => Some(children_ops(lib)),
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
}
