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
        _ => None,
    }
}
