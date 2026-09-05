//! JSON instance-document parsing via `tree-sitter-json` (RFC 7951; M0/M3).
//!
//! - `parse_root` — top-level member keys (M0 sniffing).
//! - `parse` — the **full member tree** with byte ranges and per-member module
//!   qualifiers (M3 goto/hover/diagnostics).
//!
//! Member names are `module:name` (RFC 7951 §4). Arrays of objects represent
//! list entries and keep their object elements as children.

use std::ops::Range;

use tree_sitter::{Node, Parser};
use tree_sitter_json::LANGUAGE;

/// Top level of an RFC 7951 JSON document: its root object member keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonRoot {
    /// Top-level member names exactly as written (may be `module:name`).
    pub top_keys: Vec<String>,
}

// ---------------------------------------------------------------------------
// Full member tree (M3)
// ---------------------------------------------------------------------------

/// A JSON object (root, nested, or a list-entry element of an array).
#[derive(Debug, Clone)]
pub struct JsonObject {
    /// Member indices (document order).
    pub members: Vec<usize>,
    /// Byte range of the whole `{ … }` (completion context lookup).
    pub range: Range<usize>,
}

/// A JSON object member (`"name": value`).
#[derive(Debug, Clone)]
pub struct JsonMember {
    /// Raw key text (may be `module:local`).
    pub name: String,
    /// Module qualifier before the first `:`, when present.
    pub module: Option<String>,
    /// Local part of the name.
    pub local: String,
    /// Byte range of the key (including quotes).
    pub key_range: Range<usize>,
    /// Byte range of the value.
    pub value_range: Range<usize>,
    /// Per-element byte ranges when the value is an array of scalars (a
    /// leaf-list, or `[null]` for an `empty` leaf); `None` otherwise.
    pub scalar_items: Option<Vec<Range<usize>>>,
    /// The value's shape.
    pub val: JsonVal,
}

#[derive(Debug, Clone)]
pub enum JsonVal {
    /// Scalar, or an array of scalars (leaf-list / `[null]` empty).
    Leaf,
    /// A nested object value.
    Object(usize),
    /// An array whose elements are objects (list entries).
    Array(Vec<usize>),
}

/// A parsed RFC 7951 document: an arena of objects + members.
#[derive(Debug, Clone)]
pub struct JsonDoc {
    pub objects: Vec<JsonObject>,
    pub members: Vec<JsonMember>,
    pub root: usize,
}

impl JsonDoc {
    /// The key/member whose `key…value` span contains `byte` (deepest wins).
    pub fn member_at(&self, byte: usize) -> Option<usize> {
        let mut best: Option<usize> = None;
        for (i, m) in self.members.iter().enumerate() {
            let start = m.key_range.start;
            let end = m.value_range.end;
            if start <= byte && byte < end {
                best = Some(match best {
                    None => i,
                    Some(b) if start >= self.members[b].key_range.start => i,
                    Some(b) => b,
                });
            }
        }
        best
    }
}

fn children<'t>(n: Node<'t>) -> Vec<Node<'t>> {
    let mut out = Vec::new();
    for i in 0..n.child_count() {
        if let Some(c) = n.child(i as u32) {
            out.push(c);
        }
    }
    out
}

