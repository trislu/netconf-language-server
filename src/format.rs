//! Document formatting (`textDocument/formatting`), full-document `TextEdit`.
//!
//! Strategy (D14): **regenerate from the statement tree + splice comments**.
//! Each statement is printed as `indent keyword [argument] ;` / block form;
//! comment spans come from `Repository::comments` and are re-inserted at the
//! indentation of the block they belong to (never deleted).
//!
//! Argument text is always taken from the **raw source** (`arg.range`), never
//! the dequoted `logical`, so quotes, `+`-concatenations and multi-line
//! strings are preserved verbatim (see `docs/architecture.md` §8.2).

use std::ops::Range;

use ropey::Rope;
use yrepo::{Comment, Statement};

fn indent_of(depth: usize, width: u32) -> String {
    if width == 0 {
        return String::new();
    }
    " ".repeat(depth * width as usize)
}

fn slice(rope: &Rope, range: Range<usize>) -> String {
    rope.get_byte_slice(range).map(|s| s.to_string()).unwrap_or_default()
}

fn emit_comment(out: &mut Vec<String>, comment: &Comment, indent: &str) {
    for (i, line) in comment.text.split('\n').enumerate() {
        if i == 0 {
            out.push(format!("{indent}{line}"));
        } else {
            out.push(line.to_owned());
        }
    }
}

/// Direct comments of `stmt`: start is inside its body but inside no child.
fn direct_comments<'c>(
    stmt: &Statement,
    comments: &'c [Comment],
) -> Vec<&'c Comment> {
    let Some(body) = stmt.body() else {
        return vec![];
    };
    comments
        .iter()
        .filter(|c| {
            body.contains(&c.range.start)
                && !stmt
                    .children
                    .iter()
                    .any(|ch| ch.span().contains(&c.range.start))
        })
        .collect()
}

fn gen_stmt(
    out: &mut Vec<String>,
    rope: &Rope,
    stmt: &Statement,
    depth: usize,
    width: u32,
    comments: &[Comment],
) {
    let indent = indent_of(depth, width);
    let kw = stmt.keyword.as_ref().map(|r| slice(rope, r.clone())).unwrap_or_default();
    let arg_part = stmt
        .arg
        .as_ref()
        .map(|a| {
            let raw = slice(rope, a.range.clone());
            if raw.is_empty() {
                String::new()
            } else {
                format!(" {raw}")
            }
        })
        .unwrap_or_default();

    if !stmt.is_block() {
        out.push(format!("{indent}{kw}{arg_part};"));
        return;
    }

    let direct = direct_comments(stmt, comments);
    if stmt.children.is_empty() && direct.is_empty() {
        out.push(format!("{indent}{kw}{arg_part} {{ }}"));
        return;
    }

    out.push(format!("{indent}{kw}{arg_part} {{"));
    let mut ci = 0usize;
    let mut ki = 0usize;
    while ci < direct.len() || ki < stmt.children.len() {
        let cs = direct.get(ci).map(|c| c.range.start);
        let ks = stmt.children.get(ki).map(|c| c.span().start);
        match (cs, ks) {
            // Comment sits before the next child: emit it first.
            (Some(cs), Some(ks)) if cs < ks => {
                emit_comment(out, direct[ci], &indent_of(depth + 1, width));
                ci += 1;
            }
            // Child comes next.
            (_, Some(_)) => {
                let child = &stmt.children[ki];
                ki += 1;
                gen_stmt(out, rope, child, depth + 1, width, comments);
            }
            // Only comments left.
            (Some(_), None) => {
                emit_comment(out, direct[ci], &indent_of(depth + 1, width));
                ci += 1;
            }
            (None, None) => break,
        }
    }
    out.push(format!("{indent}}}"));
}

pub(crate) fn handle(
    rope: &Rope,
    root: Option<&Statement>,
    comments: &[Comment],
    width: u32,
) -> Option<String> {
    let root = root?;
    let mut out = Vec::new();

    // Comments before the root module statement.
    let root_start = root.span().start;
    for c in comments.iter().filter(|c| c.range.end <= root_start) {
        emit_comment(&mut out, c, "");
    }

    gen_stmt(&mut out, rope, root, 0, width, comments);

    // Comments after the root statement.
    let root_end = root.span().end;
    for c in comments.iter().filter(|c| c.range.start >= root_end) {
        emit_comment(&mut out, c, "");
    }

    while out.last().is_some_and(|l| l.trim().is_empty()) {
        out.pop();
    }
    if out.is_empty() {
        return Some(String::new());
    }
    Some(format!("{}\n", out.join("\n")))
}
