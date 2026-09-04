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
