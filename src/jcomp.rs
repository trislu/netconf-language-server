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
//!
//! It also completes the **value** of an `identityref` leaf (RFC 7951 §6.8:
//! the value is `"module-name:identity-name"`), from the same workspace-wide
//! identity graph the XML side uses.

use ropey::Rope;
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, Documentation, InsertTextFormat,
    TextEdit,
};
use yrepo::{Library, NodeKind, ValueType};

use crate::convert;
use crate::idgraph::{IdentityGraph, resolve_base};
use crate::inst::ModuleInfo;
use crate::jmap;
use crate::json::{JsonDoc, JsonVal};
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

/// The deepest member whose **value** span contains `byte`, inclusive of the
/// value end so a caret right after an unquoted scalar (a boolean or number
/// being typed) counts as inside it. `None` for a caret in a key or outside
/// every value.
fn member_at_value(doc: &JsonDoc, byte: usize) -> Option<usize> {
    doc.members
        .iter()
        .enumerate()
        .filter(|(_, m)| m.value_range.start <= byte && byte <= m.value_range.end)
        .max_by_key(|(_, m)| m.value_range.start)
        .map(|(i, _)| i)
}

/// Completion items for a leaf's value when the type has a finite value set:
/// enumeration members and `bits` names (RFC 7951 encodes both as strings) and
/// `boolean` (a JSON boolean literal). `union` stays silent. `None` when the
/// caret is not inside the value of such a leaf.
///
/// When the value has not been typed yet (a fresh slot after `:`/`[`) the
/// members are inserted **quoted**; inside an existing string they replace the
/// partial token bare.
fn leaf_value_items(
    doc: &JsonDoc,
    text: &str,
    byte: usize,
    lib: &Library,
) -> Option<Vec<CompletionItem>> {
    let member = member_at_value(doc, byte)?;
    let map = jmap::map(doc, lib);
    let res = map.resolved(member)?;
    let vt = lib.value_type(&res.module, res.id)?;
    let (values, kind, detail, is_string): (Vec<&str>, CompletionItemKind, &str, bool) = match &vt {
        ValueType::Enumeration { members } => (
            members.iter().map(String::as_str).collect(),
            CompletionItemKind::ENUM,
            "enum member",
            true,
        ),
        ValueType::Bits { members } => (
            members.iter().map(String::as_str).collect(),
            CompletionItemKind::VALUE,
            "bit",
            true,
        ),
        ValueType::Boolean => (
            vec!["true", "false"],
            CompletionItemKind::VALUE,
            "boolean",
            false,
        ),
        _ => return None,
    };
    // Inside an existing `"…"` the token start follows the opening quote.
    let token_start = crate::idcomp::word_start(text, byte);
    let in_string = text[..token_start].ends_with('"');
    Some(
        values
            .into_iter()
            .map(|v| CompletionItem {
                label: v.to_owned(),
                kind: Some(kind),
                detail: Some(detail.to_owned()),
                insert_text: Some(if is_string && !in_string {
                    format!("\"{v}\"")
                } else {
                    v.to_owned()
                }),
                ..Default::default()
            })
            .collect(),
    )
}

