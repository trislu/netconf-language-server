//! Process-global LSP `Client` holder plus `window/logMessage` helpers.
//!
//! All logging is routed through the client channel (stdout is reserved for
//! the JSON-RPC transport, enforced by `#![deny(clippy::print_stdout)]`).

use std::fmt::Display;
use std::sync::atomic::{AtomicI32, Ordering};

use tokio::sync::OnceCell;
use tower_lsp_server::{
    Client, NotCancellable, OngoingProgress, Unbounded, jsonrpc,
    ls_types::{ConfigurationItem, LSPAny, MessageType, ProgressToken},
};

static CLIENT_INSTANCE: OnceCell<Client> = OnceCell::const_new();

pub(crate) fn init(c: Client) {
    let _ = CLIENT_INSTANCE.set(c);
}

pub(crate) struct Window;

#[macro_export]
macro_rules! log {
    ($msg:expr) => {
        (
            tower_lsp_server::ls_types::MessageType::LOG,
            $msg.to_owned(),
        )
    };
}

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
    ///
    /// Fire-and-forget on purpose: `workspace/diagnostic/refresh` is a client
    /// *request*, so awaiting it inside a handler would stall the server until
    /// the client answers. Handlers run serially (`concurrency_level(1)`), so
    /// the refresh is spawned instead — the client reply is still awaited on
    /// the background task, but no handler slot is occupied meanwhile.
    pub(crate) fn refresh() {
        if let Some(client) = CLIENT_INSTANCE.get() {
            let client = client.clone();
            tokio::spawn(async move {
                let _ = client.workspace_diagnostic_refresh().await;
            });
        }
    }
}

/// Ask the client to re-request semantic tokens (`workspace/semanticTokens/refresh`).
pub(crate) struct Semantics;

impl Semantics {
    /// Request a full re-request of semantic tokens for every open document.
    ///
    /// Semantic tokens are a pull: the client only re-requests a document when
    /// it changes — never when the server's *classification* changes (e.g. a
    /// `netconf.semantic` edit arrives via `didChangeConfiguration`). Without
    /// this, a new per-role token/modifier mapping would not show until the
    /// document was edited or reopened. Fire-and-forget on purpose — the
    /// `workspace/semanticTokens/refresh` request is spawned so no handler slot
    /// is occupied while the client answers (handlers run serially).
    pub(crate) fn refresh() {
        if let Some(client) = CLIENT_INSTANCE.get() {
            let client = client.clone();
            tokio::spawn(async move {
                let _ = client.semantic_tokens_refresh().await;
            });
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

/// A live server→client work-done progress stream (unbounded,
/// non-cancellable). Created by [`Progress::work_done`]; report intermediate
/// messages with [`WorkDone::report`] and always end with [`WorkDone::finish`].
pub(crate) struct WorkDone {
    ongoing: OngoingProgress<Unbounded, NotCancellable>,
}

impl WorkDone {
    /// Update the secondary progress message shown in the client UI.
    pub(crate) async fn report<M: Into<String>>(&self, message: M) {
        self.ongoing.report(message).await;
    }

    /// End the progress stream (must be called exactly once).
    pub(crate) async fn finish(self) {
        self.ongoing.finish().await;
    }
}

/// Server→client `window/workDoneProgress` helpers, mirroring [`Window`].
pub(crate) struct Progress;

impl Progress {
    /// Begin an unbounded work-done progress stream titled `title` with an
    /// initial `message`. Returns `None` when no client is connected yet or
    /// the client declined `window/workDoneProgress/create` — callers should
    /// simply run without a progress bar in that case.
    pub(crate) async fn work_done(
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> Option<WorkDone> {
        let client = CLIENT_INSTANCE.get()?.clone();
        // Namespaced string token: server-created work-done tokens share the
        // client's progress namespace, so a numeric token could in principle
        // collide with a token the client generated itself. The `netconf/`
        // prefix makes our stream's token unique across the session (each
        // process also restarts its own counter, which is fine since tokens
        // are scoped to one client↔server connection).
        let n = PROGRESS_TOKEN.fetch_add(1, Ordering::Relaxed);
        let token = ProgressToken::String(format!("netconf/wd/{n}"));
        if client
            .create_work_done_progress(token.clone())
            .await
            .is_err()
        {
            return None;
        }
        let ongoing = client
            .progress(token, title.into())
            .with_message(message.into())
            .begin()
            .await;
        Some(WorkDone { ongoing })
    }
}

/// Monotonic counter suffixing each server work-done progress token.
static PROGRESS_TOKEN: AtomicI32 = AtomicI32::new(1);
