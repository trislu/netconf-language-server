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
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions, SemanticTokensResult,
    SemanticTokensServerCapabilities,
};
use yrepo::{Statement, StatementKind, Token, TokenKind};

use crate::config::{SemanticOverride, SemanticSettings};

/// The token classes we emit, in legend order (index == variant rank). The enum
/// is an **identical map to the standard LSP `SemanticTokenType` set**: every
/// standard token type is announced in the server capability, so any of them
/// can be picked per role in `netconf.semantic` without a client restart. A
/// class that no role currently maps to simply produces no tokens by default.
//
// `Class` is a deliberate mirror of the LSP token type `"class"` (the variant
// name therefore matches the enum name), so allow clippy's rename hint here.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum Class {
    Namespace = 0,
    Type = 1,
    Class = 2,
    Enum = 3,
    Interface = 4,
    Struct = 5,
    TypeParameter = 6,
    Parameter = 7,
    Variable = 8,
    Property = 9,
    EnumMember = 10,
    Event = 11,
    Function = 12,
    Method = 13,
    Macro = 14,
    Keyword = 15,
    Modifier = 16,
    Comment = 17,
    String = 18,
    Number = 19,
    Regexp = 20,
    Operator = 21,
    Decorator = 22,
}

impl Class {
    /// Every announced class, in legend order (index == variant rank).
    const ALL: [Class; 23] = [
        Class::Namespace,
        Class::Type,
        Class::Class,
        Class::Enum,
        Class::Interface,
        Class::Struct,
        Class::TypeParameter,
        Class::Parameter,
        Class::Variable,
        Class::Property,
        Class::EnumMember,
        Class::Event,
        Class::Function,
        Class::Method,
        Class::Macro,
        Class::Keyword,
        Class::Modifier,
        Class::Comment,
        Class::String,
        Class::Number,
        Class::Regexp,
        Class::Operator,
        Class::Decorator,
    ];

    /// The standard LSP tag (the `SemanticTokenType` name) of this class.
    fn name(self) -> &'static str {
        use Class as C;
        match self {
            C::Namespace => "namespace",
            C::Type => "type",
            C::Class => "class",
            C::Enum => "enum",
            C::Interface => "interface",
            C::Struct => "struct",
            C::TypeParameter => "typeParameter",
            C::Parameter => "parameter",
            C::Variable => "variable",
            C::Property => "property",
            C::EnumMember => "enumMember",
            C::Event => "event",
            C::Function => "function",
            C::Method => "method",
            C::Macro => "macro",
            C::Keyword => "keyword",
            C::Modifier => "modifier",
            C::Comment => "comment",
            C::String => "string",
            C::Number => "number",
            C::Regexp => "regexp",
            C::Operator => "operator",
            C::Decorator => "decorator",
        }
    }

    /// Parse a class from its standard LSP name (a `netconf.semantic` value).
    fn from_name(name: &str) -> Option<Class> {
        Class::ALL.iter().copied().find(|c| c.name() == name)
    }

    fn lsp_type(self) -> SemanticTokenType {
        SemanticTokenType::new(self.name())
    }
}

/// The token modifiers that can ride along with a role's token, mirroring the
/// standard LSP `SemanticTokenModifier` set in legend order (index == variant
/// rank → `token_modifiers_bitset` bit). All are announced, so any can be
/// picked per role. The built-in classification only uses `readonly` — the
/// const proxy on constant `units` values (LSP has no `const` tag).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum Modifier {
    Declaration = 0,
    Definition = 1,
    Readonly = 2,
    Static = 3,
    Deprecated = 4,
    Abstract = 5,
    Async = 6,
    Modification = 7,
    Documentation = 8,
    DefaultLibrary = 9,
}

impl Modifier {
    /// Every announced modifier, in legend order (index == variant rank).
    const ALL: [Modifier; 10] = [
        Modifier::Declaration,
        Modifier::Definition,
        Modifier::Readonly,
        Modifier::Static,
        Modifier::Deprecated,
        Modifier::Abstract,
        Modifier::Async,
        Modifier::Modification,
        Modifier::Documentation,
        Modifier::DefaultLibrary,
    ];

    /// The standard LSP tag (the `SemanticTokenModifier` name) of this modifier.
    fn name(self) -> &'static str {
        use Modifier as M;
        match self {
            M::Declaration => "declaration",
            M::Definition => "definition",
            M::Readonly => "readonly",
            M::Static => "static",
            M::Deprecated => "deprecated",
            M::Abstract => "abstract",
            M::Async => "async",
            M::Modification => "modification",
            M::Documentation => "documentation",
            M::DefaultLibrary => "defaultLibrary",
        }
    }

    /// Parse a modifier from its standard LSP name (a config value).
    fn from_name(name: &str) -> Option<Modifier> {
        Modifier::ALL.iter().copied().find(|m| m.name() == name)
    }

    /// This modifier's bit in a `token_modifiers_bitset`.
    const fn bit(self) -> u32 {
        1 << self as u32
    }

    /// Fold a set of modifiers into a `token_modifiers_bitset`.
    fn bits(mods: &[Modifier]) -> u32 {
        mods.iter().fold(0, |acc, m| acc | m.bit())
    }

    fn lsp(self) -> SemanticTokenModifier {
        SemanticTokenModifier::new(self.name())
    }
}

/// Bitset bit for the `readonly` (constant) modifier used by `units`.
const MOD_CONST: u32 = Modifier::Readonly.bit();

pub(crate) fn capability() -> SemanticTokensServerCapabilities {
    SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
        legend: SemanticTokensLegend {
            token_types: Class::ALL.iter().map(|c| c.lsp_type()).collect(),
            token_modifiers: Modifier::ALL.iter().map(|m| m.lsp()).collect(),
        },
        full: Some(SemanticTokensFullOptions::Bool(true)),
        ..Default::default()
    })
}

/// A *role*: **why** a span is colored. Each role is a member of the
/// `netconf.semantic` configuration object (see [`Role::key`]); its value
/// picks a [`Class`] (any standard token type) and optional [`Modifier`]s, so
/// a user can retarget any construct without a rule engine. Every role has a
/// built-in default [`Class`] (and, for constant `units`, the `readonly`
/// modifier) that applies until configured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// Module/submodule/import/prefix *names* (D4 map: namespace).
    ModuleName,
    /// `type` / `uses` / `base` / `refine` reference arguments.
    TypeRef,
    /// `deviate add|replace|delete|not-supported` verb words.
    DeviateVerb,
    /// `revision` / `revision-date` date arguments.
    DateString,
    /// `range` / `length` string arguments (config key: `rangeLength`).
    RangeLength,
    /// `pattern` arguments — regex strings, colored `regexp` by default
    /// (config key: `patternArg`).
    PatternArg,
    /// `key` / `unique` / (unquoted) `augment` string arguments.
    ListRefString,
    /// Data-node definition names (container/leaf/list/…/case).
    DataName,
    /// Other definition names (grouping/typedef/identity/feature/extension).
    DefName,
    /// `enum` / `bit` names (config key: `enumAndBitNames`).
    EnumBitName,
    /// A bare signed number argument (`default -10;`, `value -1;`, …).
    NumberLiteral,
    /// Unquoted reference/value words (`if-feature foo`, `default lo`, …).
    WordArg,
    /// Unquoted vendor-extension arguments (`StatementKind::Unknown`),
    /// config key `vendorExtension`.
    VendorExtArg,
    /// Unquoted constant `units` values (Variable + `readonly` by default),
    /// config key `units`.
    UnitsConst,
    /// Comments (grammar tokens, colored anywhere).
    Comment,
    /// Quoted strings inside composite arguments.
    StringLit,
    /// Numbers inside composite arguments.
    NumberLit,
    /// `true`/`false` value words (colored anywhere).
    BooleanLit,
    /// Value keywords inside composite arguments (`status`, `ordered-by`, …).
    KeywordLit,
    /// `+` string-concatenation operators.
    OperatorLit,
}

impl Role {
    const ALL: [Role; 20] = [
        Role::ModuleName,
        Role::TypeRef,
        Role::DeviateVerb,
        Role::DateString,
        Role::RangeLength,
        Role::PatternArg,
        Role::ListRefString,
        Role::DataName,
        Role::DefName,
        Role::EnumBitName,
        Role::NumberLiteral,
        Role::WordArg,
        Role::VendorExtArg,
        Role::UnitsConst,
        Role::Comment,
        Role::StringLit,
        Role::NumberLit,
        Role::BooleanLit,
        Role::KeywordLit,
        Role::OperatorLit,
    ];

