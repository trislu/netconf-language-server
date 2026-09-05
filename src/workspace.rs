//! Workspace `.yang` file discovery and URI <-> path helpers.

use std::path::{Path, PathBuf};

use tower_lsp_server::ls_types::Uri;

/// Recursively find every `*.yang` file under `root`, skipping common
/// non-source directories.
pub(crate) fn walk_yang_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if matches!(
                    name.as_ref(),
                    "target" | ".git" | "node_modules" | ".vscode" | "dist"
                ) {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "yang") {
                out.push(path);
            }
        }
    }
    out
}

/// The language a document belongs to, decided by its file extension.
///
/// `yang` drives the YANG repo/snapshot path; `xml`/`json` are candidate
/// NETCONF instance documents (routed to the instance path — never upserted
/// into `yrepo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DocLang {
    Yang,
    Xml,
    Json,
    Other,
}

/// Classify a document by its file extension (M0 language routing).
pub(crate) fn doc_lang(url: &str) -> DocLang {
    match url_to_path(url).and_then(|p| p.extension().map(|e| e.to_string_lossy().into_owned())) {
        Some(e) if e == "yang" => DocLang::Yang,
        Some(e) if e == "xml" => DocLang::Xml,
        Some(e) if e == "json" => DocLang::Json,
        _ => DocLang::Other,
    }
}

/// Whether a document is a YANG module (the only docs `yrepo` sees).
pub(crate) fn is_yang(url: &str) -> bool {
    doc_lang(url) == DocLang::Yang
}

/// Best-effort file URI string for a path.
pub(crate) fn path_to_url(path: &Path) -> Option<String> {
    Uri::from_file_path(path).map(|u| u.as_str().to_owned())
}

/// Convert a `file://` URI string to a filesystem path.
pub(crate) fn url_to_path(url: &str) -> Option<PathBuf> {
    let uri = url.parse::<Uri>().ok()?;
    uri.to_file_path().map(|p| p.into_owned())
}
