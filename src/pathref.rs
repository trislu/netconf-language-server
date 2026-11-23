//! Source positions for schema-node-id path arguments (`augment`, `deviation`,
//! `leafref` `path`).
//!
//! The compiled library resolves a whole path at once, but hover and goto need
//! the node at the segment under the caret, and on a `prefix:name` segment the
//! `import` that binds the prefix. This module maps each segment back to its
//! bytes in the argument, and accumulates the path text up to that segment for
//! [`yrepo::Library::resolve_abs_schema_node_id`].

use std::ops::Range;

use ropey::Rope;
use yrepo::Argument;

/// One `/`-separated segment of a schema-node-id argument.
#[derive(Debug, Clone)]
pub(crate) struct PathSegment {
    /// Byte range of `prefix:name` in the source (predicates excluded).
    pub(crate) name_range: Range<usize>,
    /// Byte range of the prefix half, when the segment is prefixed.
    pub(crate) prefix_range: Option<Range<usize>>,
    /// Byte range of the local-name half.
    pub(crate) local_range: Range<usize>,
    /// The path through this segment, predicates stripped, in the form
    /// `resolve_abs_schema_node_id` takes.
    pub(crate) path: String,
}

impl PathSegment {
    /// The prefix text when `byte` lands on the prefix half. The colon counts
    /// as part of the prefix, as in [`crate::goto::ref_part_at`].
    pub(crate) fn prefix_at(&self, rope: &Rope, byte: usize) -> Option<String> {
        let range = self.prefix_range.as_ref()?;
        if byte < range.start || byte > range.end {
            return None;
        }
        Some(rope.get_byte_slice(range.clone())?.to_string())
    }
}

