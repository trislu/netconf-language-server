//! Folding ranges (`textDocument/foldingRange`): statement-level folds.
//!
//! Every block statement whose `{ … }` body spans more than one line yields a
//! `Region` fold from the `{` line to the `}` line (see `docs/architecture.md`
//! §8.1).

use ropey::Rope;
use tower_lsp_server::ls_types::{FoldingRange, FoldingRangeKind};
use yrepo::{Statement, StatementEnd};

pub(crate) fn handle(rope: &Rope, root: Option<&Statement>) -> Vec<FoldingRange> {
    let Some(root) = root else {
        return vec![];
    };
    let mut out = Vec::new();
    for stmt in root.preorder() {
        if let Some(StatementEnd::Braces { open, close }) = &stmt.end {
            let start = rope.byte_to_line(open.start) as u32;
            let end = rope.byte_to_line(close.start) as u32;
            if end > start {
                out.push(FoldingRange {
                    start_line: start,
                    end_line: end,
                    kind: Some(FoldingRangeKind::Region),
                    ..Default::default()
                });
            }
        }
    }
    out
}