/// Completion items at `byte` (byte offset) in `text`.
///
/// Offered only at a fresh member slot: immediately after `{` or `,`
/// (ignoring whitespace), inside an object. Returns an empty list for dormant
/// documents, leaf content, and member-value positions.
pub fn handle(text: &str, byte: usize, lib: &Library) -> Vec<CompletionItem> {
    let Some(doc) = crate::incomplete::tolerant_json_at(text, byte) else {
        return Vec::new();
    };
    // A leaf with a finite value set (`enumeration`/`bits`): complete its value
    // before the member-slot logic (a value position is not a member slot).
    if let Some(items) = leaf_value_items(&doc, text, byte, lib) {
        return items;
    }
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

/// Every summarized module's top-level data node, module-qualified (Tier-1
/// fallback). A summary carries only names, so each root is offered with the
/// generic container snippet — its real kind needs the compiled schema.
fn top_candidates_summaries(modules: &[ModuleInfo]) -> Vec<Candidate> {
    let mut out = Vec::new();
    for m in modules {
        for name in &m.top_data {
            out.push(Candidate {
                qname: format!("{}:{}", m.name, name),
                kind: NodeKind::Container,
                keys: Vec::new(),
                mandatory: false,
                default: None,
                quoted_placeholder: false,
            });
        }
    }
    out.sort_by(|a, b| a.qname.cmp(&b.qname));
    out
}

/// Completion items for the **value** of an `identityref` leaf at `byte`.
///
/// An RFC 7951 identityref value is `"module-name:identity-name"` (§6.8), so no
/// prefix declaration is involved. Each item replaces the partial value token
/// inside the string, or fills a fresh value slot (with the quotes), with
/// `module:identity`. Empty unless the caret is at the value of an
/// `identityref` leaf.
pub fn identity_completion(
    rope: &Rope,
    byte: usize,
    lib: &Library,
    graph: &IdentityGraph,
) -> Vec<CompletionItem> {
    let text = rope.to_string();
    let Some(doc) = crate::incomplete::tolerant_json_at(&text, byte) else {
        return Vec::new();
    };
    let Some(member) = member_at_value(&doc, byte) else {
        return Vec::new();
    };
    let map = jmap::map(&doc, lib);
    let Some(res) = map.resolved(member) else {
        return Vec::new();
    };
    let Some(ValueType::Identityref { base }) = lib.value_type(&res.module, res.id) else {
        return Vec::new();
    };
    let Some((base_module, base_name)) = base
        .as_deref()
        .and_then(|b| resolve_base(lib, &res.module, b))
    else {
        return Vec::new();
    };
    let candidates = graph.derived_from(&base_module, &base_name);
    if candidates.is_empty() {
        return Vec::new();
    }
    let token_start = crate::idcomp::word_start(&text, byte);
    let range = convert::range_to_lsp(rope, token_start..byte);
    // At a fresh value slot (no opening quote yet) the item must supply the
    // quotes; inside an existing `"…"` it replaces the token bare.
    let in_string = text[..token_start].ends_with('"');
    candidates
        .into_iter()
        .map(|c| {
            let label = format!("{}:{}", c.module, c.name);
            CompletionItem {
                label: label.clone(),
                kind: Some(CompletionItemKind::ENUM),
                detail: Some(format!("identity (module {})", c.module)),
                documentation: c
                    .namespace
                    .map(|ns| Documentation::String(format!("namespace `{ns}`"))),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: if in_string {
                        label
                    } else {
                        format!("\"{label}\"")
                    },
                })),
                ..Default::default()
            }
        })
        .collect()
}