    /// The `netconf.semantic` member name for this role — every role is a key
    /// of the configuration object, so any construct can be retargeted:
    ///
    /// `moduleName`, `typeRef`, `deviateVerb`, `dateString`, `rangeLength`,
    /// `patternArg`, `keyUniqueAugment`, `dataNodeName`, `definitionName`,
    /// `enumAndBitNames`, `numberArgument`, `referenceWord`, `vendorExtension`,
    /// `units`, `comment`, `stringLiteral`, `numberLiteral`, `boolean`,
    /// `valueKeyword`, `operator`.
    fn key(self) -> &'static str {
        use Role as R;
        match self {
            R::ModuleName => "moduleName",
            R::TypeRef => "typeRef",
            R::DeviateVerb => "deviateVerb",
            R::DateString => "dateString",
            R::RangeLength => "rangeLength",
            R::PatternArg => "patternArg",
            R::ListRefString => "keyUniqueAugment",
            R::DataName => "dataNodeName",
            R::DefName => "definitionName",
            R::EnumBitName => "enumAndBitNames",
            R::NumberLiteral => "numberArgument",
            R::WordArg => "referenceWord",
            R::VendorExtArg => "vendorExtension",
            R::UnitsConst => "units",
            R::Comment => "comment",
            R::StringLit => "stringLiteral",
            R::NumberLit => "numberLiteral",
            R::BooleanLit => "boolean",
            R::KeywordLit => "valueKeyword",
            R::OperatorLit => "operator",
        }
    }

    /// Parse a role from its config member name. `None` for an unknown key
    /// (future-proofing — such keys are ignored, the role keeps its default).
    fn from_key(key: &str) -> Option<Role> {
        Role::ALL.iter().copied().find(|r| r.key() == key)
    }

    /// The class a role uses when nothing overrides it.
    fn default_class(self) -> Class {
        use Role as R;
        match self {
            R::ModuleName => Class::Namespace,
            R::TypeRef => Class::Type,
            R::DeviateVerb | R::BooleanLit | R::KeywordLit => Class::Keyword,
            R::DateString | R::RangeLength | R::ListRefString | R::StringLit => Class::String,
            R::PatternArg => Class::Regexp,
            R::DataName
            | R::DefName
            | R::EnumBitName
            | R::WordArg
            | R::VendorExtArg
            | R::UnitsConst => Class::Variable,
            R::NumberLiteral | R::NumberLit => Class::Number,
            R::Comment => Class::Comment,
            R::OperatorLit => Class::Operator,
        }
    }

    /// The modifiers a role carries by default — `readonly` (constant) for
    /// `units` (REQ4), none otherwise.
    fn default_mods(self) -> u32 {
        match self {
            Role::UnitsConst => MOD_CONST,
            _ => 0,
        }
    }
}

/// Which statement kinds carry an *atomic* (whole-argument) role — one token
/// over the argument span. `None` means the argument is composite and its
/// inner literals (string/number/bool/`+`) are lexed instead.
fn role_of_kind(kind: &StatementKind) -> Option<Role> {
    use Role as R;
    use StatementKind as K;
    Some(match kind {
        // Names: modules/prefixes.
        K::Module | K::Submodule | K::Import | K::Include | K::BelongsTo | K::Prefix => {
            R::ModuleName
        }
        // Type / grouping / identity references.
        K::Type | K::Uses | K::Base | K::Refine => R::TypeRef,
        // `deviate` verb words (`add` / `replace` / `delete` / `not-supported`)
        // are colored like the keyword that introduces them.
        K::DeviateAdd | K::DeviateDelete | K::DeviateReplace | K::DeviateNotSupported => {
            R::DeviateVerb
        }
        // Dates: `revision` / `revision-date` take a bare `YYYY-MM-DD` string.
        K::Revision | K::RevisionDate => R::DateString,
        // Range / length expressions are string-valued per RFC; color the whole
        // argument (quoted or bare) as a string. Lexing their inner boundaries
        // instead would leave `range "-16..-1"` uncolored: the grammar hides
        // the digits of signed numbers inside the argument token, so only `-`,
        // `..` and quotes survive as leaves.
        K::Range | K::Length => R::RangeLength,
        // `pattern` takes a quoted regex string; color the whole argument as a
        // regexp (mirrors `range`/`length`, which also color whole arguments).
        K::Pattern => R::PatternArg,
        // Key/unique member lists and (unquoted) augment target paths are
        // colored as one string for now (REQ1/REQ5); leaf-goto from them is
        // a deferred idea (see TODO.md).
        K::Key | K::Unique | K::Augment => R::ListRefString,
        // Data nodes.
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
        | K::Notification => R::DataName,
        // Definitions.
        K::Grouping | K::Typedef | K::Identity | K::Feature | K::Extension => R::DefName,
        // Enum/bit members.
        K::Bit | K::Enum => R::EnumBitName,
        _ => return None,
    })
}

/// UTF-16 length of a byte range.
fn utf16_len(rope: &Rope, range: Range<usize>) -> u32 {
    rope.get_byte_slice(range)
        .map(|s| s.chars().map(|c| c.len_utf16()).sum::<usize>() as u32)
        .unwrap_or(0)
}

/// Delta-encode sorted `(Class, modifier-bits, byte range)` items.
///
/// LSP semantic tokens are **single-line**: a token may not span a line break
/// (`@types/vscode` `SemanticTokens`: "A token cannot be multiline."). Items
/// produced by the passes may legitimately cover several lines (multi-line
/// double-quoted strings, `/* … */` block comments), so each item is split into
/// one token **per line** before delta-encoding. Within a line, `delta_start`
/// is measured from the previous token's start; on a new line it is measured
/// from column 0 (see VS Code `SemanticTokens` encoding docs).
fn encode(rope: &Rope, items: &[(Class, u32, Range<usize>)]) -> Vec<SemanticToken> {
    let mut out = Vec::with_capacity(items.len());
    // Position (start-to-start) of the previously emitted token.
    let mut prev_line = 0u32;
    let mut prev_col = 0u32;
    let last_line = (rope.len_lines().saturating_sub(1)) as u32;

    for (class, mods, range) in items {
        let end = range.end.min(rope.len_bytes());
        let mut start = range.start.min(rope.len_bytes());
        if end <= start {
            continue;
        }
        let mut line = (rope.byte_to_line(start) as u32).min(last_line);
        loop {
            let line_start = rope.line_to_byte(line as usize);
            // Byte offset just past this line's content (line terminator excluded).
            let content_end = line_start + rope.line(line as usize).len_bytes();
            let seg_start = start.max(line_start);
            let seg_end = end.min(content_end);
            if seg_end > seg_start {
                let col = utf16_len(rope, line_start..seg_start);
                let delta_line = line.saturating_sub(prev_line);
                let delta_start = if delta_line > 0 {
                    col
                } else {
                    col.saturating_sub(prev_col)
                };
                let length = utf16_len(rope, seg_start..seg_end);
                out.push(SemanticToken {
                    delta_line,
                    delta_start,
                    length,
                    token_type: *class as u32,
                    token_modifiers_bitset: *mods,
                });
                prev_line = line;
                prev_col = col;
            }
            if end <= content_end {
                break;
            }
            if line >= last_line {
                break;
            }
            line += 1;
            start = rope.line_to_byte(line as usize);
        }
    }
    out
}

/// Is `range` fully inside one of `regions`?
fn inside(range: &Range<usize>, regions: &[Range<usize>]) -> bool {
    regions
        .iter()
        .any(|r| range.start >= r.start && range.end <= r.end)
}

/// Does `raw` (the source text of a statement argument) form a single bare
/// (unquoted) signed number literal?
///
/// The grammar lexes a leading `-` as its own anonymous token and merges the
/// following digits into the enclosing argument token, so those digits never
/// appear as a leaf — the lexical pass can never see `-10` as a number. Color
/// such an argument whole instead.
fn is_bare_number(raw: &str) -> bool {
    let t = raw.trim();
    if t.is_empty() || t.starts_with('"') || t.starts_with('\'') {
        return false;
    }
    let t = t
        .strip_prefix('+')
        .or_else(|| t.strip_prefix('-'))
        .unwrap_or(t);
    let (int_part, frac_part) = match t.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (t, None),
    };
    let digits = |x: &str| !x.is_empty() && x.bytes().all(|b| b.is_ascii_digit());
    digits(int_part) && frac_part.map(digits).unwrap_or(true)
}

/// Is `raw` a single unquoted word (no quotes, no inner whitespace)? The
/// argument-string rule: quoted → string (composite pass); unquoted single
/// words (identifiers, `urn:…` URIs, `calls/second`-style values) are colored
/// as a name/variable.
fn is_unquoted_word(raw: &str) -> bool {
    let t = raw.trim();
    !t.is_empty()
        && !t.starts_with('"')
        && !t.starts_with('\'')
        && !t.contains(|c: char| c.is_whitespace())
}

