use std::collections::BTreeMap;

use serde::Deserialize;

/// Server configuration, read from the client's `netconf` settings section
/// (VS Code properties are camelCase, e.g. `netconf.indentSize`).
#[derive(Deserialize, Default, Debug, Clone)]
pub(crate) struct Config {
    /// Number of spaces used per indentation level (spaces only; no tabs).
    #[serde(rename = "indentSize", default)]
    pub(crate) indent_size: Option<u8>,
    /// Semantic-token classification customization (`netconf.semantic`).
    #[serde(rename = "semantic", default)]
    pub(crate) semantic: SemanticSettings,
}

impl Config {
    pub(crate) fn indent_width(&self) -> u32 {
        self.indent_size
            .map(|v| u32::from(v.clamp(1, 16)))
            .unwrap_or(4)
    }
}

/// `netconf.semantic` — a per-role struct. Every supported YANG highlight
/// *role* (`semantic_token::Role`, see its `key()` doc) is a member of this
/// object, and each member's value picks the semantic **token type** for that
/// role plus, optionally, the **modifiers** that ride along with it.
///
/// Because the server announces the full standard `SemanticTokenType` /
/// `SemanticTokenModifier` sets in its legend, any of those names is a valid
/// value — users can compose arbitrary classifications, no server restart.
///
/// A member left out keeps that role's built-in classification. An unknown
/// role key, or a member whose token type / modifier name is unknown, is
/// ignored (the role keeps its built-in classification).
///
/// ```jsonc
/// "netconf.semantic": {
///   // shorthand: only the token type
///   "enumAndBitNames": "enumMember",
///   // full: token type + modifiers (multi-select) — either field optional
///   "units": { "token": "variable", "modifiers": ["readonly"] }
/// }
/// ```
#[derive(Deserialize, Default, Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemanticSettings {
    /// role key → override. `#[serde(flatten)]` makes each
    /// `netconf.semantic.<role>` key a map entry, so the config object reads
    /// exactly like a struct whose members are the supported roles.
    #[serde(flatten)]
    pub(crate) roles: BTreeMap<String, SemanticOverride>,
}

/// The per-role value: either the shorthand token-type name (`"enumMember"`)
/// or an object selecting `token` and/or `modifiers`.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub(crate) enum SemanticOverride {
    /// Shorthand: just the token type. The role's modifiers stay at their
    /// built-in value (e.g. `units` keeps `readonly`).
    Type(String),
    /// Full form: `{ "token": "enumMember", "modifiers": ["declaration", …] }`.
    /// A missing `token` keeps the role's default type; a missing `modifiers`
    /// keeps the role's default modifiers; an empty array clears them.
    Fields(SemanticOverrideFields),
}

/// The full (non-shorthand) per-role override value.
#[derive(Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SemanticOverrideFields {
    #[serde(rename = "token", default)]
    pub(crate) token: Option<String>,
    #[serde(default)]
    pub(crate) modifiers: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn semantic_settings_deserialize_as_a_role_struct() {
        // The client forwards the whole `netconf` section (indentSize plus the
        // nested `semantic` object), so Config must parse it as one value.
        let cfg: Config = serde_json::from_value(json!({
            "indentSize": 2,
            "semantic": {
                "enumAndBitNames": "enumMember",
                "units": { "token": "string", "modifiers": [] },
                "vendorExtension": { "modifiers": ["declaration"] }
            }
        }))
        .expect("parse netconf config");
        assert_eq!(cfg.indent_size, Some(2));
        // Shorthand string form.
        assert!(matches!(
            cfg.semantic.roles.get("enumAndBitNames"),
            Some(SemanticOverride::Type(n)) if n == "enumMember"
        ));
        // Full object form: token + empty modifiers.
        match cfg.semantic.roles.get("units") {
            Some(SemanticOverride::Fields(f)) => {
                assert_eq!(f.token.as_deref(), Some("string"));
                assert_eq!(f.modifiers.as_deref(), Some(&[] as &[String]));
            }
            _ => panic!("units should parse as the fields form"),
        }
        // Modifiers only: token stays unset (role default).
        match cfg.semantic.roles.get("vendorExtension") {
            Some(SemanticOverride::Fields(f)) => {
                assert_eq!(f.token, None);
                assert_eq!(
                    f.modifiers.as_deref(),
                    Some(&["declaration".to_owned()] as &[String])
                );
            }
            _ => panic!("vendorExtension should parse as the fields form"),
        }
    }

    #[test]
    fn missing_semantic_defaults_to_empty_role_map() {
        let cfg: Config = serde_json::from_value(json!({ "indentSize": 4 })).expect("parse");
        assert!(cfg.semantic.roles.is_empty());
        // An absent `netconf.semantic` (or empty config) reproduces the
        // historic classification.
        let defaults = Config::default();
        assert!(defaults.semantic.roles.is_empty());
    }
}