/// Tier-1 completion items at `byte` from the module summaries only (no
/// compiled schema).
///
/// Offered at a fresh member slot of the **root object** (an empty root
/// counts, as does one whose members match a summarized data root; a dormant
/// root stays silent): every summarized module's top-level data node,
/// module-qualified. A nested object needs a compiled schema, so it returns
/// nothing (Tier 2 handles it once a `Library` exists).
pub fn handle_summaries(text: &str, byte: usize, modules: &[ModuleInfo]) -> Vec<CompletionItem> {
    let Some(doc) = crate::incomplete::tolerant_json(text) else {
        return Vec::new();
    };
    // Two in-progress root positions: a fresh member slot (preceded by `{` or
    // `,`) or a member name just started (preceded by the opening `"` the
    // editor inserted when the user typed it).
    let bytes = text.as_bytes();
    let mut p = byte;
    while p > 0 && bytes[p - 1].is_ascii_whitespace() {
        p -= 1;
    }
    let in_name = p > 0 && bytes[p - 1] == b'"';
    let slot = p > 0 && (bytes[p - 1] == b'{' || bytes[p - 1] == b',');
    if !slot && !in_name {
        return Vec::new();
    }
    let Some(obj) = enclosing_object(&doc, byte) else {
        return Vec::new();
    };
    if obj != doc.root {
        return Vec::new();
    }
    let root_obj = &doc.objects[doc.root];
    let recognized = root_obj.members.is_empty()
        || root_obj.members.iter().any(|&m| {
            let mem = &doc.members[m];
            let Some(module) = mem.module.as_deref() else {
                return false;
            };
            modules
                .iter()
                .any(|s| s.name == module && s.top_data.iter().any(|t| t == &mem.local))
        });
    if !recognized {
        return Vec::new();
    }
    top_candidates_summaries(modules)
        .into_iter()
        .map(|c| {
            let mut it = item(&c);
            if in_name {
                // The opening quote is already in the document; complete just
                // the qualified member name.
                it.insert_text = Some(c.qname.clone());
                it.insert_text_format = Some(InsertTextFormat::PLAIN_TEXT);
            }
            it
        })
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

    fn summaries() -> Vec<ModuleInfo> {
        vec![
            ModuleInfo {
                name: "m".to_owned(),
                namespace: "urn:m".to_owned(),
                top_data: vec!["system".to_owned(), "flag".to_owned()],
                rpcs: Vec::new(),
                notifications: Vec::new(),
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

    #[test]
    fn summaries_root_object_offers_qualified_data_roots() {
        let mods = summaries();
        // A fresh (empty) root object offers every summarized data root.
        let (text, byte) = caret("{\n  |\n}");
        let items = handle_summaries(&text, byte, &mods);
        let got = labels(&items);
        assert!(got.contains(&"m:system".to_owned()), "{got:?}");
        assert!(got.contains(&"m:flag".to_owned()), "{got:?}");
        assert!(got.contains(&"other:box".to_owned()), "{got:?}");
        let sys = items.iter().find(|c| c.label == "m:system").unwrap();
        assert!(
            sys.insert_text
                .as_deref()
                .unwrap_or("")
                .starts_with("\"m:system\":"),
            "{:?}",
            sys.insert_text
        );
        // A root with a recognized member counts too (continuing a document).
        let (text, byte) = caret("{\n  \"m:system\": {},\n  |\n}");
        assert!(labels(&handle_summaries(&text, byte, &mods)).contains(&"m:system".to_owned()));
    }

    #[test]
    fn summaries_nested_and_dormant_positions_stay_empty() {
        let mods = summaries();
        // Nested object needs a compiled schema.
        let (text, byte) = caret("{\n  \"m:system\": {\n    |\n  }\n}");
        assert!(handle_summaries(&text, byte, &mods).is_empty());
        // A root whose members match no summary stays dormant.
        let (text, byte) = caret("{\n  \"zz:thing\": 1,\n  |\n}");
        assert!(handle_summaries(&text, byte, &mods).is_empty());
        // Not a fresh member slot.
        let (text, byte) = caret("{\n  \"m:system\": {} |\n}");
        assert!(handle_summaries(&text, byte, &mods).is_empty());
    }

    const IDA: &str = r#"module ida {
  yang-version 1.1;
  namespace "urn:ida";
  prefix a;
  revision 2026-01-01;
  identity root;
  container box { leaf id { type identityref { base root; } } }
}"#;
    const IDB: &str = r#"module idb {
  yang-version 1.1;
  namespace "urn:idb";
  prefix b;
  revision 2026-01-01;
  import ida { prefix a; }
  identity child { base a:root; }
}"#;

    fn identity_setup() -> (Arc<Library>, IdentityGraph) {
        let dir = std::env::temp_dir().join(format!(
            "ncls-jcomp-id-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("ida.yang");
        let b = dir.join("idb.yang");
        std::fs::write(&a, IDA).unwrap();
        std::fs::write(&b, IDB).unwrap();
        let mut repo = Repository::new();
        repo.upsert(a.to_string_lossy().to_string(), IDA);
        repo.upsert(b.to_string_lossy().to_string(), IDB);
        let lib = repo.compile().library.expect("library");
        let mut index = yrepo::SummaryIndex::default();
        index.scan_many_files_with([&a, &b], |p| Some(p.to_string_lossy().to_string()));
        let graph = IdentityGraph::build(&index);
        std::fs::remove_dir_all(&dir).ok();
        (lib, graph)
    }

    #[test]
    fn identityref_values_complete_module_qualified_identities() {
        let (lib, graph) = identity_setup();
        let (text, byte) = caret(r#"{ "ida:box": { "id": "|" } }"#);
        let rope = Rope::from_str(&text);
        let items = identity_completion(&rope, byte, &lib, &graph);
        let mut got = labels(&items);
        got.sort();
        // The base itself plus the identity derived in the other module.
        assert_eq!(got, vec!["ida:root".to_owned(), "idb:child".to_owned()]);
        // The value is inserted bare (inside the existing quotes).
        let idb = items.iter().find(|i| i.label == "idb:child").unwrap();
        let Some(CompletionTextEdit::Edit(edit)) = &idb.text_edit else {
            panic!("text edit");
        };
        assert_eq!(edit.new_text, "idb:child");

        // A fresh value slot (`"id": `) is repaired, and the item supplies the
        // quotes the JSON string needs.
        let (text, byte) = caret(r#"{ "ida:box": { "id": | } }"#);
        let rope = Rope::from_str(&text);
        let items = identity_completion(&rope, byte, &lib, &graph);
        let idb = items.iter().find(|i| i.label == "idb:child").unwrap();
        let Some(CompletionTextEdit::Edit(edit)) = &idb.text_edit else {
            panic!("text edit");
        };
        assert_eq!(edit.new_text, "\"idb:child\"");
    }

    #[test]
    fn identity_value_completion_replaces_a_partial_token() {
        let (lib, graph) = identity_setup();
        let (text, byte) = caret(r#"{ "ida:box": { "id": "idb:|" } }"#);
        let rope = Rope::from_str(&text);
        let items = identity_completion(&rope, byte, &lib, &graph);
        let idb = items.iter().find(|i| i.label == "idb:child").unwrap();
        let Some(CompletionTextEdit::Edit(edit)) = &idb.text_edit else {
            panic!("text edit");
        };
        // The edit range covers the whole typed `idb:` token.
        assert_eq!(edit.range.end.character - edit.range.start.character, 4);
    }

    #[test]
    fn enumeration_leaves_complete_their_values() {
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
        let mut repo = Repository::new();
        repo.upsert("/ev.yang", src);
        let lib = repo.compile().library.expect("library");

        let labels = |marked: &str| -> Vec<String> {
            let (text, byte) = caret(marked);
            handle(&text, byte, &lib)
                .iter()
                .map(|c| c.label.clone())
                .collect()
        };
        assert_eq!(
            labels(r#"{ "ev:box": { "color": "|" } }"#),
            ["red", "green"]
        );
        assert_eq!(labels(r#"{ "ev:box": { "flags": "|" } }"#), ["up", "down"]);
        // A boolean value is a JSON literal; the caret is inside it.
        assert_eq!(
            labels(r#"{ "ev:box": { "active": tr|ue } }"#),
            ["true", "false"]
        );
        // A union stays silent.
        assert!(labels(r#"{ "ev:box": { "mixed": "on|e" } }"#).is_empty());
        // A plain string leaf has no value completion, and a member slot still
        // offers member names (not values).
        assert!(labels(r#"{ "ev:box": { "name": "|" } }"#).is_empty());
        assert!(labels(r#"{ "ev:box": { | } }"#).contains(&"name".to_owned()));

        // A value slot with nothing typed yet (`"color": `) is repaired for
        // completion, and string values are inserted quoted.
        let (text, byte) = caret(r#"{ "ev:box": { "color": | } }"#);
        let items = handle(&text, byte, &lib);
        let insert = |label: &str| {
            items
                .iter()
                .find(|i| i.label == label)
                .unwrap_or_else(|| panic!("no {label}"))
                .insert_text
                .clone()
                .unwrap_or_default()
        };
        assert_eq!(insert("red"), "\"red\"");
        // A `bits` array slot is quoted too (the array already parses).
        let (text, byte) = caret(r#"{ "ev:box": { "flags": [ | ] } }"#);
        let items = handle(&text, byte, &lib);
        assert_eq!(
            items
                .iter()
                .find(|i| i.label == "up")
                .unwrap()
                .insert_text
                .as_deref(),
            Some("\"up\"")
        );
        // Boolean stays a bare literal.
        let (text, byte) = caret(r#"{ "ev:box": { "active": | } }"#);
        let items = handle(&text, byte, &lib);
        assert_eq!(
            items
                .iter()
                .find(|i| i.label == "true")
                .unwrap()
                .insert_text
                .as_deref(),
            Some("true")
        );
    }
}