/// The *role* for a composite argument whose raw text is a single literal.
/// Returns `None` to fall back to the composite (inner-literal) coloring.
///
/// - numbers (`default -10;`, `value -1;`, …) keep their own role;
/// - an unquoted reference/value argument is a `WordArg` (colored like a
///   `typedef` name): `if-feature foo;` (REQ2), `default disabled;` (REQ3),
///   `argument name;` (REQ6), and unquoted `namespace` URIs;
/// - an unquoted vendor-extension argument (`StatementKind::Unknown`) is a
///   `VendorExtArg`;
/// - an unquoted `units` value is the constant `UnitsConst` role (REQ4).
fn whole_arg_role(kind: &StatementKind, raw: &str) -> Option<Role> {
    use Role as R;
    use StatementKind as K;
    if is_bare_number(raw) {
        return Some(R::NumberLiteral);
    }
    if !is_unquoted_word(raw) {
        return None;
    }
    match kind {
        K::Unknown(_) => Some(R::VendorExtArg),
        K::IfFeature | K::Default | K::Argument | K::Namespace => Some(R::WordArg),
        K::Units => Some(R::UnitsConst),
        _ => None,
    }
}

/// A parsed `netconf.semantic.<role>` value. Fields are `None` when the config
/// did not mention them (the role's built-in value is kept).
type ParsedOverride = (Option<Class>, Option<Vec<Modifier>>);

/// Validate one `netconf.semantic.<role>` value.
///
/// Accepts the shorthand string form (`"enumMember"` — token only, modifiers
/// untouched) or the full `{ "token", "modifiers" }` object form (either field
/// optional; an empty `modifiers` array clears the role's modifiers). Returns
/// `None` when a *present* field names an unknown token type or modifier — the
/// whole override is then ignored and the role keeps its built-in
/// classification.
fn parse_override(value: &SemanticOverride) -> Option<ParsedOverride> {
    match value {
        SemanticOverride::Type(name) => Some((Some(Class::from_name(name)?), None)),
        SemanticOverride::Fields(f) => {
            let token = match &f.token {
                Some(name) => Some(Class::from_name(name)?),
                None => None,
            };
            let modifiers = match &f.modifiers {
                None => None,
                Some(list) => Some(
                    list.iter()
                        .map(|name| Modifier::from_name(name))
                        .collect::<Option<Vec<Modifier>>>()?,
                ),
            };
            Some((token, modifiers))
        }
    }
}

/// The effective per-role classification for one document, resolved from the
/// `netconf.semantic` per-role struct. A role the config does not mention
/// keeps its built-in `(class, modifier-bits)`; a mentioned role takes the
/// configured token type and/or modifiers.
#[derive(Debug, Clone)]
pub(crate) struct Style {
    emit: [Option<(Class, u32)>; Role::ALL.len()],
}

impl Default for Style {
    fn default() -> Self {
        Style::from_settings(&SemanticSettings::default())
    }
}

impl Style {
    /// Resolve the config into one emission per role. Called once per
    /// `semantic_tokens_full` request (cheap) and never cached, so a
    /// `didChangeConfiguration` takes effect on the next request with no
    /// recompile and — because the legend announces the full standard token
    /// type + modifier sets — no client restart.
    fn from_settings(s: &SemanticSettings) -> Style {
        let mut emit: [Option<(Class, u32)>; Role::ALL.len()] = std::array::from_fn(|_| None);
        // Start from each role's built-in classification…
        for (i, role) in Role::ALL.iter().enumerate() {
            emit[i] = Some((role.default_class(), role.default_mods()));
        }
        // …then apply the configured members. Unknown role keys are ignored
        // (future-proofing); a member whose token type / modifier is unknown is
        // ignored too, leaving that role's built-in classification.
        for (key, value) in &s.roles {
            let Some(role) = Role::from_key(key) else {
                continue;
            };
            let Some((token, modifiers)) = parse_override(value) else {
                continue;
            };
            let (mut class, mut mods) = emit[role as usize].expect("role present");
            if let Some(c) = token {
                class = c;
            }
            if let Some(list) = modifiers {
                mods = Modifier::bits(&list);
            }
            emit[role as usize] = Some((class, mods));
        }
        Style { emit }
    }

    /// Emission for a role: its configured (or built-in) class and modifier
    /// bits. Always `Some` — a role always maps to a token type.
    fn at(&self, role: Role) -> Option<(Class, u32)> {
        self.emit[role as usize]
    }
}

