//! Completion (`textDocument/completion`) for `type` and identity `base`
//! arguments (D16), backed by `Library::{type,identity}_candidates`.

use ropey::Rope;
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionParams,
};
use yrepo::{Library, Statement, StatementKind, TypeCandidateKind};

pub(crate) fn capability() -> CompletionOptions {
    CompletionOptions {
        // `:` refreshes prefix-qualified YANG candidates as they are typed; `<`
        // starts an XML instance start tag (M2); `{`/`,` open a fresh member
        // slot in a JSON (RFC 7951) object (M4). Empty results are harmless
        // for the other languages.
        trigger_characters: Some(vec![
            ":".to_owned(),
            "<".to_owned(),
            "{".to_owned(),
            ",".to_owned(),
        ]),
        ..Default::default()
    }
}

/// Completion items for the statement under `byte` (if it is a `type`/`base`
/// argument).
pub(crate) fn handle(
    root: &Statement,
    rope: &Rope,
    byte: usize,
    scope: &str,
    lib: &Library,
    _params: &CompletionParams,
) -> Option<Vec<CompletionItem>> {
    use StatementKind as K;
    let stmt = root.narrowest_at(byte)?;
    let arg = stmt.arg.as_ref()?;
    if !arg.range.contains(&byte) {
        return None;
    }

    match &stmt.kind {
        K::Type => {
            let items = lib.type_candidates(scope);
            if items.is_empty() {
                return Some(vec![]);
            }
            Some(
                items
                    .into_iter()
                    .map(|c| CompletionItem {
                        label: c.name.clone(),
                        kind: Some(match c.kind {
                            TypeCandidateKind::Builtin => CompletionItemKind::TYPE_PARAMETER,
                            TypeCandidateKind::Typedef => CompletionItemKind::STRUCT,
                        }),
                        detail: Some(match c.module {
                            Some(m) => format!("{m} (typedef)"),
                            None => "built-in".to_owned(),
                        }),
                        ..Default::default()
                    })
                    .collect(),
            )
        }
        K::Base => {
            let items = lib.identity_candidates(scope);
            if items.is_empty() {
                return Some(vec![]);
            }
            Some(
                items
                    .into_iter()
                    .map(|name| CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::ENUM),
                        detail: Some("identity".to_owned()),
                        ..Default::default()
                    })
                    .collect(),
            )
        }
        K::Uses => {
            let items = lib.grouping_candidates(scope);
            if items.is_empty() {
                return Some(vec![]);
            }
            Some(
                items
                    .into_iter()
                    .map(|name| CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::STRUCT),
                        detail: Some("grouping".to_owned()),
                        ..Default::default()
                    })
                    .collect(),
            )
        }
        K::Path | K::Augment | K::Deviation => path_completions(rope, stmt, byte, scope, lib),
        _ => None,
    }
}

/// Completion for a `type leafref { path "…" }` argument: suggest schema node
/// names along the partially typed absolute path. The context node is found
/// by walking the completed segments (predicates dropped) from the module
/// named by the first segment's prefix; children are offered as the typed
/// prefix (or bare when the target module is the current scope).
fn path_completions(
    rope: &Rope,
    stmt: &Statement,
    byte: usize,
    scope: &str,
    lib: &Library,
) -> Option<Vec<CompletionItem>> {
    use yrepo::NodeKind as NK;
    let arg = stmt.arg.as_ref()?;
    if !arg.range.contains(&byte) {
        return None;
    }
    let mut typed = rope
        .get_byte_slice(arg.range.start..byte)
        .map(|s| s.to_string())
        .unwrap_or_default();
    typed = typed.trim_start_matches(['"', '\'', ' ']).to_string();
    if !typed.starts_with('/') {
        return None;
    }
    let (mut full_parent, mut partial) = match typed.rfind('/') {
        Some(i) if i + 1 < typed.len() => (typed[..=i].to_string(), typed[i + 1..].to_string()),
        _ => (typed.clone(), String::new()),
    };
    // A trailing "b:" means the next segment's prefix is typed but its name
    // is not yet — match on the empty name; a partially typed "b:po" matches
    // "po" (labels reuse the path's own prefix).
    if partial.ends_with(':') {
        partial.clear();
    } else if let Some((_, rest)) = partial.split_once(':') {
        partial = rest.to_string();
    }
    if !full_parent.ends_with('/') {
        full_parent.push('/');
    }
    // Drop any already-typed segment that is still incomplete.
    let segs: Vec<&str> = full_parent.split('/').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        // Leading "/" with no module prefix yet: offer "pfx:" candidates for
        // the module's own prefix and every import.
        let mut prefixes: Vec<(String, String)> = Vec::new();
        if let Some(own) = lib.module(scope)?.prefix() {
            prefixes.push((own.to_string(), scope.to_string()));
        }
        prefixes.extend(lib.import_prefixes(scope));
        let partial = partial.trim_start_matches(':');
        let items: Vec<CompletionItem> = prefixes
            .into_iter()
            .filter(|(p, _)| p.starts_with(partial))
            .map(|(p, m)| CompletionItem {
                label: format!("{p}:"),
                kind: Some(CompletionItemKind::MODULE),
                detail: Some(format!("module {m}")),
                ..Default::default()
            })
            .collect();
        return Some(items);
    }
    let first = segs[0];
    let (pfx, _) = match first.split_once(':') {
        Some((p, l)) => (Some(p.to_string()), l.to_string()),
        None => (None, first.to_string()),
    };
    let module = match &pfx {
        Some(p) => lib.prefix_to_module(scope, p)?.to_string(),
        None => scope.to_string(),
    };
    let rec = lib.module(&module)?;
    let mut cur: Option<usize> = None;
    for seg in segs {
        let kids: Vec<usize> = match cur {
            None => rec.top_nodes().to_vec(),
            Some(id) => rec.node(id)?.children().to_vec(),
        };
        let target = local_of(seg);
        let mut found = None;
        for id in kids {
            let n = rec.node(id)?;
            if n.name() == target {
                found = Some(id);
                break;
            }
        }
        match found {
            Some(id) => cur = Some(id),
            None => return None,
        }
    }
    let kids: Vec<usize> = match cur {
        None => rec.top_nodes().to_vec(),
        Some(id) => rec.node(id)?.children().to_vec(),
    };
    let label_for = |name: &str| -> String {
        match &pfx {
            Some(p) => format!("{p}:{name}"),
            None if module == scope => name.to_string(),
            None => name.to_string(),
        }
    };
    let mut items = Vec::new();
    for id in kids {
        let n = rec.node(id)?;
        let name = n.name();
        if name.is_empty() || !name.starts_with(&partial) {
            continue;
        }
        let kind = match n.kind() {
            NK::Leaf | NK::LeafList => CompletionItemKind::TYPE_PARAMETER,
            NK::Container | NK::List | NK::Choice | NK::Case => CompletionItemKind::STRUCT,
            _ => CompletionItemKind::FIELD,
        };
        items.push(CompletionItem {
            label: label_for(name),
            kind: Some(kind),
            detail: Some("schema node".to_owned()),
            ..Default::default()
        });
    }
    Some(items)
}

