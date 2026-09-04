//! Semantic tokens (`textDocument/semanticTokens/full`).
//!
//! Two complementary passes produce **disjoint, sorted** tokens:
//! 1. structural — statement `keyword` spans and *atomic* argument spans
//!    (classified by `StatementKind`);
//! 2. lexical — `yrepo::Repository::tokens` for comments anywhere plus the
//!    literal tokens (`string`/`number`/`boolean`/`+`) that fall inside
//!    *composite* argument spans.
//!
//! See `docs/architecture.md` §8.3 (D4/D15).

use std::ops::Range;

use ropey::Rope;
use tower_lsp_server::ls_types::{
    SemanticToken, SemanticTokenType, SemanticTokens, SemanticTokensFullOptions,
    SemanticTokensLegend, SemanticTokensOptions, SemanticTokensResult,
    SemanticTokensServerCapabilities,
};
use yrepo::{Statement, StatementKind, Token, TokenKind};

/// The semantic token classes we emit, in legend order (index == variant rank).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum Class {
    Keyword = 0,
    Namespace = 1,
    Type = 2,
    Variable = 3,
    String = 4,
    Number = 5,
    Comment = 6,
    Operator = 7,
}

impl Class {
    fn lsp_type(self) -> SemanticTokenType {
        use SemanticTokenType as T;
        match self {
            Class::Keyword => T::KEYWORD,
            Class::Namespace => T::NAMESPACE,
            Class::Type => T::TYPE,
            Class::Variable => T::VARIABLE,
            Class::String => T::STRING,
            Class::Number => T::NUMBER,
            Class::Comment => T::COMMENT,
            Class::Operator => T::OPERATOR,
        }
    }
}

pub(crate) fn capability() -> SemanticTokensServerCapabilities {
    SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
        legend: SemanticTokensLegend {
            token_types: [
                Class::Keyword,
                Class::Namespace,
                Class::Type,
                Class::Variable,
                Class::String,
                Class::Number,
                Class::Comment,
                Class::Operator,
            ]
            .iter()
            .map(|c| c.lsp_type())
            .collect(),
            token_modifiers: vec![],
        },
        full: Some(SemanticTokensFullOptions::Bool(true)),
        ..Default::default()
    })
}

/// How a statement's argument should be colored (D4 map).
#[derive(Debug, Clone, Copy)]
enum ArgSemantics {
    /// Identifier / identifier-ref: one token over the whole argument span.
    Atomic(Class),
    /// Value whose internals (string/number/bool/`+`) get their own tokens.
    Composite,
}

fn arg_semantics(kind: &StatementKind) -> ArgSemantics {
    use StatementKind as K;
    match kind {
        // Names: modules/prefixes.
        K::Module | K::Submodule | K::Import | K::Include | K::BelongsTo | K::Prefix => {
            ArgSemantics::Atomic(Class::Namespace)
        }
        // Type / grouping / identity references.
        K::Type | K::Uses | K::Base | K::Refine => ArgSemantics::Atomic(Class::Type),
        // Data nodes and definition names.
        K::Container
        | K::Leaf
        | K::LeafList
        | K::List
        | K::Choice
        | K::Case
        | K::Anyxml
        | K::Anydata
        | K::Rpc
        | K::Action
        | K::Notification
        | K::Grouping
        | K::Typedef
        | K::Identity
        | K::Feature
        | K::Extension
        | K::Bit
        | K::Enum => ArgSemantics::Atomic(Class::Variable),
        _ => ArgSemantics::Composite,
    }
}

/// UTF-16 length of a byte range.
fn utf16_len(rope: &Rope, range: Range<usize>) -> u32 {
    rope.get_byte_slice(range)
        .map(|s| s.chars().map(|c| c.len_utf16()).sum::<usize>() as u32)
        .unwrap_or(0)
}

/// Delta-encode sorted `(Class, byte range)` items (see gemcap `semantic_token`).
fn encode(rope: &Rope, items: &[(Class, Range<usize>)]) -> Vec<SemanticToken> {
    let mut out = Vec::with_capacity(items.len());
    let mut prev = 0..0;
    for (class, range) in items {
        let prev_line = rope.byte_to_line(prev.start.min(rope.len_bytes()));
        let cur_line = rope.byte_to_line(range.start.min(rope.len_bytes()));
        let delta_line = (cur_line.saturating_sub(prev_line)) as u32;
        let delta_start = if delta_line > 0 {
            let line_start = rope.line_to_byte(cur_line);
            utf16_len(rope, line_start..range.start.min(rope.len_bytes()))
        } else {
            utf16_len(rope, prev.start.min(rope.len_bytes())..range.start.min(rope.len_bytes()))
        };
        let length = utf16_len(rope, range.start.min(rope.len_bytes())..range.end.min(rope.len_bytes()));
        out.push(SemanticToken {
            delta_line,
            delta_start,
            length,
            token_type: *class as u32,
            token_modifiers_bitset: 0,
        });
        prev = range.clone();
    }
    out
}

/// Is `range` fully inside one of `regions`?
fn inside(range: &Range<usize>, regions: &[Range<usize>]) -> bool {
    regions.iter().any(|r| range.start >= r.start && range.end <= r.end)
}

/// Compute semantic tokens for a document.
///
/// `root`/`tokens` are borrowed from the repository (hold its read-lock while
/// calling); `rope` is the open-document buffer.
pub(crate) fn handle(
    rope: &Rope,
    root: Option<&Statement>,
    tokens: &[Token],
) -> Option<Vec<SemanticToken>> {
    let root = root?;

    let mut items: Vec<(Class, Range<usize>)> = Vec::new();
    let mut composite: Vec<Range<usize>> = Vec::new();

    // Pass 1 — structural.
    for stmt in root.preorder() {
        if let Some(kw) = &stmt.keyword {
            items.push((Class::Keyword, kw.clone()));
        }
        if let Some(arg) = &stmt.arg {
            match arg_semantics(&stmt.kind) {
                ArgSemantics::Atomic(class) => items.push((class, arg.range.clone())),
                ArgSemantics::Composite => composite.push(arg.range.clone()),
            }
        }
    }

    // Pass 2 — lexical (grammar tokens).
    for token in tokens {
        let class = match token.kind {
            TokenKind::Comment => Class::Comment,
            TokenKind::String => Class::String,
            TokenKind::Number => Class::Number,
            TokenKind::Boolean => Class::Keyword,
            TokenKind::Operator => Class::Operator,
            // Identifiers/keywords/punct are covered by pass 1 (or uncolored).
            _ => continue,
        };
        // Literals only inside composite argument spans; comments anywhere.
        if class == Class::Comment || inside(&token.range, &composite) {
            items.push((class, token.range.clone()));
        }
    }

    // Disjoint & sorted by construction; sort defensively, then drop overlaps.
    items.sort_by_key(|(_, r)| (r.start, r.end));
    let mut kept: Vec<(Class, Range<usize>)> = Vec::with_capacity(items.len());
    let mut end = 0usize;
    for item in items {
        if item.1.start >= end {
            end = item.1.end;
            kept.push(item);
        }
    }

    Some(encode(rope, &kept))
}

/// Wrap encoded token data in an LSP response.
pub(crate) fn result(data: Vec<SemanticToken>, version: i32) -> SemanticTokensResult {
    SemanticTokensResult::Tokens(SemanticTokens {
        result_id: Some(version.to_string()),
        data,
    })
}
