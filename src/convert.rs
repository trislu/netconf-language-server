//! Byte offset <-> LSP position conversion over a `ropey::Rope`.
//!
//! `yrepo` speaks UTF-8 **byte** offsets; LSP speaks **line / UTF-16 column**
//! positions. All conversion happens here.

use std::ops::Range;

use ropey::Rope;
use tower_lsp_server::ls_types::{Position, Range as LspRange};

/// UTF-16 length of the byte `range`.
pub(crate) fn utf16_len(rope: &Rope, range: Range<usize>) -> u32 {
    rope.get_byte_slice(range)
        .map(|s| s.chars().map(|c| c.len_utf16()).sum::<usize>() as u32)
        .unwrap_or(0)
}

/// Map a byte offset to an LSP `Position` (line, UTF-16 column).
pub(crate) fn byte_to_position(rope: &Rope, byte: usize) -> Position {
    let line = rope.byte_to_line(byte.min(rope.len_bytes()));
    let line_start = rope.line_to_byte(line);
    Position {
        line: line as u32,
        character: utf16_len(rope, line_start..byte.min(rope.len_bytes())),
    }
}

/// Map a byte range to an LSP `Range`.
pub(crate) fn range_to_lsp(rope: &Rope, range: Range<usize>) -> LspRange {
    LspRange {
        start: byte_to_position(rope, range.start),
        end: byte_to_position(rope, range.end),
    }
}

/// Map an LSP `Position` to a byte offset (end of the line if the column is
/// past the line's end, clamped inside a multi-byte character).
pub(crate) fn position_to_byte(rope: &Rope, pos: Position) -> Option<usize> {
    let line_count = rope.len_lines();
    if pos.line >= line_count as u32 {
        return None;
    }
    let line_start = rope.line_to_byte(pos.line as usize);
    let line_end = rope.line_to_byte(pos.line as usize + 1);
    let mut byte = line_start;
    let mut utf16: u32 = 0;
    let line = rope.get_byte_slice(line_start..line_end)?;
    for ch in line.chars() {
        if utf16 >= pos.character {
            break;
        }
        let next = utf16 + ch.len_utf16() as u32;
        if next > pos.character {
            // Cursor is inside a multi-byte character: clamp to its start.
            break;
        }
        byte += ch.len_utf8();
        utf16 = next;
    }
    Some(byte)
}
