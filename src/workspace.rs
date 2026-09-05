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

/// `std::fs::canonicalize` on Windows returns `\\?\`-prefixed verbatim
/// paths that URI builders would turn into unusable URLs; `dunce`
/// canonicalizes while stripping that prefix (cross-platform).
///
/// Canonical document key for a `file://` URI.
///
/// Client URIs (`textDocument/didOpen`, …) and the workspace scan may spell
/// the same file differently (drive-letter case, `%3A`-style encoding, …).
/// Since `yrepo` keys modules by the exact URL string, such spelling
/// differences make one module enter the repository twice and compile to a
/// spurious "duplicate module" diagnostic. Resolve the file path, canonicalize
/// it, and rebuild the URI so every ingestion path uses one key. Non-file
/// URIs (e.g. tests) pass through unchanged.
pub(crate) fn canon_url(url: &str) -> String {
    url_to_path(url)
        .and_then(|path| {
            let canon = dunce::canonicalize(&path).unwrap_or(path);
            path_to_url(&canon)
        })
        .unwrap_or_else(|| url.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{canon_url, path_to_url, url_to_path};
    #[cfg(windows)]
    use std::path::PathBuf;

    // Windows paths are case-insensitive: a drive letter may be spelled
    // `C:` by the client and `c:` by a URI rebuilt from a scan. That test
    // only makes sense on Windows, so it stays gated here.
    #[test]
    #[cfg(windows)]
    fn canon_url_normalizes_spelling_of_the_same_file() {
        // Windows paths are case-insensitive, but the client URI and a
        // URI rebuilt from a scan may differ in drive-letter case. Both
        // spellings must canonicalize to one key so `yrepo` never sees
        // one module twice.
        let dir = std::env::temp_dir().join("netconf-ls-canon-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("spelling.yang");
        std::fs::write(&file, "module spelling {}").unwrap();
        let url = path_to_url(&file).expect("file url");

        // Same path, drive letter lowercased (`c:\…` instead of `C:\…`).
        let path = url_to_path(&url).expect("file path");
        let path_text = path.to_string_lossy();
        let colon = path_text.find(':').expect("windows drive colon");
        let lower_path = PathBuf::from(format!(
            "{}{}",
            path_text[..colon].to_ascii_lowercase(),
            &path_text[colon..]
        ));
        let lower_url = path_to_url(&lower_path).expect("lower url");

        let key = canon_url(&url);
        assert_eq!(key, canon_url(&lower_url));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn canon_keys_of_real_files_stay_path_convertible() {
        // Zed converts every LocationLink target URI back to a path, so keys
        // we emit must survive `Uri -> file path` (on Windows, canonicalize
        // yields `\\?\` verbatim paths — hence `dunce` in `canon_url`).
        for name in [
            "examples/example-demo.yang",
            "examples/example-ietf-interfaces.yang",
            "examples/example-netconf-config.xml",
            "examples/example-netconf-data.json",
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
            let url = path_to_url(&path).expect("file url");
            let key = canon_url(&url);
            // The key must convert back to the *same* file — a URI that merely
            // parses is not enough (e.g. `file://///%3F/…` from verbatim paths).
            let back = url_to_path(&key).expect("key not path-convertible");
            assert_eq!(
                back.canonicalize().unwrap(),
                path.canonicalize().unwrap(),
                "key points elsewhere: {key}"
            );
            // Canonical keys must be idempotent, or scan/open still diverge.
            assert_eq!(key, canon_url(&key));
        }
    }
}
