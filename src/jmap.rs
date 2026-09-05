//! Instance → schema mapping for JSON (RFC 7951) documents (M3).
//!
//! RFC 7951 §4: member names are `module:name` where the qualifier is the
//! **module name** that owns the node's instance namespace (D30). Top-level
//! members are always qualified; nested members are qualified iff their
//! namespace differs from the parent's (bare = same module as the parent
//! member).

use std::collections::HashSet;
use std::ops::Range;

use yrepo::{Library, NodeKind, SchemaNode, ValueType};

use crate::depth;
use crate::inst_map::{InstDiag, Resolved};
use crate::json::{JsonDoc, JsonVal};

/// Result of mapping all members of a JSON document.
#[derive(Debug, Clone)]
pub struct JMap {
    res: Vec<Option<Resolved>>,
    pub diags: Vec<InstDiag>,
}

impl JMap {
    /// The schema node a member maps to, if it is mapped.
    pub fn resolved(&self, member: usize) -> Option<&Resolved> {
        self.res.get(member).and_then(Option::as_ref)
    }
}

#[derive(Clone)]
enum Ctx {
    /// Root object — members select top-level data nodes of compiled modules.
    Top,
    /// Inside a mapped node: members are its data children. `def` is the
    /// parent member's module name (bare members live in that module).
    Node {
        module: String,
        id: usize,
        def: String,
    },
}

fn instance_module_name(node: &SchemaNode) -> String {
    node.instance_module().to_owned()
}

/// The schema child of `rec.node(id)` named `local` whose instance module is
/// `module`, if any (children via `data_children`, i.e. through choice/case).
fn find_child(
    lib: &Library,
    module: &str,
    id: usize,
    local: &str,
    qualifier: &str,
) -> Option<usize> {
    let rec = lib.module(module)?;
    for c in rec.data_children(id) {
        let n = rec.node(c)?;
        if n.name() == local && instance_module_name(n) == qualifier {
            return Some(c);
        }
    }
    None
}

/// Whether any candidate child named `local` exists (to tell “wrong module”
/// from “unknown member”).
fn name_exists(lib: &Library, module: &str, id: usize, local: &str) -> bool {
    lib.module(module)
        .map(|rec| {
            rec.data_children(id)
                .iter()
                .any(|&c| rec.node(c).is_some_and(|n| n.name() == local))
        })
        .unwrap_or(false)
}

fn resolve_member(
    lib: &Library,
    local: &str,
    qualifier: &Option<String>,
    ctx: &Ctx,
    range: &Range<usize>,
    diags: &mut Vec<InstDiag>,
) -> Option<Resolved> {
    match ctx {
        Ctx::Top => {
            let Some(module) = qualifier else {
                diags.push(InstDiag {
                    range: range.clone(),
                    message: "top-level member must be module-qualified (RFC 7951 §4)".to_owned(),
                    code: "json_unknown_member",
                });
                return None;
            };
            let rec = lib.module(module)?;
            for &id in rec.top_nodes() {
                if let Some(n) = rec.node(id)
                    && n.kind().is_data()
                    && n.name() == local
                {
                    return Some(Resolved {
                        module: module.clone(),
                        id,
                    });
                }
            }
            diags.push(InstDiag {
                range: range.clone(),
                message: format!(
                    "unknown member `{local}` (module `{module}` has no top-level data node)"
                ),
                code: "json_unknown_member",
            });
            None
        }
        Ctx::Node { module, id, def } => {
            let expected = qualifier.as_deref().unwrap_or(def);
            if let Some(child) = find_child(lib, module, *id, local, expected) {
                return Some(Resolved {
                    module: module.clone(),
                    id: child,
                });
            }
            if name_exists(lib, module, *id, local) {
                diags.push(InstDiag {
                    range: range.clone(),
                    message: format!(
                        "member `{local}` is not in module `{expected}` here (it belongs to \
                         another module's namespace)"
                    ),
                    code: "json_wrong_module",
                });
            } else {
                diags.push(InstDiag {
                    range: range.clone(),
                    message: format!("unknown member `{local}` (not a child of this node)"),
                    code: "json_unknown_member",
                });
            }
            None
        }
    }
}