/// Compute semantic tokens for a document.
///
/// `root`/`tokens` are borrowed from the repository (hold its read-lock while
/// calling); `rope` is the open-document buffer; `settings` is the active
/// `netconf.semantic` configuration (a default value reproduces the historic
/// classification exactly).
pub(crate) fn handle(
    rope: &Rope,
    root: Option<&Statement>,
    tokens: &[Token],
    settings: &SemanticSettings,
) -> Option<Vec<SemanticToken>> {
    let root = root?;
    let style = Style::from_settings(settings);

    // Declarations carrying a direct `status deprecated;` child are deprecated
    // (always-on). Record their spans so every token of the declaration —
    // keyword, name, the `status deprecated;` marker's own words, and every
    // descendant — carries the standard `deprecated` modifier.
    let mut deprecated: Vec<Range<usize>> = Vec::new();
    for stmt in root.preorder() {
        for child in &stmt.children {
            if child.kind == StatementKind::Status
                && child.arg.as_ref().is_some_and(|a| a.name() == "deprecated")
            {
                deprecated.push(stmt.range.clone());
                break;
            }
        }
    }

    let mut items: Vec<(Class, u32, Range<usize>)> = Vec::new();
    let mut composite: Vec<Range<usize>> = Vec::new();

    // Pass 1 — structural.
    for stmt in root.preorder() {
        if let Some(kw) = &stmt.keyword {
            items.push((Class::Keyword, 0, kw.clone()));
        }
        if let Some(arg) = &stmt.arg {
            if let Some(role) = role_of_kind(&stmt.kind) {
                // Atomic whole-argument span (name / ref / verb / date / …).
                if let Some((class, mods)) = style.at(role) {
                    items.push((class, mods, arg.range.clone()));
                }
            } else {
                // Composite argument: color the whole literal when it reads as
                // one (number / unquoted word / units), else let the lexical
                // pass color its inner literals.
                let raw = rope
                    .get_byte_slice(arg.range.clone())
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                if let Some(role) = whole_arg_role(&stmt.kind, &raw)
                    && let Some((class, mods)) = style.at(role)
                {
                    items.push((class, mods, arg.range.clone()));
                } else {
                    composite.push(arg.range.clone());
                }
            }
        }
    }

    // Pass 2 — lexical (grammar tokens).
    for token in tokens {
        let role = match token.kind {
            TokenKind::Comment => Role::Comment,
            TokenKind::String => Role::StringLit,
            TokenKind::Number => Role::NumberLit,
            TokenKind::Boolean => Role::BooleanLit,
            TokenKind::Operator => Role::OperatorLit,
            // Value words that read as keywords and live inside composite
            // argument spans (`status deprecated`, `ordered-by user`, `min`/
            // `max` in range/length) are lexed as keywords; color them only
            // there. Statement keywords sit *outside* composite args and are
            // already colored by pass 1, so the `inside(composite)` guard
            // below never double-colors them.
            TokenKind::Keyword => Role::KeywordLit,
            // Identifiers/punct are covered by pass 1 (or uncolored).
            _ => continue,
        };
        let Some((class, mods)) = style.at(role) else {
            continue;
        };
        // Comments, and booleans (`true`/`false` — quoted forms like
        // `config "false"` sit outside the composite span because the arg
        // node is then just the opening quote), are colored anywhere; all
        // other literals only inside composite argument spans.
        let standalone = matches!(token.kind, TokenKind::Comment | TokenKind::Boolean);
        if standalone || inside(&token.range, &composite) {
            items.push((class, mods, token.range.clone()));
        }
    }

    // Disjoint & sorted by construction; sort defensively, then drop overlaps.
    items.sort_by_key(|(_, _, r)| (r.start, r.end));
    let mut kept: Vec<(Class, u32, Range<usize>)> = Vec::with_capacity(items.len());
    let mut end = 0usize;
    for item in items {
        if item.2.start >= end {
            end = item.2.end;
            kept.push(item);
        }
    }

    // Always-on deprecation: OR the `deprecated` modifier into every kept token
    // inside a deprecated declaration's span — including the `status
    // deprecated;` marker's own words (see the note above).
    if !deprecated.is_empty() {
        let dep_bit = Modifier::Deprecated.bit();
        for item in &mut kept {
            let inside = |r: &Range<usize>| item.2.start >= r.start && item.2.end <= r.end;
            if deprecated.iter().any(inside) {
                item.1 |= dep_bit;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use crate::config::SemanticOverrideFields;

    /// Decode the delta stream back to absolute `(line, utf16 col, length, type)`
    /// using the VS Code convention (start-to-start deltas on the same line).
    fn decode(out: &[SemanticToken]) -> Vec<(u32, u32, u32, u32)> {
        let mut abs = Vec::with_capacity(out.len());
        let mut line = 0u32;
        let mut col = 0u32;
        for t in out {
            if t.delta_line > 0 {
                line += t.delta_line;
                col = t.delta_start;
            } else {
                col += t.delta_start;
            }
            abs.push((line, col, t.length, t.token_type));
        }
        abs
    }

    /// Semantic tokens over an inline module, plus decoded absolute positions.
    fn tokens_for(src: &str) -> (Rope, Vec<(u32, u32, u32, u32)>) {
        let rope = Rope::from_str(src);
        let mut repo = yrepo::Repository::new();
        repo.upsert(String::from("u"), src.to_string());
        repo.compile();
        let root = repo.statement("u");
        let toks = repo.tokens("u").unwrap();
        let out = handle(&rope, root, toks, &SemanticSettings::default()).expect("tokens");
        let abs = decode(&out);
        (rope, abs)
    }

    /// A `+`-concatenated quoted argument (`namespace "…" + "…"`) must color
    /// each quoted segment as a String and the `+` as an Operator (regression:
    /// these were previously not highlighted).
    #[test]
    fn concatenated_namespace_highlights_each_string_and_plus() {
        let src = "module n {\n  namespace \"http://foo/bar/\"\n          + \"qux\";\n}\n";
        let (rope, abs) = tokens_for(src);
        let spans: Vec<(String, u32)> = abs
            .iter()
            .map(|&(l, c, len, ty)| (seg_text(&rope, l, c, len), ty))
            .collect();
        let first = "\"http://foo/bar/\"";
        let second = "\"qux\"";
        let detail = |need: &str| {
            format!(
                "missing {need:?} token; emitted spans:\n{spans:#?}",
                spans = spans
            )
        };
        assert!(
            spans
                .iter()
                .any(|(t, ty)| t == first && *ty == Class::String as u32),
            "{}",
            detail(first)
        );
        assert!(
            spans
                .iter()
                .any(|(t, ty)| t == second && *ty == Class::String as u32),
            "{}",
            detail(second)
        );
        assert!(
            spans
                .iter()
                .any(|(t, ty)| t == "+" && *ty == Class::Operator as u32),
            "{}",
            detail("+")
        );
    }

    fn line_utf16_len(rope: &Rope, line: u32) -> u32 {
        let i = (line as usize).min(rope.len_lines().saturating_sub(1));
        rope.line(i).chars().map(|c| c.len_utf16() as u32).sum()
    }

    /// Text covered by an absolute token (converts the UTF-16 column to bytes).
    fn seg_text(rope: &Rope, line: u32, col: u32, len: u32) -> String {
        let i = (line as usize).min(rope.len_lines().saturating_sub(1));
        let line_txt: String = rope.line(i).to_string();
        let mut utf = 0u32;
        let mut byte = 0usize;
        for ch in line_txt.chars() {
            if utf >= col {
                break;
            }
            utf += ch.len_utf16() as u32;
            byte += ch.len_utf8();
        }
        let start = rope.line_to_byte(i) + byte;
        let end = (start + (len as usize)).min(rope.len_bytes());
        rope.get_byte_slice(start..end)
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// Byte range of an absolute `(line, utf16 col, utf16 len)` token.
    fn abs_byte_range(rope: &Rope, line: u32, col: u32, len: u32) -> Range<usize> {
        let i = (line as usize).min(rope.len_lines().saturating_sub(1));
        let line_start = rope.line_to_byte(i);
        let txt: String = rope.line(i).to_string();
        let mut utf: u32 = 0;
        let mut pos = 0usize;
        let mut s: Option<usize> = None;
        let mut e: Option<usize> = None;
        let tok_end = col + len;
        for ch in txt.chars() {
            let cl = ch.len_utf16() as u32;
            // A char belongs to the token when its UTF-16 window overlaps it.
            if cl > 0 && utf < tok_end && utf + cl > col {
                if s.is_none() {
                    s = Some(line_start + pos);
                }
                e = Some(line_start + pos + ch.len_utf8());
            }
            utf += cl;
            pos += ch.len_utf8();
        }
        let s = s.unwrap_or(line_start);
        let e = e.unwrap_or(s);
        s..e
    }

    /// Raw token is a *word* (has an alphanumeric run): identifiers, keywords,
    /// numbers, string contents. Pure punctuation/whitespace is excluded —
    /// those are never semantically highlighted by design.
    fn is_word(tok: &yrepo::Token) -> bool {
        tok.text.chars().any(|c| c.is_alphanumeric() || c == '_')
    }

    /// Walk `dir` (recursively) collecting `*.yang` file paths.
    fn walk_yang(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk_yang(&p, out);
                } else if p.extension().is_some_and(|x| x == "yang") {
                    out.push(p);
                }
            }
        }
    }

    /// One unhighlighted word token found in the corpus.
    struct Gap {
        file: String,
        line: u32,
        text: String,
        kind: yrepo::TokenKind,
        /// Debug name of the narrowest enclosing statement.
        stmt: String,
        /// Whether the token sits inside that statement's argument (vs body).
        in_arg: bool,
    }

    /// Narrowest statement containing `byte` (later preorder hit = deepest).
    fn enclosing<'a>(root: &'a yrepo::Statement, byte: usize) -> (String, bool) {
        let mut best: Option<&'a yrepo::Statement> = None;
        for s in root.preorder() {
            if byte >= s.range.start && byte < s.range.end {
                best = Some(s);
            }
        }
        match best {
            Some(s) => {
                let in_arg = s
                    .arg
                    .as_ref()
                    .is_some_and(|a| byte >= a.range.start && byte < a.range.end);
                (format!("{:?}", s.kind), in_arg)
            }
            None => (String::from("?"), false),
        }
    }

    /// Compute highlight coverage over every file in `root`. Uses the *real*
    /// production path ([`handle`]) so the report matches what the editor
    /// shows. Returns `(parsed files, total word tokens, gaps)`.
    fn coverage_gaps(root: &Path) -> (usize, usize, Vec<Gap>) {
        let mut files = Vec::new();
        walk_yang(root, &mut files);
        files.sort();

        // Parse the whole workspace once (like the server does).
        let mut repo = yrepo::Repository::new();
        let mut docs: Vec<(PathBuf, Rope)> = Vec::new();
        for f in &files {
            let Ok(text) = std::fs::read_to_string(f) else {
                continue;
            };
            repo.upsert(f.to_string_lossy().to_string(), text.clone());
            docs.push((f.clone(), Rope::from_str(&text)));
        }
        repo.compile();

        let mut total_words = 0usize;
        let mut gaps: Vec<Gap> = Vec::new();

        for (path, rope) in &docs {
            let url = path.to_string_lossy().to_string();
            let root_stmt = repo.statement(&url);
            let Some(toks) = repo.tokens(&url) else {
                continue;
            };

            // Decoded semantic token byte ranges for this document.
            let sem = match handle(rope, root_stmt, toks, &SemanticSettings::default()) {
                Some(out) => {
                    let abs = decode(&out);
                    let mut v: Vec<Range<usize>> = abs
                        .iter()
                        .map(|&(l, c, n, _)| abs_byte_range(rope, l, c, n))
                        .collect();
                    v.sort_by_key(|r| (r.start, r.end));
                    // merge
                    let mut merged: Vec<Range<usize>> = Vec::new();
                    for r in v {
                        if let Some(last) = merged.last_mut()
                            && r.start <= last.end
                        {
                            last.end = last.end.max(r.end);
                            continue;
                        }
                        merged.push(r);
                    }
                    merged
                }
                None => Vec::new(),
            };

            let covered = |r: &Range<usize>| -> bool {
                sem.iter().any(|x| x.start <= r.start && x.end >= r.end)
            };

            for t in toks {
                if !is_word(t) {
                    continue;
                }
                total_words += 1;
                if covered(&t.range) {
                    continue;
                }
                let line0 = rope.byte_to_line(t.range.start);
                let (stmt, in_arg) = match root_stmt {
                    Some(r) => enclosing(r, t.range.start),
                    None => (String::from("(no root)"), false),
                };
                gaps.push(Gap {
                    file: path
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    line: line0 as u32 + 1,
                    text: t.text.clone(),
                    kind: t.kind,
                    stmt,
                    in_arg,
                });
            }
        }
        (docs.len(), total_words, gaps)
    }

    /// Aggregate `gaps` into markdown tables and render the report.
    fn render_report(
        out_path: &Path,
        root: &Path,
        docs: usize,
        total_words: usize,
        gaps: &[Gap],
    ) -> String {
        let mut by_kind: BTreeMap<String, usize> = BTreeMap::new();
        let mut by_stmt: BTreeMap<String, usize> = BTreeMap::new();
        let mut by_file: BTreeMap<String, usize> = BTreeMap::new();
        for g in gaps {
            *by_kind.entry(g.kind.as_str().to_string()).or_default() += 1;
            *by_stmt
                .entry(format!(
                    "{}{}",
                    g.stmt,
                    if g.in_arg { " (arg)" } else { "" }
                ))
                .or_default() += 1;
            *by_file.entry(g.file.clone()).or_default() += 1;
        }
        let uncovered = gaps.len();

        let mut by_stmt: Vec<_> = by_stmt.into_iter().collect();
        by_stmt.sort_by_key(|a| std::cmp::Reverse(a.1));
        let mut by_file: Vec<_> = by_file.into_iter().collect();
        by_file.sort_by_key(|a| std::cmp::Reverse(a.1));

        let mut lines: Vec<String> = Vec::new();
        lines.push("# YANG semantic-token coverage report".into());
        lines.push(String::new());
        lines.push(
            "Scope: the `yang` corpus used by the netconf-language-server tests. Each file's text is \
             tokenized with `yrepo` and highlighted with the exact production pass \
             (`semantic_token::handle`); a **word token** (identifier / keyword / number / \
             boolean / quoted string — punctuation and whitespace are never semantically \
             colored) is counted as **uncovered** when no semantic token covers its span."
                .into(),
        );
        lines.push(String::new());
        lines.push("| metric | value |".into());
        lines.push("|---|---:|".into());
        lines.push(format!("| corpus dir | `{}` |", root.display()));
        lines.push(format!("| *.yang files parsed | {docs} |"));
        lines.push(format!("| word tokens | {total_words} |"));
        lines.push(format!(
            "| **uncovered word tokens** | **{uncovered} ({:.2}%)** |",
            if total_words == 0 {
                0.0
            } else {
                uncovered as f64 * 100.0 / total_words as f64
            }
        ));
        lines.push(String::new());

        lines.push("## By raw token kind".into());
        lines.push(String::new());
        lines.push("| kind | uncovered |".into());
        lines.push("|---|---:|".into());
        for (k, v) in by_kind {
            lines.push(format!("| {k} | {v} |"));
        }
        lines.push(String::new());

        lines.push("## By enclosing statement".into());
        lines.push(String::new());
        lines.push("| statement | uncovered |".into());
        lines.push("|---|---:|".into());
        for (k, v) in &by_stmt {
            lines.push(format!("| `{k}` | {v} |"));
        }
        lines.push(String::new());

        lines.push("## Files with most uncovered tokens".into());
        lines.push(String::new());
        lines.push("| file | uncovered |".into());
        lines.push("|---|---:|".into());
        for (k, v) in by_file.into_iter().take(25) {
            lines.push(format!("| {k} | {v} |"));
        }
        lines.push(String::new());

        lines.push("## Representative examples (a few per context)".into());
        lines.push(String::new());
        lines.push("```".into());
        let rank = |g: &Gap| {
            by_stmt
                .iter()
                .position(|(k, _)| {
                    *k == format!("{}{}", g.stmt, if g.in_arg { " (arg)" } else { "" })
                })
                .unwrap_or(usize::MAX)
        };
        let mut examples: Vec<&Gap> = gaps.iter().collect();
        examples.sort_by_key(|g| rank(g));
        let mut shown: BTreeMap<String, usize> = BTreeMap::new();
        for g in examples {
            let key = format!("{}{}", g.stmt, if g.in_arg { " (arg)" } else { "" });
            let n = shown.entry(key).or_insert(0);
            if *n >= 5 {
                continue;
            }
            *n += 1;
            let where_ = if g.in_arg { "arg" } else { "body" };
            lines.push(format!(
                "{}:{}  [{:?} @ {}:{}]  {:?}",
                g.file,
                g.line,
                g.kind.as_str(),
                g.stmt,
                where_,
                g.text
            ));
        }
        lines.push("```".into());
        lines.push(String::new());

        lines.push("## Observations".into());
        lines.push(String::new());
        let obs: &[&str] = &[
            "An \"uncovered\" word token means *no semantic token* covers it. VS Code still",
            "colors the base token via the TextMate grammar on top of semantic tokens, so many",
            "of these (identifiers, plain words) are visible but use the default text color.",
            "",
            "All previously-identified gap families are now coloured:",
            "* `key` / `unique` member lists and (unquoted) `augment` target paths are whole",
            "  strings (REQ1/REQ5);",
            "* `if-feature` refs, `default` refs, `argument` names and extension bare-id args",
            "  are coloured like a `typedef` name (REQ2/REQ3/REQ6);",
            "* unquoted `units` values are `Variable` + `readonly` (const) (REQ4).",
            "",
            "No word tokens are left uncovered: unquoted `namespace` URIs and slash-containing",
            "`units` / `default` values are coloured as variables under the same quoted/unquoted",
            "argument-string rule — 100% coverage on this corpus.",
        ];
        for l in obs {
            lines.push((*l).to_string());
        }
        lines.push(String::new());
        lines.push(format!(
            "_(regenerated by `semantic_token::tests::generate_highlight_report` into `{}`)_",
            out_path.display()
        ));
        lines.join("\n")
    }

    #[test]
    #[ignore = "regenerates a highlight report for an arbitrary YANG corpus; run explicitly"]
    fn generate_highlight_report() {
        // No hardcoded corpus — point it at any tree of *.yang files.
        let Ok(root) = std::env::var("NETCONF_LSP_YANG_CORPUS") else {
            panic!(
                "set NETCONF_LSP_YANG_CORPUS=<dir-of-yang> to regenerate the highlight report \
                 (optionally NETCONF_LSP_HIGHLIGHT_OUT=<path> for the output file)"
            );
        };
        let root = PathBuf::from(root);
        let out = std::env::var("NETCONF_LSP_HIGHLIGHT_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("highlight_report.md"));
        let (docs, total, gaps) = coverage_gaps(&root);
        let report = render_report(&out, &root, docs, total, &gaps);
        std::fs::write(&out, report).expect("write highlight report");
    }

    // --- Vendored corpus (`testdata/highlight`) + coverage guard ------------

    fn corpus_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/highlight")
    }

    fn baseline_path() -> PathBuf {
        corpus_dir().join("baseline.json")
    }

    /// Semantic tokens over one fixture module (`name` relative to the corpus
    /// dir), decoded to absolute positions.
    fn fixture_semantic(name: &str) -> (Rope, Vec<(u32, u32, u32, u32)>) {
        let src = std::fs::read_to_string(corpus_dir().join(name)).expect("fixture");
        let rope = Rope::from_str(&src);
        let mut repo = yrepo::Repository::new();
        repo.upsert("f", src);
        repo.compile();
        let root = repo.statement("f");
        let toks = repo.tokens("f").unwrap();
        let out = handle(&rope, root, toks, &SemanticSettings::default()).expect("tokens");
        let abs = decode(&out);
        (rope, abs)
    }

    /// Like [`fixture_semantic`] but also keeps each token's modifier bitset.
    #[allow(clippy::type_complexity)]
    fn fixture_semantic_full(name: &str) -> (Rope, Vec<(u32, u32, u32, u32, u32)>) {
        let src = std::fs::read_to_string(corpus_dir().join(name)).expect("fixture");
        let rope = Rope::from_str(&src);
        let mut repo = yrepo::Repository::new();
        repo.upsert("f", src);
        repo.compile();
        let root = repo.statement("f");
        let toks = repo.tokens("f").unwrap();
        let out = handle(&rope, root, toks, &SemanticSettings::default()).expect("tokens");
        let mut abs: Vec<(u32, u32, u32, u32, u32)> = Vec::with_capacity(out.len());
        let (mut line, mut col) = (0u32, 0u32);
        for t in &out {
            if t.delta_line > 0 {
                line += t.delta_line;
                col = t.delta_start;
            } else {
                col += t.delta_start;
            }
            abs.push((line, col, t.length, t.token_type, t.token_modifiers_bitset));
        }
        (rope, abs)
    }

    /// Texts of the decoded tokens of one class.
    fn class_texts(rope: &Rope, abs: &[(u32, u32, u32, u32)], class: Class) -> Vec<String> {
        abs.iter()
            .filter(|(_, _, _, t)| *t == class as u32)
            .map(|&(l, c, n, _)| seg_text(rope, l, c, n))
            .collect()
    }

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct HighlightBaseline {
        files: usize,
        total_words: usize,
        uncovered: usize,
        buckets: BTreeMap<String, usize>,
    }

    fn bucket_key(g: &Gap) -> String {
        format!("{}{}", g.stmt, if g.in_arg { " (arg)" } else { "" })
    }

    fn bucket_map(gaps: &[Gap]) -> BTreeMap<String, usize> {
        let mut m = BTreeMap::new();
        for g in gaps {
            *m.entry(bucket_key(g)).or_default() += 1;
        }
        m
    }

    /// The shapes we explicitly fixed must stay highlighted (regression guard):
    /// negative numbers/decimals, whole `range`/`length` strings, `deviate`
    /// verbs, quoted augment targets, value keywords (`status`/`ordered-by`),
    /// `revision` dates, quoted booleans and structural keyword/type/variable
    /// coloring.
    #[test]
    fn highlight_known_shapes_are_covered() {
        // Negative integers & decimals (numbers.yang).
        let (rope, abs) = fixture_semantic("numbers.yang");
        let numbers = class_texts(&rope, &abs, Class::Number);
        for expected in ["-10", "-27", "-1", "0", "-3.25", "2.5", "2"] {
            assert!(
                numbers.iter().any(|s| s == expected),
                "number {expected:?} not highlighted in numbers.yang: {numbers:?}"
            );
        }

        // deviate verbs & the deviation keyword (deviations.yang).
        let (rope, abs) = fixture_semantic("deviations.yang");
        let kws = class_texts(&rope, &abs, Class::Keyword);
        assert!(
            kws.iter().any(|s| s == "deviation"),
            "deviation keyword: {kws:?}"
        );
        assert!(
            kws.iter().filter(|s| *s == "deviate").count() >= 4,
            "'deviate' keywords missing: {kws:?}"
        );
        for verb in ["add", "replace", "delete", "not-supported"] {
            assert!(
                kws.iter().any(|s| s == verb),
                "deviate verb {verb:?}: {kws:?}"
            );
        }

        // Quoted augment target is a string; node names & statement keywords
        // are colored (refs.yang).
        let (rope, abs) = fixture_semantic("refs.yang");
        let strings = class_texts(&rope, &abs, Class::String);
        assert!(
            strings
                .iter()
                .any(|s| s.contains("r:top") && s.starts_with('"')),
            "quoted augment target not a string token: {strings:?}"
        );
        let vars = class_texts(&rope, &abs, Class::Variable);
        for name in [
            "top",
            "item",
            "name",
            "kind",
            "serial",
            "flag-on",
            "via-quoted",
            "extra",
        ] {
            assert!(
                vars.iter().any(|s| s == name),
                "data node {name:?} not variable-colored: {vars:?}"
            );
        }
        let kw = class_texts(&rope, &abs, Class::Keyword);
        assert!(kw.iter().any(|s| s == "feature"), "feature keyword: {kw:?}");

        // Value keywords inside composite args are keyword-colored
        // (keywords.yang): `status current/deprecated` and `ordered-by user`.
        // (`range`/`length` args are whole strings now — see below.)
        let (rope, abs) = fixture_semantic("keywords.yang");
        let kws = class_texts(&rope, &abs, Class::Keyword);
        for w in [
            "container",
            "list",
            "leaf",
            "type",
            "key",
            "ordered-by",
            "user",
            "status",
            "deprecated",
            "current",
        ] {
            assert!(
                kws.iter().any(|s| s == w),
                "keyword {w:?} not highlighted in keywords.yang: {kws:?}"
            );
        }
        // ...and its quoted `range`/`length` args are colored as strings whole.
        let strings = class_texts(&rope, &abs, Class::String);
        assert_eq!(
            strings
                .iter()
                .filter(|s| s.as_str() == "\"1..max\"")
                .count(),
            2,
            "range/length quoted args not colored whole: {strings:?}"
        );

        // Quoted range args with negative bounds are colored whole, incl.
        // `range "-16..-1"` (mirrors bbf-...-body-mounted.yang).
        let (rope, abs) = fixture_semantic("numbers.yang");
        let strings = class_texts(&rope, &abs, Class::String);
        for r in [
            "\"-16..-1\"",
            "\"-48..-14\"",
            "\"-3..6\"",
            "\"-63.5..-0.5\"",
        ] {
            assert!(
                strings.iter().any(|s| s == r),
                "range {r:?} not a whole string token: {strings:?}"
            );
        }

        // Revision dates are colored as strings (dates.yang).
        let (rope, abs) = fixture_semantic("dates.yang");
        let strings = class_texts(&rope, &abs, Class::String);
        for d in ["2020-05-29", "2018-04-23", "2017-05-08"] {
            assert!(
                strings.iter().any(|s| s == d),
                "revision date {d:?} not highlighted: {strings:?}"
            );
        }

        // Extension statements (vendor-unknown.yang): the head `prefix:name`
        // is keyword-colored and a bare identifier argument is colored like a
        // typedef name (quoted args stay strings).
        let (rope, abs) = fixture_semantic("vendor-unknown.yang");
        let kws = class_texts(&rope, &abs, Class::Keyword);
        for head in ["vendor:id", "vendor:info", "vendor:flag", "vendor:validate"] {
            assert!(
                kws.iter().any(|s| s == head),
                "extension head {head:?} not keyword-colored: {kws:?}"
            );
        }
        let vars = class_texts(&rope, &abs, Class::Variable);
        assert!(
            vars.iter().any(|s| s == "on"),
            "bare extension arg 'on' not variable-colored: {vars:?}"
        );

        // Quoted AND unquoted booleans are both colored (quoted.yang):
        // 2 × `false` (config "false" + config false), 2 × `true`
        // (mandatory "true" + mandatory true).
        let (rope, abs) = fixture_semantic("quoted.yang");
        let kws = class_texts(&rope, &abs, Class::Keyword);
        assert_eq!(
            kws.iter().filter(|s| *s == "false").count(),
            2,
            "config booleans not both colored: {kws:?}"
        );
        assert_eq!(
            kws.iter().filter(|s| *s == "true").count(),
            2,
            "mandatory booleans not both colored: {kws:?}"
        );

        // REQ1/REQ5 — key/unique/augment args are whole strings (refs.yang):
        // `key "name kind"`, `unique "serial"`, quoted `augment "/r:top"`, and
        // the unquoted augment target reads as one string too.
        let (rope, abs) = fixture_semantic("refs.yang");
        let strings = class_texts(&rope, &abs, Class::String);
        for expected in ["\"name kind\"", "\"serial\"", "\"/r:top\""] {
            assert!(
                strings.iter().any(|s| s == expected),
                "key/unique/augment {expected:?} not a whole string: {strings:?}"
            );
        }
        assert!(
            strings.iter().any(|s| s.contains("/r:top/r:item")),
            "unquoted augment target not a string: {strings:?}"
        );

        // REQ2 — if-feature bare ref colored like a typedef name.
        let vars = class_texts(&rope, &abs, Class::Variable);
        assert!(
            vars.iter().any(|s| s == "fast"),
            "if-feature ref 'fast' not variable-colored: {vars:?}"
        );

        // REQ4 — an unquoted `units` value is Variable + readonly (const proxy).
        let (rope, abs5) = fixture_semantic_full("refs.yang");
        let constish: Vec<String> = abs5
            .iter()
            .filter(|(_, _, _, _, m)| *m & MOD_CONST != 0)
            .map(|&(l, c, n, _, _)| seg_text(&rope, l, c, n))
            .collect();
        assert!(
            constish.iter().any(|s| s == "milliseconds"),
            "units 'milliseconds' not Variable+readonly: {constish:?}"
        );

        // REQ3 — `default lo;` enum reference colored as a name (numbers.yang).
        let (rope, abs) = fixture_semantic("numbers.yang");
        let vars = class_texts(&rope, &abs, Class::Variable);
        assert!(
            vars.iter().any(|s| s == "lo"),
            "default enum ref 'lo' not variable-colored: {vars:?}"
        );

        // REQ6 — `argument name;` (unquoted) colored as a name (vendor-unknown).
        let (rope, abs) = fixture_semantic("vendor-unknown.yang");
        let vars = class_texts(&rope, &abs, Class::Variable);
        assert!(
            vars.iter().any(|s| s == "name"),
            "extension argument 'name' not variable-colored: {vars:?}"
        );
    }

    /// The vendored corpus must keep producing exactly the same highlight
    /// gaps. Any *growth* or *new* gap family fails; so does shrinking, so a
    /// fix must be recorded by re-blessing the baseline.
    #[test]
    fn highlight_coverage_matches_baseline() {
        let dir = corpus_dir();
        let (files, total, gaps) = coverage_gaps(&dir);
        let actual = HighlightBaseline {
            files,
            total_words: total,
            uncovered: gaps.len(),
            buckets: bucket_map(&gaps),
        };
        let bp = baseline_path();
        let expected: HighlightBaseline =
            serde_json::from_str(&std::fs::read_to_string(&bp).expect("baseline.json"))
                .expect("parse baseline.json");
        assert_eq!(
            expected,
            actual,
            "highlight coverage drifted vs {}\n\
             accept intentionally changed numbers with:\n    cargo test -- --ignored bless_highlight_baseline",
            bp.display()
        );
    }

    /// Re-record `baseline.json` from the current vendored corpus. Run after
    /// intentionally fixing (or extending) highlight behavior.
    #[test]
    #[ignore = "re-blesses testdata/highlight/baseline.json; run explicitly"]
    fn bless_highlight_baseline() {
        let dir = corpus_dir();
        let (files, total, gaps) = coverage_gaps(&dir);
        let baseline = HighlightBaseline {
            files,
            total_words: total,
            uncovered: gaps.len(),
            buckets: bucket_map(&gaps),
        };
        let json = serde_json::to_string_pretty(&baseline).expect("json");
        std::fs::write(baseline_path(), json + "\n").expect("write baseline.json");
    }

    #[test]
    fn multi_line_tokens_are_split_per_line() {
        let src = "module x {\n  yang-version 1.1;\n  namespace \"urn:x\";\n  prefix x;\n\
            description \"first line\n  second line\n  third line\";\n  /* block comment\n\
            spanning lines */\n  leaf foo { type string; }\n}\n";
        let (rope, abs) = tokens_for(src);
        // No token may cross a line boundary: col + length must fit in its line.
        assert!(!abs.is_empty());
        for (line, col, len, _ty) in &abs {
            assert!(
                *col + *len <= line_utf16_len(&rope, *line),
                "token L{line} c{col} len{len} crosses a line end"
            );
        }
        // The 3-line description string yields one token per line (lines 4..6).
        let string_txt: Vec<String> = abs
            .iter()
            .filter(|(_, _, _, t)| *t == Class::String as u32)
            .map(|&(l, c, n, _)| seg_text(&rope, l, c, n))
            .collect();
        let joined: String = string_txt.iter().flat_map(|s| s.chars()).collect();
        assert!(
            joined.contains("first line")
                && joined.contains("second line")
                && joined.contains("third line")
        );
        // ...and the 2-line block comment yields two comment tokens.
        let comment_lines: u32 = abs
            .iter()
            .filter(|(_, _, _, t)| *t == Class::Comment as u32)
            .count() as u32;
        assert_eq!(
            comment_lines, 2,
            "block comment should be split into one token per line"
        );
    }

    #[test]
    fn augment_and_path_arguments_are_highlighted() {
        let src = "module x {\n  yang-version 1.1;\n  namespace \"urn:x\";\n  prefix x;\n\
            import ietf-interfaces { prefix if; }\n  augment \"/if:interfaces/if:interface\" {\n\
            leaf foo { type string; }\n  }\n  typedef r {\n    type leafref { path \"/if:interfaces/if:interface/if:name\"; }\n  }\n}\n";
        let (rope, abs) = tokens_for(src);
        let texts: Vec<String> = abs
            .iter()
            .filter(|(_, _, _, t)| *t == Class::String as u32)
            .map(|&(l, c, n, _)| seg_text(&rope, l, c, n))
            .collect();
        assert!(
            texts
                .iter()
                .any(|s| s.contains("if:interfaces/if:interface")),
            "augment path argument not highlighted: {texts:?}"
        );
        assert!(
            texts.iter().any(|s| s.contains("if:interface/if:name")),
            "leafref path argument not highlighted: {texts:?}"
        );
    }

    #[test]
    fn deviate_keywords_are_highlighted() {
        let src = "module x {\n  yang-version 1.1;\n  namespace \"urn:x\";\n  prefix x;\n\
            container c { leaf a { type string; } }\n\
            deviation \"/x:c/x:a\" {\n\
            deviate add { default \"y\"; }\n\
            deviate not-supported;\n  }\n}\n";
        let (rope, abs) = tokens_for(src);
        let kws: Vec<String> = abs
            .iter()
            .filter(|(_, _, _, t)| *t == Class::Keyword as u32)
            .map(|&(l, c, n, _)| seg_text(&rope, l, c, n))
            .collect();
        assert!(
            kws.iter().any(|s| s == "deviation"),
            "deviation keyword missing: {kws:?}"
        );
        assert!(
            kws.iter().filter(|s| *s == "deviate").count() >= 2,
            "leading 'deviate' keywords missing: {kws:?}"
        );
        assert!(
            kws.iter().any(|s| s == "add"),
            "'deviate add' verb missing: {kws:?}"
        );
        assert!(
            kws.iter().any(|s| s == "not-supported"),
            "'deviate not-supported' verb missing: {kws:?}"
        );
    }

    #[test]
    fn negative_numbers_are_highlighted() {
        // Mirrors `bbf-clip-data-mode-profile-body-mounted.yang`: unquoted
        // negative `default` literals (and a negative enum `value`).
        let src = "module x {\n  yang-version 1.1;\n  namespace \"urn:x\";\n  prefix x;\n\
            leaf a { type int8 { range \"-16..-1\"; } default -10; }\n\
            leaf b { type int8 { range \"-48..-14\"; } default -27; }\n\
            leaf c { type enumeration { enum n { value -1; } } }\n\
            leaf d { type int8 { range \"-3..6\"; } default 0; }\n\
            leaf e { type decimal64 { fraction-digits 2; } default -3.25; }\n}\n";
        let (rope, abs) = tokens_for(src);
        let numbers: Vec<String> = abs
            .iter()
            .filter(|(_, _, _, t)| *t == Class::Number as u32)
            .map(|&(l, c, n, _)| seg_text(&rope, l, c, n))
            .collect();
        for expected in ["-10", "-27", "-1", "0", "-3.25"] {
            assert!(
                numbers.iter().any(|s| s == expected),
                "negative number {expected:?} not highlighted: {numbers:?}"
            );
        }
        // Positive bounds inside quoted range strings stay strings, not numbers.
        assert!(
            numbers.iter().all(|s| s != "-16"),
            "quoted range content should not become a number token: {numbers:?}"
        );
    }

    // --- Customization: per-role token type + modifiers ---------------------

    /// Run [`handle`] over an inline module under an explicit
    /// `netconf.semantic` configuration, keeping the raw encoded tokens.
    fn run_settings(src: &str, settings: &SemanticSettings) -> (Rope, Vec<SemanticToken>) {
        let rope = Rope::from_str(src);
        let mut repo = yrepo::Repository::new();
        repo.upsert(String::from("u"), src.to_string());
        repo.compile();
        let root = repo.statement("u");
        let toks = repo.tokens("u").unwrap();
        let out = handle(&rope, root, toks, settings).expect("tokens");
        (rope, out)
    }

    /// Decoded absolute tokens under a given configuration.
    fn tokens_for_settings(
        src: &str,
        settings: &SemanticSettings,
    ) -> (Rope, Vec<(u32, u32, u32, u32)>) {
        let (rope, out) = run_settings(src, settings);
        let abs = decode(&out);
        (rope, abs)
    }

    /// Decode keeping each token's modifier bitset.
    fn decode_marked(out: &[SemanticToken]) -> Vec<(u32, u32, u32, u32, u32)> {
        let mut abs = Vec::with_capacity(out.len());
        let (mut line, mut col) = (0u32, 0u32);
        for t in out {
            if t.delta_line > 0 {
                line += t.delta_line;
                col = t.delta_start;
            } else {
                col += t.delta_start;
            }
            abs.push((line, col, t.length, t.token_type, t.token_modifiers_bitset));
        }
        abs
    }

    /// Texts of decoded tokens whose *type* is one of `classes`.
    fn text_of_classes(
        rope: &Rope,
        abs: &[(u32, u32, u32, u32)],
        classes: &[Class],
    ) -> Vec<String> {
        abs.iter()
            .filter(|(_, _, _, t)| classes.iter().any(|c| *c as u32 == *t))
            .map(|&(l, c, n, _)| seg_text(rope, l, c, n))
            .collect()
    }

    /// Texts of tokens carrying any of the modifier bits in `mask`.
    fn text_of_marked(rope: &Rope, out: &[SemanticToken], mask: u32) -> Vec<String> {
        decode_marked(out)
            .into_iter()
            .filter(|(_, _, _, _, m)| m & mask != 0)
            .map(|(l, c, n, _, _)| seg_text(rope, l, c, n))
            .collect()
    }

    fn has_class(abs: &[(u32, u32, u32, u32)], class: Class) -> bool {
        abs.iter().any(|(_, _, _, t)| *t == class as u32)
    }

    /// Settings built from role-key → override pairs (mirrors a
    /// `netconf.semantic` object whose members are the roles).
    fn semantic(pairs: &[(&str, SemanticOverride)]) -> SemanticSettings {
        SemanticSettings {
            roles: pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect(),
        }
    }

    /// Shorthand override value: only the token type.
    fn role_type(name: &str) -> SemanticOverride {
        SemanticOverride::Type(name.to_owned())
    }

    /// Full override value: token type (optional) + modifiers (optional).
    fn role_fields(token: Option<&str>, modifiers: &[&str]) -> SemanticOverride {
        SemanticOverride::Fields(SemanticOverrideFields {
            token: token.map(str::to_owned),
            modifiers: Some(modifiers.iter().map(|m| m.to_string()).collect()),
        })
    }

    #[test]
    fn roles_retarget_to_any_standard_type() {
        let src = "module x {\n  yang-version 1.1;\n  namespace \"urn:x\";\n  prefix x;\n\
            container c {\n    leaf e { type enumeration { enum up; } }\n    leaf n { type int8; }\n  }\n}\n";
        // Every role is a member of the config object and any standard token
        // type (the identical map to SemanticTokenType) is a legal value.
        let settings = semantic(&[
            ("moduleName", role_type("decorator")),
            ("enumAndBitNames", role_type("enumMember")),
            ("typeRef", role_type("class")),
            ("dataNodeName", role_type("property")),
        ]);
        let (rope, abs) = tokens_for_settings(src, &settings);
        let decorators = text_of_classes(&rope, &abs, &[Class::Decorator]);
        assert!(
            decorators.iter().any(|t| t == "x"),
            "module name not decorator: {decorators:?}"
        );
        let enumed = text_of_classes(&rope, &abs, &[Class::EnumMember]);
        assert!(
            enumed.iter().any(|t| t == "up"),
            "enum name not enumMember: {enumed:?}"
        );
        let classes = text_of_classes(&rope, &abs, &[Class::Class]);
        for t in ["int8", "enumeration"] {
            assert!(
                classes.iter().any(|s| s == t),
                "{t:?} not class-colored: {classes:?}"
            );
        }
        let props = text_of_classes(&rope, &abs, &[Class::Property]);
        for t in ["c", "e", "n"] {
            assert!(
                props.iter().any(|s| s == t),
                "data node {t:?} not property: {props:?}"
            );
        }
        // Roles we left out keep their built-in classification (keywords and
        // quoted strings are still colored as such).
        assert!(has_class(&abs, Class::Keyword), "unconfigured role changed");
        assert!(has_class(&abs, Class::String), "unconfigured role changed");
    }

    #[test]
    fn units_modifiers_follow_the_role_value() {
        let src = "module x {\n  yang-version 1.1;\n  namespace \"urn:x\";\n  prefix x;\n\
            leaf u { type string; units meters-second; }\n}\n";
        // Built-in: units value is Variable + readonly (const proxy).
        let (rope, out) = run_settings(src, &SemanticSettings::default());
        assert!(
            text_of_marked(&rope, &out, Modifier::Readonly.bit())
                .iter()
                .any(|t| t == "meters-second"),
            "units not Variable+readonly by default"
        );
        // { "token": "string", "modifiers": [] } → a plain string, modifier off.
        let settings = semantic(&[("units", role_fields(Some("string"), &[]))]);
        let (rope, out) = run_settings(src, &settings);
        let abs = decode(&out);
        assert!(
            text_of_classes(&rope, &abs, &[Class::String])
                .iter()
                .any(|t| t == "meters-second"),
            "units value not string-colored after override"
        );
        assert!(
            text_of_marked(&rope, &out, Modifier::Readonly.bit()).is_empty(),
            "readonly modifier should drop when modifiers are cleared"
        );
        // Re-assert the constant look explicitly.
        let settings = semantic(&[("units", role_fields(Some("variable"), &["readonly"]))]);
        let (rope, out) = run_settings(src, &settings);
        assert!(
            text_of_marked(&rope, &out, Modifier::Readonly.bit())
                .iter()
                .any(|t| t == "meters-second"),
            "explicit readonly modifier lost"
        );
        // Any standard modifier rides along, e.g. deprecated.
        let settings = semantic(&[("units", role_fields(Some("variable"), &["deprecated"]))]);
        let (rope, out) = run_settings(src, &settings);
        assert!(
            text_of_marked(&rope, &out, Modifier::Deprecated.bit())
                .iter()
                .any(|t| t == "meters-second"),
            "deprecated modifier not applied"
        );
        assert!(
            text_of_marked(&rope, &out, Modifier::Readonly.bit()).is_empty(),
            "readonly should be replaced, not merged, when modifiers are set"
        );
    }

    #[test]
    fn unknown_roles_and_types_fall_back_to_builtins() {
        let src = "module x {\n  yang-version 1.1;\n  namespace \"urn:x\";\n  prefix x;\n\
            leaf u { type string; units meters-second; }\n}\n";
        let settings = semantic(&[
            ("notARole", role_type("enumMember")), // unknown member → ignored
            ("units", role_type("notATokenType")), // unknown value → ignored
        ]);
        let (rope, out) = run_settings(src, &settings);
        let abs = decode(&out);
        // units keeps its built-in Variable + readonly classification.
        assert!(
            text_of_classes(&rope, &abs, &[Class::Variable])
                .iter()
                .any(|t| t == "meters-second")
        );
        assert!(
            text_of_marked(&rope, &out, Modifier::Readonly.bit())
                .iter()
                .any(|t| t == "meters-second")
        );
        // And the bogus enumMember key changed nothing.
        assert!(
            !text_of_classes(&rope, &abs, &[Class::EnumMember])
                .iter()
                .any(|t| t == "x")
        );
    }

    /// The announced legend is the full standard `SemanticTokenType` /
    /// `SemanticTokenModifier` set, so a user can compose any classification
    /// the semantic-highlight guide supports — no server restart needed.
    #[test]
    fn capability_announces_the_full_standard_legend() {
        use tower_lsp_server::ls_types::SemanticTokensServerCapabilities as Cap;
        let expected_types: Vec<String> = [
            "namespace",
            "type",
            "class",
            "enum",
            "interface",
            "struct",
            "typeParameter",
            "parameter",
            "variable",
            "property",
            "enumMember",
            "event",
            "function",
            "method",
            "macro",
            "keyword",
            "modifier",
            "comment",
            "string",
            "number",
            "regexp",
            "operator",
            "decorator",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let expected_mods: Vec<String> = [
            "declaration",
            "definition",
            "readonly",
            "static",
            "deprecated",
            "abstract",
            "async",
            "modification",
            "documentation",
            "defaultLibrary",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        match capability() {
            Cap::SemanticTokensOptions(opts) => {
                let types: Vec<String> = opts
                    .legend
                    .token_types
                    .iter()
                    .map(|t| t.as_str().to_owned())
                    .collect();
                let mods: Vec<String> = opts
                    .legend
                    .token_modifiers
                    .iter()
                    .map(|t| t.as_str().to_owned())
                    .collect();
                assert_eq!(
                    types, expected_types,
                    "token types must be the full standard set"
                );
                assert_eq!(
                    mods, expected_mods,
                    "modifiers must be the full standard set"
                );
                assert_eq!(Class::ALL.len(), expected_types.len());
                assert_eq!(Modifier::ALL.len(), expected_mods.len());
            }
            _ => panic!("unexpected semantic-tokens capability shape"),
        }
    }

    #[test]
    fn pattern_arguments_are_regexp_by_default() {
        let src = "module x {\n  yang-version 1.1;\n  namespace \"urn:x\";\n  prefix x;\n\
            leaf a { type string { pattern \"[0-9]+\"; } }\n}\n";
        let (rope, abs) = tokens_for_settings(src, &SemanticSettings::default());
        let regexps = text_of_classes(&rope, &abs, &[Class::Regexp]);
        assert!(
            regexps.iter().any(|t| t == "\"[0-9]+\""),
            "pattern arg not regexp-colored by default: {regexps:?}"
        );
        // Overridable back to a plain string via the `patternArg` member.
        let settings = semantic(&[("patternArg", role_type("string"))]);
        let (rope, abs) = tokens_for_settings(src, &settings);
        assert!(
            text_of_classes(&rope, &abs, &[Class::Regexp]).is_empty(),
            "patternArg → string should leave no regexp tokens"
        );
        assert!(
            text_of_classes(&rope, &abs, &[Class::String])
                .iter()
                .any(|t| t == "\"[0-9]+\""),
            "pattern arg not string-colored after patternArg override"
        );
    }

    #[test]
    fn status_deprecated_marks_the_declaration_subtree() {
        let src = "module m {\n  yang-version 1.1;\n  namespace \"urn:x\";\n  prefix x;\n\
            container c {\n    status deprecated;\n    leaf a { type string; }\n  }\n\
            container d {\n    leaf b { type string; }\n  }\n}\n";
        let (rope, out) = run_settings(src, &SemanticSettings::default());
        let marked = text_of_marked(&rope, &out, Modifier::Deprecated.bit());
        // The deprecated container's own name and its subtree leaf are marked…
        for t in ["c", "a"] {
            assert!(
                marked.iter().any(|s| s == t),
                "{t:?} not deprecated-marked: {marked:?}"
            );
        }
        // …but not an unrelated sibling declaration…
        assert!(
            marked.iter().all(|s| s != "d" && s != "b"),
            "sibling should not be deprecated-marked: {marked:?}"
        );
        // …and the `status deprecated;` marker words themselves are struck too
        // (no marker exclusion).
        for t in ["status", "deprecated"] {
            assert!(
                marked.iter().any(|s| s == t),
                "{t:?} should be deprecated-marked: {marked:?}"
            );
        }
    }
}
