//! `identityref` value completion for XML instance documents.
//!
//! An XML-encoded `identityref` value is `pfx:identity`, where `pfx` is bound
//! to the identity module's namespace by an `xmlns` declaration on the element
//! or an ancestor (RFC 7950 §9.10.3). For a leaf whose type is `identityref`,
//! this module offers:
//!
//! - at an `xmlns:` attribute name in the leaf's start tag, one namespace
//!   declaration per module that contributes a matching identity (the module's
//!   YANG prefix is reused as the XML prefix);
//! - in the leaf's text content, one `pfx:identity` value per matching
//!   identity, adding the `xmlns:pfx` declaration when it is not yet in scope.
//!
//! Matching identities come from the workspace-wide [`IdentityGraph`] built
//! from the summary index, so they cover modules outside the open closure.

use std::collections::{BTreeMap, HashMap, HashSet};

use ropey::Rope;
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, Documentation, Range as LspRange,
    TextEdit,
};
use yrepo::{Library, ValueType};

use crate::convert;
use crate::idgraph::{Candidate, IdentityGraph};
use crate::incomplete::tolerant_xml;
use crate::inst_map::{Resolved, map_doc};
use crate::xml::XmlDoc;

/// Completion items for an `identityref` leaf at `byte`, if the caret is in one
/// of the two positions above. Empty when the caret is not on an `identityref`
/// leaf (or no identity derives from its `base`).
pub fn handle(
    rope: &Rope,
    byte: usize,
    lib: &Library,
    graph: &IdentityGraph,
) -> Vec<CompletionItem> {
    let text = rope.to_string();
    let Some(doc) = tolerant_xml(&text) else {
        return Vec::new();
    };
    let Some(elem) = doc.element_at(byte) else {
        return Vec::new();
    };
    let map = map_doc(&doc, lib);
    let Some(res) = map.resolved(elem) else {
        return Vec::new();
    };
    let Some((base_module, base_name)) = identityref_base(lib, res) else {
        return Vec::new();
    };
    let candidates = graph.derived_from(&base_module, &base_name);
    if candidates.is_empty() {
        return Vec::new();
    }
    if in_start_tag(&doc, &text, elem, byte) {
        xmlns_items(rope, &doc, &text, elem, byte, &candidates)
    } else {
        value_items(rope, &doc, &text, byte, elem, &candidates)
    }
}

/// The resolved `(module, local)` of the leaf's identityref `base`.
fn identityref_base(lib: &Library, res: &Resolved) -> Option<(String, String)> {
    let ValueType::Identityref { base } = lib.value_type(&res.module, res.id)? else {
        return None;
    };
    crate::idgraph::resolve_base(lib, &res.module, base.as_deref()?)
}

/// Whether `byte` is inside the element's start tag (before the first `>`).
fn in_start_tag(doc: &XmlDoc, text: &str, elem: usize, byte: usize) -> bool {
    doc.start_tag_end(elem, text).is_some_and(|end| byte < end)
}

/// One module's matching identities, kept in first-seen (sorted) order.
fn group_by_module(candidates: &[Candidate]) -> Vec<(String, Vec<Candidate>)> {
    let mut map: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for c in candidates {
        if c.namespace.is_none() {
            continue;
        }
        map.entry(c.module.clone()).or_default().push(c.clone());
    }
    map.into_iter().collect()
}

/// The XML prefix to use per module: an in-scope declaration already bound to
/// the module's namespace, else the module's YANG prefix, else the module name
/// (made unique).
fn assign_prefixes(
    doc: &XmlDoc,
    elem: usize,
    modules: &[(String, Vec<Candidate>)],
) -> HashMap<String, String> {
    let existing = in_scope_prefixes(doc, elem);
    let mut used: HashSet<String> = existing.iter().map(|(p, _)| p.clone()).collect();
    let mut out = HashMap::new();
    for (module, ids) in modules {
        let Some(ns) = ids.first().and_then(|c| c.namespace.as_deref()) else {
            continue;
        };
        if let Some((prefix, _)) = existing.iter().find(|(_, uri)| uri == ns) {
            out.insert(module.clone(), prefix.clone());
        }
    }
    for (module, ids) in modules {
        if out.contains_key(module) {
            continue;
        }
        if let Some(prefix) = ids.first().and_then(|c| c.prefix.clone())
            && !used.contains(&prefix)
        {
            used.insert(prefix.clone());
            out.insert(module.clone(), prefix);
        }
    }
    for (module, _) in modules {
        if out.contains_key(module) {
            continue;
        }
        let mut prefix = module.clone();
        while used.contains(&prefix) {
            prefix.push('_');
        }
        used.insert(prefix.clone());
        out.insert(module.clone(), prefix);
    }
    out
}

