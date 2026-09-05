//! JSON writing — RFC 7951 member completion (M4).
//!
//! Offers the member names that may validly be typed at the caret as snippet
//! completions, resolved through the JSON instance→schema mapping
//! ([`crate::jmap`]):
//!
//! - at a fresh member slot in the **root object** → every compiled module's
//!   top-level data node, always module-qualified (`module:name`, RFC 7951
//!   §4);
//! - at a fresh member slot inside a mapped **container** (its object value)
//!   or a **list entry** (an entry object of the list's array) → that node's
//!   data children (`choice`/`case` flattened); a child is written bare when it
//!   lives in the parent's module (same instance namespace), otherwise it is
//!   qualified with its own module name (RFC 7951 §4).
//!
//! Snippets mirror the XML writer: containers open `{ … }`, lists open an
//! array with a `{ … }` entry carrying their `key` members, leaf-lists open
//! `[ … ]`, leaves leave the value to the user.

use tower_lsp_server::ls_types::{CompletionItem, CompletionItemKind, InsertTextFormat};
use yrepo::{Library, NodeKind};

use crate::jmap;
use crate::json::{JsonDoc, JsonVal, parse};
use crate::valcheck::{self, DefaultValue};

/// One completable member.
struct Candidate {
    /// The member name as written: `module:name` or `name`.
    qname: String,
    kind: NodeKind,
    keys: Vec<String>,
    mandatory: bool,
    /// A scalar completion default for a `Leaf` member (RFC 7951-encoded).
    default: Option<DefaultValue>,
    /// A checked scalar leaf with no literal default (string/binary/bits):
    /// offer `"…"` and let the user fill the value.
    quoted_placeholder: bool,
}

impl Candidate {
    fn leaf_default(lib: &Library, module: &str, id: usize) -> (Option<DefaultValue>, bool) {
        match lib.value_type(module, id) {
            Some(vt) if vt.is_checked() => match valcheck::default_value(&vt) {
                Some(d) => (Some(d), false),
                None => (None, true),
            },
            _ => (None, false),
        }
    }
}

fn is_leaf(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Leaf | NodeKind::LeafList)
}

/// Every compiled module's top-level data node, module-qualified.
fn top_candidates(lib: &Library) -> Vec<Candidate> {
    let mut out = Vec::new();
    for m in lib.modules() {
        for &id in m.top_nodes() {
            if let Some(n) = m.node(id)
                && n.kind().is_data()
            {
                let (default, quoted_placeholder) = if n.kind() == NodeKind::Leaf {
                    Candidate::leaf_default(lib, m.name(), id)
                } else {
                    (None, false)
                };
                out.push(Candidate {
                    qname: format!("{}:{}", m.name(), n.name()),
                    kind: n.kind(),
                    keys: n.keys().to_vec(),
                    mandatory: n.is_mandatory(),
                    default,
                    quoted_placeholder,
                });
            }
        }
    }
    out.sort_by(|a, b| a.qname.cmp(&b.qname));
    out
}

/// The data children of mapped node `(module, id)` as RFC 7951 members. A
/// child in the same module as `def` (the object's owning module) is written
/// bare; one whose instance module differs is qualified with that module.
fn child_candidates(lib: &Library, module: &str, id: usize, def: &str) -> Vec<Candidate> {
    let Some(rec) = lib.module(module) else {
        return Vec::new();
    };
    let Some(node) = rec.node(id) else {
        return Vec::new();
    };
    if !matches!(node.kind(), NodeKind::Container | NodeKind::List) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for c in rec.data_children(id) {
        let Some(n) = rec.node(c) else {
            continue;
        };
        let inst = n.instance_module();
        let qname = if inst == def {
            n.name().to_owned()
        } else {
            format!("{inst}:{}", n.name())
        };
        let (default, quoted_placeholder) = if n.kind() == NodeKind::Leaf {
            Candidate::leaf_default(lib, module, c)
        } else {
            (None, false)
        };
        out.push(Candidate {
            qname,
            kind: n.kind(),
            keys: n.keys().to_vec(),
            mandatory: n.is_mandatory(),
            default,
            quoted_placeholder,
        });
    }
    out.sort_by(|a, b| a.qname.cmp(&b.qname));
    out
}

