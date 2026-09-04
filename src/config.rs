use serde::Deserialize;

/// Server configuration, read from the client's `netconf` settings section
/// (VS Code properties are camelCase, e.g. `netconf.indentSize`).
#[derive(Deserialize, Default, Debug, Clone)]
pub(crate) struct Config {
    /// Number of spaces used per indentation level (spaces only; no tabs).
    #[serde(rename = "indentSize", default)]
    pub(crate) indent_size: Option<u8>,
}

impl Config {
    pub(crate) fn indent_width(&self) -> u32 {
        self.indent_size
            .map(|v| u32::from(v.clamp(1, 16)))
            .unwrap_or(4)
    }
}