fn key_of(pair: Node<'_>) -> Option<Node<'_>> {
    pair.child_by_field_name("key")
}

fn value_of(pair: Node<'_>) -> Option<Node<'_>> {
    pair.child_by_field_name("value")
}

fn split_name(raw: &str) -> (Option<String>, String) {
    match raw.split_once(':') {
        Some((m, l)) => (Some(m.to_owned()), l.to_owned()),
        None => (None, raw.to_owned()),
    }
}

/// Build a `JsonObject` for a tree-sitter `<object>` node, returning its index.
fn build_object(
    obj_node: Node<'_>,
    text: &str,
    objects: &mut Vec<JsonObject>,
    members: &mut Vec<JsonMember>,
) -> Option<usize> {
    let idx = objects.len();
    objects.push(JsonObject {
        members: Vec::new(),
        range: obj_node.byte_range(),
    });
    for pair in children(obj_node)
        .into_iter()
        .filter(|c| c.kind() == "pair")
    {
        let key_node = key_of(pair)?;
        let val_node = value_of(pair)?;
        let raw = key_node.utf8_text(text.as_bytes()).ok()?;
        let name = raw
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(raw)
            .to_owned();
        let (module, local) = split_name(&name);

        // Classify the value: nested object, array-of-objects (a list's
        // entries), a scalar array (a leaf-list, or `[null]` for an empty
        // leaf), or a single leaf scalar.
        let mut val = JsonVal::Leaf;
        let mut scalar_items: Option<Vec<Range<usize>>> = None;
        match val_node.kind() {
            "object" => {
                let child = build_object(val_node, text, objects, members)?;
                val = JsonVal::Object(child);
            }
            "array" => {
                let kids = children(val_node);
                let objs: Vec<usize> = kids
                    .iter()
                    .filter(|c| c.kind() == "object")
                    .filter_map(|o| build_object(*o, text, objects, members))
                    .collect();
                if !objs.is_empty() {
                    val = JsonVal::Array(objs);
                } else {
                    // No object entries → a leaf-list (or `[null]` empty leaf):
                    // keep each scalar element's span for per-value checks.
                    let items: Vec<Range<usize>> = kids
                        .iter()
                        .filter(|c| {
                            matches!(c.kind(), "string" | "number" | "true" | "false" | "null")
                        })
                        .map(|c| c.byte_range())
                        .collect();
                    if !items.is_empty() {
                        scalar_items = Some(items);
                    }
                }
            }
            _ => {}
        }

        let mem = JsonMember {
            name,
            module,
            local,
            key_range: key_node.byte_range(),
            value_range: val_node.byte_range(),
            val,
            scalar_items,
        };
        members.push(mem);
        let last = members.len() - 1;
        objects[idx].members.push(last);
    }
    Some(idx)
}

/// Parse a JSON document into its full member tree.
///
/// Returns `None` when the text does not parse, or the root is not an object.
pub fn parse(text: &str) -> Option<JsonDoc> {
    let mut parser = Parser::new();
    parser.set_language(&LANGUAGE.into()).ok()?;
    let tree = parser.parse(text, None)?;
    let doc = tree.root_node();
    let root_obj = children(doc).into_iter().find(|c| c.kind() == "object")?;
    let mut objects = Vec::new();
    let mut members = Vec::new();
    let root = build_object(root_obj, text, &mut objects, &mut members)?;
    Some(JsonDoc {
        objects,
        members,
        root,
    })
}

/// Parse a JSON document and return the root object's member keys (M0).
pub fn parse_root(text: &str) -> Option<JsonRoot> {
    let doc = parse(text)?;
    let root_keys = doc
        .objects
        .get(doc.root)
        .map(|o| o.members.clone())
        .unwrap_or_default()
        .into_iter()
        .map(|m| doc.members[m].name.clone())
        .collect();
    Some(JsonRoot {
        top_keys: root_keys,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc7951_style_top_level_keys() {
        let root = parse_root(
            r#"{
  "ietf-interfaces:interfaces": {
    "interface": [ { "name": "eth0", "enabled": true } ]
  },
  "example-vlan:vlan-tagging": true
}"#,
        )
        .expect("parses");
        assert_eq!(
            root.top_keys,
            vec![
                "ietf-interfaces:interfaces".to_owned(),
                "example-vlan:vlan-tagging".to_owned()
            ]
        );
    }

    #[test]
    fn non_object_or_invalid_returns_none() {
        assert!(parse_root("[1, 2, 3]").is_none());
        assert!(parse_root("").is_none());
        assert!(parse_root("{ not json").is_none());
    }

    #[test]
    fn full_tree_members_have_qualifiers_and_ranges() {
        let doc = parse(
            r#"{
  "ietf-interfaces:interfaces": {
    "interface": [
      { "name": "eth0", "enabled": true }
    ]
  }
}"#,
        )
        .expect("parses");
        // Top-level member + 3 members in the list entry object.
        let n = doc.members.len();
        assert_eq!(n, 4, "members: {doc:?}");
        let top = doc.objects[doc.root].members[0];
        assert_eq!(doc.members[top].module.as_deref(), Some("ietf-interfaces"));
        assert_eq!(doc.members[top].local, "interfaces");
        // The list-entry object members resolve local names & caret hits.
        let name = doc.members.iter().find(|m| m.local == "name").unwrap();
        assert_eq!(name.module, None);
        assert!(name.key_range.start < name.value_range.start);
        let byte = name.key_range.start + 2; // inside key
        assert_eq!(
            doc.member_at(byte),
            Some(doc.members.iter().position(|m| m.local == "name").unwrap())
        );
    }
}
