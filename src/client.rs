//! Process-global LSP `Client` holder plus `window/logMessage` helpers.
//!
//! All logging is routed through the client channel (stdout is reserved for
//! the JSON-RPC transport, enforced by `#![deny(clippy::print_stdout)]`).

use std::fmt::Display;

use tokio::sync::OnceCell;
use tower_lsp_server::{
    Client, jsonrpc,
    ls_types::{ConfigurationItem, LSPAny, MessageType},
};

static CLIENT_INSTANCE: OnceCell<Client> = OnceCell::const_new();

pub(crate) fn init(c: Client) {
    let _ = CLIENT_INSTANCE.set(c);
}

pub(crate) struct Window;

#[macro_export]
macro_rules! info {
    ($msg:expr) => {
        (
            tower_lsp_server::ls_types::MessageType::INFO,
            $msg.to_owned(),
        )
    };
}

#[macro_export]
macro_rules! warning {
    ($msg:expr) => {
        (
            tower_lsp_server::ls_types::MessageType::WARNING,
            $msg.to_owned(),
        )
    };
}

impl Window {
    /// Fire-and-forget logging for call paths that cannot `await`.
    #[allow(unused)]
    pub(crate) fn log_sync<M: Display>(m: (MessageType, M)) {
        if let Some(client) = CLIENT_INSTANCE.get() {
            let client = client.clone();
            let msg = m.1.to_string();
            tokio::spawn(async move { client.log_message(m.0, msg).await });
        }
    }

    pub(crate) async fn log<M: Display>(m: (MessageType, M)) {
        if let Some(client) = CLIENT_INSTANCE.get() {
            let client = client.clone();
            client.log_message(m.0, m.1).await;
        }
    }
}

pub(crate) struct Workspace;

impl Workspace {
    pub(crate) async fn configuration(
        items: Vec<ConfigurationItem>,
    ) -> jsonrpc::Result<Vec<LSPAny>> {
        match CLIENT_INSTANCE.get() {
            Some(c) => c.configuration(items).await,
            None => Ok(vec![]),
        }
    }
}

pub(crate) struct Diagnostics;

impl Diagnostics {
    /// Ask the client to re-pull diagnostics for every open document
    /// (`workspace/diagnostic/refresh`).
    ///
    /// We use **pull** diagnostics, so the client only re-requests a document
    /// when that document changes — never when the *set of known modules*
    /// changes. That left stale results behind (e.g. "… imports … but it is
    /// not open" computed before the workspace scan finished or before a
    /// dependency was opened) until the user manually reopened the file.
    /// Call this after any repo change that can affect other open documents:
    /// a document opened/closed, or the workspace scan completing.
    pub(crate) async fn refresh() {
        if let Some(client) = CLIENT_INSTANCE.get() {
            let _ = client.workspace_diagnostic_refresh().await;
        }
    }
}

pub(crate) struct Edits;

impl Edits {
    /// Apply a server-initiated `WorkspaceEdit` (used by the M2 template
    /// insert command).
    pub(crate) async fn apply(edit: tower_lsp_server::ls_types::WorkspaceEdit) {
        let Some(client) = CLIENT_INSTANCE.get() else {
            return;
        };
        if let Err(e) = client.apply_edit(edit).await {
            Window::log(warning!(format!("apply_edit failed: {e}"))).await;
        }
    }
}