/// The value snippet for a candidate's member (`"qname": …`).
fn insert_for(c: &Candidate) -> String {
    match c.kind {
        NodeKind::Container => format!("\"{}\": {{\n  $0\n}}", c.qname),
        NodeKind::List => {
            if c.keys.is_empty() {
                format!("\"{}\": [\n  {{\n    $0\n  }}\n]", c.qname)
            } else {
                let mut inner = String::new();
                for (i, k) in c.keys.iter().enumerate() {
                    if i > 0 {
                        inner.push_str(",\n");
                    }
                    if i == 0 {
                        // First key value is the primary tab stop.
                        inner.push_str(&format!("    \"{k}\": $0"));
                    } else {
                        inner.push_str(&format!("    \"{k}\": "));
                    }
                }
                format!("\"{}\": [\n  {{\n{inner}\n  }}\n]", c.qname)
            }
        }
        NodeKind::LeafList => format!("\"{}\": [\n  $0\n]", c.qname),
        NodeKind::Leaf => {
            let body = match &c.default {
                Some(DefaultValue::Empty) => "[null]".to_owned(),
                Some(DefaultValue::Bare(b)) => (*b).to_owned(),
                Some(DefaultValue::Quoted(q)) => format!("\"{q}\""),
                None if c.quoted_placeholder => "\"$0\"".to_owned(),
                None => String::new(),
            };
            if body.is_empty() {
                format!("\"{}\": ", c.qname)
            } else {
                format!("\"{}\": {body}", c.qname)
            }
        }
        _ => format!("\"{}\": ", c.qname),
    }
}

fn item(c: &Candidate) -> CompletionItem {
    let detail = if is_leaf(c.kind) {
        if c.mandatory {
            "leaf (mandatory)".to_owned()
        } else {
            "leaf".to_owned()
        }
    } else {
        c.kind.as_str().to_owned()
    };
    CompletionItem {
        label: c.qname.clone(),
        kind: Some(if is_leaf(c.kind) {
            CompletionItemKind::FIELD
        } else {
            CompletionItemKind::STRUCT
        }),
        detail: Some(detail),
        sort_text: Some(c.qname.clone()),
        insert_text: Some(insert_for(c)),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..Default::default()
    }
}

/// The object whose braces enclose `byte`, deepest first.
fn enclosing_object(doc: &JsonDoc, byte: usize) -> Option<usize> {
    doc.objects
        .iter()
        .enumerate()
        .filter(|(_, o)| o.range.start <= byte && byte <= o.range.end)
        .max_by_key(|(_, o)| o.range.start)
        .map(|(i, _)| i)
}

/// The member whose value object/array contains `obj`, if any (the root object
/// has none).
fn containing_member(doc: &JsonDoc, obj: usize) -> Option<usize> {
    doc.members.iter().position(|m| match &m.val {
        JsonVal::Object(c) => *c == obj,
        JsonVal::Array(list) => list.contains(&obj),
        JsonVal::Leaf => false,
    })
}

