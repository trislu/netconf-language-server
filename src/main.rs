//! NETCONF Language Server — entry point.

#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]

mod client;
mod closure;
mod completion;
mod config;
mod convert;
mod depth;
mod diagnostic;
mod document;
mod fold;
mod format;
mod goto;
mod hover;
mod incomplete;
mod inst;
mod inst_map;
mod jcomp;
mod jmap;
mod json;
mod references;
mod schema_idx;
mod semantic_token;
mod server;
mod template;
mod valcheck;
mod workspace;
mod xcomp;
mod xml;

use tower_lsp_server::LspService;

use server::Server;

// Both Linux flavors we ship (gnu and musl) use mimalloc. The catalog scan is
// allocation-churn heavy (one owned `String` per CST leaf); the default musl
// allocator turns that churn into a syscall storm (~10x wall time, 69% sys CPU
// on a multi-MB subtree) — see
// docs/perf/catalog-scan-regression-2026-09-11.md §6 P0. Windows and macOS keep
// their system allocators.
#[cfg(target_os = "linux")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(Server::new);
    // Serve messages strictly one at a time, in arrival order
    // (`concurrency_level(1)`). The open-closure state is notification-driven
    // and cross-document (did_open/did_change/did_close mutate the shared
    // repository + closure), so handlers must observe each other's effects in
    // client order — see tower-lsp-server#36. The default concurrency of 4
    // gives no such ordering. Trade-off: `$/cancelRequest` cannot preempt a
    // running handler; handlers are quick (compile is cached per generation).
    tower_lsp_server::Server::new(stdin, stdout, socket)
        .concurrency_level(1)
        .serve(service)
        .await;
}
