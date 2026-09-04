//! NETCONF Language Server — entry point.

#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

mod client;
mod completion;
mod config;
mod convert;
mod diagnostic;
mod document;
mod fold;
mod format;
mod goto;
mod hover;
mod semantic_token;
mod server;
mod workspace;

use tower_lsp_server::LspService;

use server::Server;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Server::new);
    tower_lsp_server::Server::new(stdin, stdout, socket)
        .serve(service)
        .await;
}
