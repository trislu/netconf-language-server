//! Completion (`textDocument/completion`) for `type` and identity `base`
//! arguments (D16), backed by `Library::{type,identity}_candidates`.

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
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yrepo::Repository;

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
        let items = handle(root, byte, "cmod", &lib, &fake_params()).expect("items");
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
}