/// In-scope `prefix → uri` declarations on `elem`, nearest declaration first.
fn in_scope_prefixes(doc: &XmlDoc, elem: usize) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut cur = Some(elem);
    while let Some(i) = cur {
        let Some(n) = doc.nodes.get(i) else {
            break;
        };
        for d in n.own_ns.iter().rev() {
            if let Some(prefix) = &d.prefix
                && seen.insert(prefix.clone())
            {
                out.push((prefix.clone(), d.uri.clone()));
            }
        }
        cur = n.parent;
    }
    out
}

/// Completion at an `xmlns:` attribute name in the leaf's start tag: one
/// namespace declaration per module contributing a matching identity.
fn xmlns_items(
    rope: &Rope,
    doc: &XmlDoc,
    text: &str,
    elem: usize,
    byte: usize,
    candidates: &[Candidate],
) -> Vec<CompletionItem> {
    let token_start = word_start(text, byte);
    let typed = &text[token_start..byte];
    // Only when the user is (still) typing the `xmlns:` attribute name.
    if !typed.starts_with("xmlns:") && !"xmlns:".starts_with(typed) {
        return Vec::new();
    }
    if in_value_context(text, token_start) {
        return Vec::new();
    }
    let modules = group_by_module(candidates);
    if modules.is_empty() {
        return Vec::new();
    }
    let prefixes = assign_prefixes(doc, elem, &modules);
    let range = convert::range_to_lsp(rope, token_start..byte);
    let mut items = Vec::new();
    for (module, ids) in &modules {
        let Some(ns) = ids.first().and_then(|c| c.namespace.clone()) else {
            continue;
        };
        let prefix = &prefixes[module];
        let declaration = format!("xmlns:{prefix}=\"{ns}\"");
        let mut body = format!("module `{module}`\n\nnamespace `{ns}`\n\nmatching identities:\n");
        for id in ids {
            body.push_str(&format!("\n- `{prefix}:{}`", id.name));
        }
        items.push(CompletionItem {
            label: declaration.clone(),
            kind: Some(CompletionItemKind::MODULE),
            detail: Some(format!("module {module} (prefix {prefix})")),
            documentation: Some(Documentation::MarkupContent(
                tower_lsp_server::ls_types::MarkupContent {
                    kind: tower_lsp_server::ls_types::MarkupKind::Markdown,
                    value: body,
                },
            )),
            filter_text: Some(prefix.clone()),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: declaration,
            })),
            ..Default::default()
        });
    }
    items
}