/// Completion items at `byte` (byte offset) in `text`.
///
/// Offered only at a fresh member slot: immediately after `{` or `,`
/// (ignoring whitespace), inside an object. Returns an empty list for dormant
/// documents, leaf content, and member-value positions.
pub fn handle(text: &str, byte: usize, lib: &Library) -> Vec<CompletionItem> {
    let Some(doc) = parse(text) else {
        return Vec::new();
    };
    // A fresh member slot is preceded (ignoring whitespace) by `{` or `,`.
    let bytes = text.as_bytes();
    let mut p = byte;
    while p > 0 && bytes[p - 1].is_ascii_whitespace() {
        p -= 1;
    }
    if p == 0 || (bytes[p - 1] != b'{' && bytes[p - 1] != b',') {
        return Vec::new();
    }
    let Some(obj) = enclosing_object(&doc, byte) else {
        return Vec::new();
    };
    // Root object → top-level data nodes of every compiled module. Only when
    // the document is a netconf payload: an empty root (fresh file) counts, as
    // does a root with at least one recognized member; a dormant document
    // (members matching nothing) stays silent.
    if obj == doc.root {
        let map = jmap::map(&doc, lib);
        let root_obj = &doc.objects[doc.root];
        let recognized = root_obj.members.is_empty()
            || root_obj.members.iter().any(|&m| map.resolved(m).is_some());
        if !recognized {
            return Vec::new();
        }
        return top_candidates(lib).into_iter().map(|c| item(&c)).collect();
    }
    // Nested object → children of the member that owns it (must be mapped).
    let Some(owner) = containing_member(&doc, obj) else {
        return Vec::new();
    };
    let map = jmap::map(&doc, lib);
    let Some(res) = map.resolved(owner).cloned() else {
        return Vec::new();
    };
    let Some(rec) = lib.module(&res.module) else {
        return Vec::new();
    };
    let Some(node) = rec.node(res.id) else {
        return Vec::new();
    };
    let def = node.instance_module();
    child_candidates(lib, &res.module, res.id, def)
        .into_iter()
        .map(|c| item(&c))
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
      leaf uid { type uint32; }
    }
  }
  container other { leaf x { type string; } }
}"#;

    fn compile() -> Arc<Library> {
        let mut repo = Repository::new();
        repo.upsert("/m.yang", MOD);
        repo.compile().library.expect("library")
    }

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

    /// Strip the `|` caret marker, returning the real text and its byte pos.
    fn caret(marked: &str) -> (String, usize) {
        let i = marked.find('|').expect("marker");
        let mut text = marked.to_owned();
        text.remove(i);
        (text, i)
    }

    fn labels(items: &[CompletionItem]) -> Vec<String> {
        items.iter().map(|c| c.label.clone()).collect()
    }

    #[test]
    fn root_object_offers_qualified_top_level_members() {
        let lib = compile();
        let (text, byte) = caret("{\n  |\n}");
        let items = handle(&text, byte, &lib);
        let got = labels(&items);
        assert!(got.contains(&"m:system".to_owned()), "{got:?}");
        assert!(got.contains(&"m:other".to_owned()), "{got:?}");
        let sys = items.iter().find(|c| c.label == "m:system").unwrap();
        assert!(
            sys.insert_text
                .as_deref()
                .unwrap_or("")
                .starts_with("\"m:system\":"),
            "{:?}",
            sys.insert_text
        );
    }

    #[test]
    fn container_body_offers_bare_children_and_list_key_stubs() {
        let lib = compile();
        let (text, byte) = caret("{\n  \"m:system\": {\n    |\n  }\n}");
        let items = handle(&text, byte, &lib);
        let got = labels(&items);
        assert!(got.contains(&"hostname".to_owned()), "{got:?}");
        assert!(got.contains(&"user".to_owned()), "{got:?}");
        // Bare members: no module qualifier inside the owning module.
        assert!(!got.iter().any(|l| l.contains(':')), "{got:?}");
        let user = items.iter().find(|c| c.label == "user").unwrap();
        let insert = user.insert_text.as_deref().unwrap_or("");
        assert!(insert.contains('['), "{insert}");
        assert!(insert.contains("\"name\": $0"), "key stub: {insert}");
    }

    #[test]
    fn list_entry_body_qualifies_cross_module_augments() {
        let lib = compile_demo();
        // Inside an interface entry, example-demo augments the interface list
        // with admin-down → must be offered module-qualified.
        let (text, byte) = caret(
            r#"{
  "example-ietf-interfaces:interfaces": {
    "interface": [
      {
        "name": "eth0",
        |
      }
    ]
  }
}"#,
        );
        let items = handle(&text, byte, &lib);
        let got = labels(&items);
        assert!(
            got.contains(&"example-demo:admin-down".to_owned()),
            "{got:?}"
        );
        assert!(got.contains(&"enabled".to_owned()), "{got:?}");
        let admin = items
            .iter()
            .find(|c| c.label == "example-demo:admin-down")
            .unwrap();
        let insert = admin.insert_text.as_deref().unwrap_or("");
        assert!(
            insert.starts_with("\"example-demo:admin-down\":"),
            "{insert}"
        );
    }

    #[test]
    fn non_member_positions_stay_empty() {
        let lib = compile();
        // Caret after a member value (not after `{`/`,`) → no completion.
        let (text, byte) = caret("{\n  \"m:system\": {\n    \"hostname\": \"h\" |\n  }\n}");
        assert!(handle(&text, byte, &lib).is_empty());
        // Dormant document (no recognized member) stays silent.
        let (text, byte) = caret("{\n  \"zz:thing\": 1, |\n}");
        assert!(handle(&text, byte, &lib).is_empty());
    }

    #[test]
    fn leaf_completion_carries_typed_value_defaults() {
        // A leaf member of each interesting kind inside a mapped container.
        let src = r#"module v {
  yang-version 1.1;
  namespace "urn:v";
  prefix v;
  revision 2026-01-01;
  container box {
    leaf flag { type boolean; }
    leaf mark { type empty; }
    leaf color { type enumeration { enum red; enum green; } }
    leaf note { type string; }
    leaf many { type union { type string; type uint16; } }
  }
}"#;
        let mut repo = Repository::new();
        repo.upsert("/v.yang", src);
        let vlib = repo.compile().library.expect("library");

        let (text, byte) = caret("{\n  \"v:box\": {\n    |\n  }\n}");
        let items = handle(&text, byte, &vlib);
        let insert = |name: &str| {
            items
                .iter()
                .find(|c| c.label == name)
                .unwrap_or_else(|| panic!("no {name}"))
                .insert_text
                .clone()
                .unwrap_or_default()
        };
        assert_eq!(insert("flag"), "\"flag\": true");
        assert_eq!(insert("mark"), "\"mark\": [null]");
        assert_eq!(insert("color"), "\"color\": \"red\"");
        assert_eq!(insert("note"), "\"note\": \"$0\"");
        // union → no typed default (neutral stub).
        assert_eq!(insert("many"), "\"many\": ");
    }
}