fn strip_preds(seg: &str) -> &str {
    match seg.find('[') {
        Some(i) => &seg[..i],
        None => seg,
    }
}

fn local_of(seg: &str) -> &str {
    strip_preds(seg).rsplit(':').next().unwrap_or(seg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yrepo::Repository;

    const LR_BASE_MOCK: &str = "module lrbase {\n  namespace \"urn:lb\";\n  prefix b;\n\
          container top { list item { key name; leaf name { type string; }\n\
          leaf port { type uint16; } } }\n\
        }\n";

    const BASE: &str = "module cbase {\n  namespace \"urn:cb\";\n  prefix cb;\n\
      grouping bg { leaf x { type string; } }\n\
    }\n";
    const MOD: &str = "module cmod {\n  namespace \"urn:cm\";\n  prefix cm;\n\
      import cbase { prefix cb; }\n\
      grouping own-g { leaf y { type string; } }\n\
      container c { uses zzz }\n\
    }\n";

    #[test]
    fn completion_suggests_groupings_for_uses() {
        let mut repo = Repository::new();
        repo.upsert("/cbase.yang", BASE.to_string());
        repo.upsert("/cmod.yang", MOD.to_string());
        let out = repo.compile();
        let lib = out.library.expect("lib");
        let root = repo.statement("/cmod.yang").expect("root");
        let byte = MOD.find("uses zzz").unwrap() + "uses ".len();
        let rope = Rope::from_str(MOD);
        let items = handle(root, &rope, byte, "cmod", &lib, &fake_params()).expect("items");
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"own-g"), "labels: {labels:?}");
        assert!(labels.contains(&"cb:bg"), "labels: {labels:?}");
    }

    fn fake_params() -> CompletionParams {
        // Only used to satisfy the handle signature; never read for YANG.
        serde_json::from_str(
            r#"{"textDocument":{"uri":"file:///cmod.yang"},"position":{"line":0,"character":0}}"#,
        )
        .unwrap()
    }

    #[test]
    fn completion_suggests_leafref_path_nodes() {
        const LR_BASE: &str = "module lrbase {\n  namespace \"urn:lb\";\n  prefix b;\n\
          container top { list item { key name; leaf name { type string; }\n\
          leaf port { type uint16; } } }\n\
        }\n";
        const LR: &str = "module lr {\n  namespace \"urn:lr\";\n  prefix l;\n\
          import lrbase { prefix b; }\n\
          leaf ref { type leafref { path \"/b:top/b:item/b:\" } }\n\
        }\n";
        let mut repo = Repository::new();
        repo.upsert("/lrbase.yang", LR_BASE.to_string());
        repo.upsert("/lr.yang", LR.to_string());
        let out = repo.compile();
        let lib = out.library.expect("lib");
        let root = repo.statement("/lr.yang").expect("root");
        let rope = Rope::from_str(LR);
        let byte = LR.find("path \"/b:top/b:item/b:\"").unwrap() + "path \"/b:top/b:item/b:".len();
        let items = handle(root, &rope, byte, "lr", &lib, &fake_params()).expect("items");
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"b:port"), "labels: {labels:?}");
        assert!(labels.contains(&"b:name"), "labels: {labels:?}");
    }

    #[test]
    fn completion_suggests_module_prefixes_at_path_start() {
        const LR2: &str = "module lr {\n  namespace \"urn:lr\";\n  prefix l;\n\
          import lrbase { prefix b; }\n\
          leaf ref { type leafref { path \"/b\" } }\n\
        }\n";
        let mut repo = Repository::new();
        repo.upsert("/lrbase.yang", LR_BASE_MOCK.to_string());
        repo.upsert("/lr.yang", LR2.to_string());
        let out = repo.compile();
        let lib = out.library.expect("lib");
        let root = repo.statement("/lr.yang").expect("root");
        let rope = Rope::from_str(LR2);
        let byte = LR2.find("path \"/b\"").unwrap() + "path \"/".len() + 1;
        let items = handle(root, &rope, byte, "lr", &lib, &fake_params()).expect("items");
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert!(labels.contains(&"b:"), "labels: {labels:?}");
    }
}