fn walk(
    doc: &JsonDoc,
    lib: &Library,
    obj: usize,
    ctx: Ctx,
    res: &mut Vec<Option<Resolved>>,
    diags: &mut Vec<InstDiag>,
) {
    let Some(object) = doc.objects.get(obj) else {
        return;
    };
    let members = object.members.clone();
    for m in members {
        let Some(member) = doc.members.get(m) else {
            continue;
        };
        let mapped = resolve_member(
            lib,
            &member.local,
            &member.module,
            &ctx,
            &member.key_range,
            diags,
        );
        res[m] = mapped.clone();
        // Recurse into the value only when this member resolved (otherwise
        // avoid cascading diagnostics inside an unknown subtree).
        let Some(resolved) = mapped else {
            continue;
        };
        let child_ctx = Ctx::Node {
            module: resolved.module.clone(),
            id: resolved.id,
            def: lib
                .module(&resolved.module)
                .and_then(|rec| rec.node(resolved.id))
                .map(instance_module_name)
                .unwrap_or_else(|| resolved.module.clone()),
        };
        match &member.val {
            JsonVal::Object(c) => walk(doc, lib, *c, child_ctx, res, diags),
            JsonVal::Array(list) => {
                for c in list {
                    walk(doc, lib, *c, child_ctx.clone(), res, diags);
                }
            }
            JsonVal::Leaf => {}
        }
    }
}

/// M4 diagnostics depth: for every present container/list member, report the
/// required members missing from its value object (for a list: from each entry
/// object) — mandatory nodes, list keys, and `choice`s with no instantiated
/// case (see [`crate::depth`]).
fn depth_pass(doc: &JsonDoc, lib: &Library, res: &[Option<Resolved>], diags: &mut Vec<InstDiag>) {
    let mut seen: HashSet<(usize, String)> = HashSet::new();
    for (m, member) in doc.members.iter().enumerate() {
        let Some(Some(resolved)) = res.get(m) else {
            continue;
        };
        let Some(rec) = lib.module(&resolved.module) else {
            continue;
        };
        let Some(node) = rec.node(resolved.id) else {
            continue;
        };
        match (node.kind(), &member.val) {
            (NodeKind::Container, JsonVal::Object(o)) => push_depth(
                doc,
                lib,
                res,
                &resolved.module,
                resolved.id,
                *o,
                member.key_range.clone(),
                diags,
                &mut seen,
            ),
            (NodeKind::List, JsonVal::Array(entries)) => {
                for &e in entries {
                    // Anchor on the entry's first member when it has one, else
                    // on the list member's key.
                    let anchor = doc
                        .objects
                        .get(e)
                        .and_then(|o| o.members.first().copied())
                        .and_then(|k| doc.members.get(k))
                        .map(|k| k.key_range.clone())
                        .unwrap_or_else(|| member.key_range.clone());
                    push_depth(
                        doc,
                        lib,
                        res,
                        &resolved.module,
                        resolved.id,
                        e,
                        anchor,
                        diags,
                        &mut seen,
                    );
                }
            }
            _ => {}
        }
    }
}

/// Validate one object (a container's value or a list entry) against schema
/// node `(module, id)` and push any missing-member/empty-choice diagnostics,
/// anchored at `anchor`, deduplicated by `(anchor.start, message)`.
#[allow(clippy::too_many_arguments)]
fn push_depth(
    doc: &JsonDoc,
    lib: &Library,
    res: &[Option<Resolved>],
    module: &str,
    id: usize,
    obj: usize,
    anchor: Range<usize>,
    diags: &mut Vec<InstDiag>,
    seen: &mut HashSet<(usize, String)>,
) {
    let Some(object) = doc.objects.get(obj) else {
        return;
    };
    let present: Vec<usize> = object
        .members
        .iter()
        .filter_map(|&k| res.get(k).and_then(Option::as_ref))
        .filter(|r| r.module == module)
        .map(|r| r.id)
        .collect();
    let report = depth::analyze(lib, module, id, &|s| present.contains(&s));
    let start = anchor.start;
    let mut push = |message: String, code: &'static str| {
        if seen.insert((start, message.clone())) {
            diags.push(InstDiag {
                range: anchor.clone(),
                message,
                code,
            });
        }
    };
    if !report.missing.is_empty() {
        let names = report.missing.join(", ");
        let word = if report.missing.len() == 1 {
            "member"
        } else {
            "members"
        };
        push(
            format!("missing required {word}: {names}"),
            "json_missing_member",
        );
    }
    for gap in &report.choices {
        push(
            format!(
                "no case of choice `{}` is present (expected one of: {})",
                gap.name,
                gap.options.join(", ")
            ),
            "json_missing_choice",
        );
    }
}