/// Completion in the leaf's text content: one `pfx:identity` value per matching
/// identity, declaring `xmlns:pfx` on the element when it is not in scope.
fn value_items(
    rope: &Rope,
    doc: &XmlDoc,
    text: &str,
    byte: usize,
    elem: usize,
    candidates: &[Candidate],
) -> Vec<CompletionItem> {
    let modules = group_by_module(candidates);
    if modules.is_empty() {
        return Vec::new();
    }
    let prefixes = assign_prefixes(doc, elem, &modules);
    let token_start = word_start(text, byte);
    let range = convert::range_to_lsp(rope, token_start..byte);
    let mut items = Vec::new();
    for (module, ids) in &modules {
        let Some(ns) = ids.first().and_then(|c| c.namespace.clone()) else {
            continue;
        };
        let prefix = &prefixes[module];
        // Declare the prefix on the element unless it is already in scope.
        let extra = if doc.in_scope_ns(elem, Some(prefix)) == Some(ns.as_str()) {
            None
        } else {
            let pos = convert::byte_to_position(rope, doc.nodes[elem].name_range.end);
            Some(vec![TextEdit {
                range: LspRange {
                    start: pos,
                    end: pos,
                },
                new_text: format!(" xmlns:{prefix}=\"{ns}\""),
            }])
        };
        for id in ids {
            let label = format!("{prefix}:{}", id.name);
            items.push(CompletionItem {
                label: label.clone(),
                kind: Some(CompletionItemKind::ENUM),
                detail: Some(format!("identity (module {module})")),
                documentation: Some(Documentation::String(format!(
                    "namespace `{ns}`\nmodule prefix `{prefix}`"
                ))),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: label,
                })),
                additional_text_edits: extra.clone(),
                ..Default::default()
            });
        }
    }
    items
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':')
}

/// Start of the `[A-Za-z0-9_.:-]` token containing `byte`.
pub(crate) fn word_start(text: &str, byte: usize) -> usize {
    let mut i = byte.min(text.len());
    while i > 0 {
        match text[..i].chars().next_back() {
            Some(c) if is_name_char(c) => i -= c.len_utf8(),
            _ => break,
        }
    }
    i
}

/// Whether `token_start` sits after `=`/a quote, i.e. inside an attribute value
/// rather than an attribute name.
fn in_value_context(text: &str, token_start: usize) -> bool {
    matches!(
        text[..token_start]
            .chars()
            .rev()
            .find(|c| !c.is_whitespace()),
        Some('=') | Some('"') | Some('\'')
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use yrepo::{Repository, SummaryIndex};

    use super::*;

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

    /// A compiled library plus the workspace identity graph over the same two
    /// modules.
    fn setup() -> (Arc<Library>, IdentityGraph) {
        let dir = std::env::temp_dir().join(format!(
            "ncls-idcomp-{}-{}",
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
        let mut index = SummaryIndex::default();
        index.scan_many_files_with([&a, &b], |p| Some(p.to_string_lossy().to_string()));
        let graph = IdentityGraph::build(&index);
        std::fs::remove_dir_all(&dir).ok();
        (lib, graph)
    }

    fn at(text: &str) -> (Rope, usize) {
        let i = text.find('|').expect("marker");
        (Rope::from_str(&text.replacen('|', "", 1)), i)
    }

    #[test]
    fn xmlns_position_offers_one_declaration_per_module() {
        let (lib, graph) = setup();
        let (rope, byte) = at(r#"<box xmlns="urn:ida"><id xmlns:|></id></box>"#);
        let items = handle(&rope, byte, &lib, &graph);
        let mut labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        labels.sort();
        assert_eq!(
            labels,
            vec!["xmlns:a=\"urn:ida\"", "xmlns:b=\"urn:idb\""],
            "one declaration per matching module"
        );
    }

    #[test]
    fn value_position_offers_derived_identities_and_declares_the_prefix() {
        let (lib, graph) = setup();
        let (rope, byte) = at(r#"<box xmlns="urn:ida"><id>|</id></box>"#);
        let items = handle(&rope, byte, &lib, &graph);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        // The base itself plus the identity derived in the other module.
        assert_eq!(labels, vec!["a:root", "b:child"]);
        let child = items.iter().find(|i| i.label == "b:child").unwrap();
        let extra = child.additional_text_edits.as_ref().expect("xmlns edit");
        assert_eq!(extra[0].new_text, " xmlns:b=\"urn:idb\"");
    }

    #[test]
    fn an_in_scope_prefix_is_reused_without_a_declaration() {
        let (lib, graph) = setup();
        let (rope, byte) = at(r#"<box xmlns="urn:ida" xmlns:b="urn:idb"><id>|</id></box>"#);
        let items = handle(&rope, byte, &lib, &graph);
        let child = items.iter().find(|i| i.label == "b:child").unwrap();
        assert!(child.additional_text_edits.is_none());
    }
}
