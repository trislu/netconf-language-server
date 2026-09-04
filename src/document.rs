use ropey::Rope;

/// Per-open-document model held by the server.
///
/// Structural/semantic/token views live in the shared `yrepo::Repository`;
/// this only keeps the buffer text (for byte <-> LSP position conversion and
/// for source slicing) plus its LSP version.
#[derive(Debug, Clone)]
pub(crate) struct Document {
    pub(crate) version: i32,
    pub(crate) rope: Rope,
}

impl Document {
    pub(crate) fn new(text: &str, version: i32) -> Self {
        Document {
            version,
            rope: Rope::from_str(text),
        }
    }
}