/// Every segment of `arg`, in source order.
///
/// Segments are split at `/` outside predicates. Within a predicate, quotes
/// protect `/`, `[` and `]`, so `[name='a/b']` stays one segment. Whitespace,
/// quotes and `+` between segments belong to no segment, which also covers a
/// path written as concatenated strings.
pub(crate) fn path_segments(rope: &Rope, arg: &Argument) -> Vec<PathSegment> {
    let Some(raw) = rope.get_byte_slice(arg.range.clone()) else {
        return Vec::new();
    };
    let raw = raw.to_string();
    let bytes = raw.as_bytes();
    let base = arg.range.start;
    let mut segments = Vec::new();
    let mut path = String::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    for (i, &b) in bytes.iter().enumerate() {
        if depth == 0 {
            match b {
                b'[' => depth = 1,
                b'/' => {
                    push(&mut segments, &mut path, &raw, base, start, i);
                    start = i + 1;
                }
                _ => {}
            }
        } else {
            match b {
                b'\'' | b'"' => match quote {
                    Some(q) if q == b => quote = None,
                    None => quote = Some(b),
                    _ => {}
                },
                b'[' if quote.is_none() => depth += 1,
                b']' if quote.is_none() => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    push(&mut segments, &mut path, &raw, base, start, bytes.len());
    segments
}

/// The segment whose `prefix:name` contains `byte`, with `true` when it is the
/// last segment (the argument's target node).
pub(crate) fn path_segment_at(
    rope: &Rope,
    arg: &Argument,
    byte: usize,
) -> Option<(PathSegment, bool)> {
    let segments = path_segments(rope, arg);
    let last = segments.len().checked_sub(1)?;
    let mut found = None;
    for (i, segment) in segments.into_iter().enumerate() {
        if segment.name_range.contains(&byte) {
            found = Some((segment, i == last));
            break;
        }
    }
    found
}

/// Append one segment when it names a node.
fn push(
    segments: &mut Vec<PathSegment>,
    path: &mut String,
    raw: &str,
    base: usize,
    start: usize,
    end: usize,
) {
    if start >= end {
        return;
    }
    // The node name ends where the first predicate starts.
    let name_end = raw[start..end].find('[').map_or(end, |i| start + i);
    // Quotes, whitespace and `+` around a segment belong to the argument, not
    // to the name: `"/a" + "/b"` still yields two segments.
    let text = raw[start..name_end]
        .trim_matches(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == '+');
    if text.is_empty() {
        return;
    }
    let Some(offset) = raw[start..name_end].find(text) else {
        return;
    };
    let name_start = start + offset;
    let name_end = name_start + text.len();
    let (prefix_range, local_start) = match text.find(':') {
        Some(colon) => (
            Some(base + name_start..base + name_start + colon),
            name_start + colon + 1,
        ),
        None => (None, name_start),
    };
    path.push('/');
    path.extend(text.chars().filter(|c| !c.is_whitespace()));
    segments.push(PathSegment {
        name_range: base + name_start..base + name_end,
        prefix_range,
        local_range: base + local_start..base + name_end,
        path: path.clone(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;
    use yrepo::Repository;

    fn arg_range(source: &str, needle: &str) -> (Rope, Argument) {
        let mut repo = Repository::new();
        repo.upsert("/p.yang", source.to_string());
        let root = repo.statement("/p.yang").expect("root").clone();
        let stmt = root
            .preorder()
            .find(|s| s.arg.as_ref().is_some_and(|a| a.name().contains(needle)));
        let arg = stmt.and_then(|s| s.arg.clone()).expect("argument");
        (Rope::from_str(source), arg)
    }

    #[test]
    fn segments_keep_their_bytes_and_strip_predicates() {
        const PATH: &str = "module p {\n  namespace \"urn:p\";\n  prefix p;\n\
          container top { list item { key name; leaf name { type string; }\n\
          leaf port { type uint16; } } }\n\
          deviation \"/p:top/p:item[p:name='a/b']/p:port\" { deviate not-supported; }\n}\n";
        let (rope, arg) = arg_range(PATH, "/p:top");
        let segments = path_segments(&rope, &arg);
        let names: Vec<String> = segments
            .iter()
            .map(|s| {
                rope.get_byte_slice(s.name_range.clone())
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(names, ["p:top", "p:item", "p:port"]);
        assert_eq!(segments[1].path, "/p:top/p:item");
        assert_eq!(segments[2].path, "/p:top/p:item/p:port");

        let prefix = segments[1].prefix_at(&rope, segments[1].prefix_range.as_ref().unwrap().start);
        assert_eq!(prefix.as_deref(), Some("p"));
        // The colon counts as prefix, the local name does not.
        let colon = segments[1].prefix_range.as_ref().unwrap().end;
        assert_eq!(segments[1].prefix_at(&rope, colon).as_deref(), Some("p"));
        assert!(
            segments[1]
                .prefix_at(&rope, segments[1].local_range.start)
                .is_none()
        );
    }

    #[test]
    fn concatenated_paths_still_split() {
        const PATH: &str = "module p {\n  namespace \"urn:p\";\n  prefix p;\n\
          container top { leaf port { type uint16; } }\n\
          deviation \"/p:top\" + \"/p:port\" { deviate not-supported; }\n}\n";
        let (rope, arg) = arg_range(PATH, "/p:top");
        let segments = path_segments(&rope, &arg);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].path, "/p:top/p:port");
        assert_eq!(
            rope.get_byte_slice(segments[1].name_range.clone())
                .unwrap()
                .to_string(),
            "p:port"
        );
    }

    #[test]
    fn an_unprefixed_segment_has_no_prefix_range() {
        const PATH: &str = "module p {\n  namespace \"urn:p\";\n  prefix p;\n\
          container top { leaf port { type uint16; } }\n\
          deviation \"/top/port\" { deviate not-supported; }\n}\n";
        let (rope, arg) = arg_range(PATH, "/top");
        let segments = path_segments(&rope, &arg);
        assert_eq!(segments.len(), 2);
        assert!(segments[0].prefix_range.is_none());
        assert_eq!(
            rope.get_byte_slice(segments[1].local_range.clone())
                .unwrap()
                .to_string(),
            "port"
        );
    }
}