/// Map every member of `doc` to a schema node (best effort) and collect
/// diagnostics. Documents that match no compiled module stay dormant (no
/// diagnostics).
pub fn map(doc: &JsonDoc, lib: &Library) -> JMap {
    let n = doc.members.len();
    let mut res: Vec<Option<Resolved>> = vec![None; n];
    let mut diags = Vec::new();
    walk(doc, lib, doc.root, Ctx::Top, &mut res, &mut diags);
    depth_pass(doc, lib, &res, &mut diags);
    let recognized = doc
        .objects
        .get(doc.root)
        .map(|o| o.members.iter().any(|&m| res[m].is_some()))
        .unwrap_or(false);
    if !recognized {
        diags.clear();
    }
    JMap { res, diags }
}

/// M5 leaf *value* validation (D31): for every mapped **leaf**/**leaf-list**
/// whose type reduces to a checked scalar, decode the member's RFC 7951 value
/// and evaluate it. A `leaf-list` is checked per scalar element (its array
/// element spans are captured by the parser); union/reference leaves are
/// skipped by [`ValueType::is_checked`].
pub fn value_diags(doc: &JsonDoc, text: &str, lib: &Library) -> Vec<InstDiag> {
    let map = map(doc, lib);
    let mut diags = Vec::new();
    for (m, member) in doc.members.iter().enumerate() {
        let Some(Some(res)) = map.res.get(m).cloned() else {
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
        // A leaf has one value; a leaf-list has one value per scalar element.
        let ranges: Vec<Range<usize>> = match node.kind() {
            NodeKind::Leaf => vec![member.value_range.clone()],
            _ => member
                .scalar_items
                .clone()
                .unwrap_or_else(|| vec![member.value_range.clone()]),
        };
        for range in ranges {
            let Some(raw) = text.get(range.clone()) else {
                continue;
            };
            let value = json_value(&vt, raw);
            if let Some(msg) = crate::valcheck::check_value(lib, &res.module, &vt, &value) {
                diags.push(InstDiag {
                    range,
                    message: format!("invalid value: {msg}"),
                    code: "json_bad_value",
                });
            }
        }
    }
    diags
}

/// Decode an RFC 7951 member value into the canonical text that
/// [`crate::valcheck::check`] expects: `empty` → `""` from `[null]`; quoted
/// members (string/binary/enumeration/bits) are JSON-unescaped; numbers and
/// booleans stay literal.
fn json_value(vt: &ValueType, raw: &str) -> String {
    let r = raw.trim();
    if matches!(vt, ValueType::Empty) {
        return if r == "[null]" {
            String::new()
        } else {
            r.to_owned()
        };
    }
    if r.starts_with('"') {
        serde_json::from_str::<String>(r).unwrap_or_else(|_| r.to_owned())
    } else {
        r.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use yrepo::Repository;

    use super::*;
    use crate::json::parse;

    const MOD: &str = r#"module j {
  yang-version 1.1;
  namespace "urn:j";
  prefix j;
  revision 2026-01-01;
  container top {
    list item {
      key "name";
      leaf name { type string; }
      leaf enabled { type boolean; }
    }
    leaf note { type string; }
  }
  container other { leaf x { type string; } }
}"#;

    fn compile() -> Arc<Library> {
        let mut repo = Repository::new();
        repo.upsert("/j.yang", MOD);
        repo.compile().library.expect("library")
    }

    fn member(doc: &JsonDoc, local: &str) -> usize {
        doc.members
            .iter()
            .position(|m| m.local == local)
            .unwrap_or_else(|| panic!("no member {local}"))
    }

    #[test]
    fn maps_top_level_and_nested_members() {
        let lib = compile();
        let text = r#"{
  "j:top": {
    "item": [
      { "name": "eth0", "enabled": true },
      { "name": "eth1" }
    ],
    "note": "hi"
  },
  "j:other": { "x": "y" }
}"#;
        let doc = parse(text).expect("parse");
        let jm = map(&doc, &lib);
        assert!(jm.diags.is_empty(), "{:?}", jm.diags);
        assert_eq!(
            doc.members[member(&doc, "top")].module.as_deref(),
            Some("j")
        );
        for local in ["top", "item", "name", "enabled", "note", "other", "x"] {
            assert!(jm.resolved(member(&doc, local)).is_some(), "{local}");
        }
    }

    #[test]
    fn flags_unknown_and_wrong_module_nested_members() {
        let lib = compile();
        let text = r#"{
  "j:top": {
    "bogus": 1,
    "name": "x"
  }
}"#;
        let doc = parse(text).expect("parse");
        let jm = map(&doc, &lib);
        // "name" is a child of item (list entry), not of top → unknown.
        let unknown: Vec<&InstDiag> = jm
            .diags
            .iter()
            .filter(|d| d.code == "json_unknown_member")
            .collect();
        assert_eq!(unknown.len(), 2, "{:?}", jm.diags);
        // Wrong-module case: inside a list entry, `other:name` (name is a
        // child of item, but in module j, not other).
        let text = r#"{
  "j:top": {
    "j:item": [ { "other:name": "x" } ]
  }
}"#;
        let doc = parse(text).expect("parse");
        let jm = map(&doc, &lib);
        assert!(
            jm.diags.iter().any(|d| d.code == "json_wrong_module"),
            "{:?}",
            jm.diags
        );
    }

    #[test]
    fn foreign_document_stays_dormant() {
        let lib = compile();
        let doc = parse(r#"{ "zz:unknown": { } }"#).expect("parse");
        let jm = map(&doc, &lib);
        assert!(jm.diags.is_empty());
    }

    const MOD_DEPTH: &str = r#"module jd {
  yang-version 1.1;
  namespace "urn:jd";
  prefix jd;
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
        repo.upsert("/jd.yang", MOD_DEPTH);
        repo.compile().library.expect("library")
    }

    fn has_code(jm: &JMap, code: &str) -> bool {
        jm.diags.iter().any(|d| d.code == code)
    }

    #[test]
    fn flags_missing_members_and_empty_choice() {
        let lib = compile_depth();
        // server present but missing mandatory `host` and any `proto` case; the
        // iface entry has its key but misses mandatory `mtu`.
        let text = r#"{
  "jd:server": {
    "iface": [
      { "name": "eth0" }
    ]
  }
}"#;
        let doc = parse(text).expect("parse");
        let jm = map(&doc, &lib);
        assert!(has_code(&jm, "json_missing_member"), "{:?}", jm.diags);
        assert!(has_code(&jm, "json_missing_choice"), "{:?}", jm.diags);
        let joined = jm
            .diags
            .iter()
            .filter(|d| d.code == "json_missing_member")
            .map(|d| d.message.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("host"), "{joined}");
        assert!(joined.contains("mtu"), "{joined}");
        assert!(
            jm.diags
                .iter()
                .any(|d| d.code == "json_missing_choice" && d.message.contains("proto")),
            "{:?}",
            jm.diags
        );
    }

    #[test]
    fn complete_subtree_has_no_depth_diagnostics() {
        let lib = compile_depth();
        let text = r#"{
  "jd:server": {
    "host": "h1",
    "port": 443,
    "iface": [
      { "name": "eth0", "mtu": 1500 }
    ]
  }
}"#;
        let doc = parse(text).expect("parse");
        let jm = map(&doc, &lib);
        assert!(jm.diags.is_empty(), "{:?}", jm.diags);
    }

    #[test]
    fn dormant_doc_clears_depth_diagnostics() {
        let lib = compile_depth();
        // Unrecognized top-level member → whole document dormant (no depth
        // noise from the (mapped) container it contains).
        let doc = parse(r#"{ "zz:server": { "host": "x" } }"#).expect("parse");
        let jm = map(&doc, &lib);
        assert!(jm.diags.is_empty(), "{:?}", jm.diags);
    }

    const MOD_VALUE: &str = r#"module jv {
  yang-version 1.1;
  namespace "urn:jv";
  prefix jv;
  revision 2026-01-01;
  container box {
    leaf port { type uint16 { range "1..65535"; } }
    leaf name { type string { length "1..3"; } }
    leaf flag { type boolean; }
    leaf color { type enumeration { enum red; enum green; } }
    leaf mark { type empty; }
  }
}"#;

    fn compile_value() -> Arc<Library> {
        let mut repo = Repository::new();
        repo.upsert("/jv.yang", MOD_VALUE);
        repo.compile().library.expect("library")
    }

    #[test]
    fn flags_invalid_member_values() {
        let lib = compile_value();
        let text = r#"{
  "jv:box": {
    "port": 99999,
    "name": "toolong",
    "flag": "yes",
    "color": "blue",
    "mark": "x"
  }
}"#;
        let doc = parse(text).expect("parse");
        let diags = value_diags(&doc, text, &lib);
        let bad: Vec<&InstDiag> = diags
            .iter()
            .filter(|d| d.code == "json_bad_value")
            .collect();
        assert_eq!(bad.len(), 5, "{:?}", diags);
        let joined: Vec<&str> = bad.iter().map(|d| d.message.as_str()).collect();
        assert!(
            joined.iter().any(|m| m.contains("out of range")),
            "{joined:?}"
        );
        assert!(joined.iter().any(|m| m.contains("length")), "{joined:?}");
        assert!(joined.iter().any(|m| m.contains("true")), "{joined:?}");
        assert!(joined.iter().any(|m| m.contains("one of")), "{joined:?}");
        assert!(joined.iter().any(|m| m.contains("empty")), "{joined:?}");
    }

    #[test]
    fn valid_member_values_produce_no_value_diagnostics() {
        let lib = compile_value();
        let text = r#"{
  "jv:box": {
    "port": 8080,
    "name": "ok",
    "flag": true,
    "color": "red",
    "mark": [null]
  }
}"#;
        let doc = parse(text).expect("parse");
        let diags = value_diags(&doc, text, &lib);
        assert!(
            !diags.iter().any(|d| d.code == "json_bad_value"),
            "{:?}",
            diags
        );
    }

    const MOD_LEAFLIST: &str = r#"module jll {
  yang-version 1.1;
  namespace "urn:jll";
  prefix jll;
  revision 2026-01-01;
  container box {
    leaf-list port { type uint16 { range "1..10"; } }
  }
}"#;

    fn compile_leaflist() -> Arc<Library> {
        let mut repo = Repository::new();
        repo.upsert("/jll.yang", MOD_LEAFLIST);
        repo.compile().library.expect("library")
    }

    #[test]
    fn leaf_list_entries_are_value_checked_per_element() {
        let lib = compile_leaflist();
        let text = r#"{
  "jll:box": {
    "port": [1, 20]
  }
}"#;
        let doc = parse(text).expect("parse");
        let diags = value_diags(&doc, text, &lib);
        let bad: Vec<&InstDiag> = diags
            .iter()
            .filter(|d| d.code == "json_bad_value")
            .collect();
        assert_eq!(bad.len(), 1, "{:?}", diags);
        assert!(bad[0].message.contains("20"), "{:?}", bad[0]);
        // The diagnostic anchors on the offending scalar element itself.
        let member = doc
            .members
            .iter()
            .position(|m| m.local == "port")
            .expect("port member");
        let items = doc.members[member].scalar_items.clone().expect("items");
        assert_eq!(items.len(), 2);
        assert_eq!(bad[0].range, items[1], "{:?}", bad[0]);
    }

    #[test]
    fn identityref_values_checked_semantically() {
        let src = r#"module jiv {
  yang-version 1.1;
  namespace "urn:jiv";
  prefix jiv;
  revision 2026-01-01;
  identity base;
  identity child { base base; }
  identity other;
  container c {
    leaf ref { type identityref { base base; } }
  }
}"#;
        let mut repo = Repository::new();
        repo.upsert("/jiv.yang", src);
        let lib = repo.compile().library.expect("library");

        // Derived identity → valid.
        let text = r#"{ "jiv:c": { "ref": "jiv:child" } }"#;
        let doc = parse(text).expect("parse");
        let diags = value_diags(&doc, text, &lib);
        assert!(
            !diags.iter().any(|d| d.code == "json_bad_value"),
            "{:?}",
            diags
        );
        // Not derived from `base`.
        let text = r#"{ "jiv:c": { "ref": "jiv:other" } }"#;
        let doc = parse(text).expect("parse");
        let diags = value_diags(&doc, text, &lib);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("not `base` or derived")),
            "{:?}",
            diags
        );
    }
}
