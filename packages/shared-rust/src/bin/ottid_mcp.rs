//! `ottid-mcp` — stdio MCP server exposing Ottid's recipe-management
//! tools to any agent host (Claude Desktop, Cursor, GPT-via-MCP-client).
//! ADR-0028.
//!
//! Lifecycle: the agent host spawns this binary as a child process and
//! speaks JSON-RPC 2.0 over its stdin/stdout pipes. We pump tracing to
//! **stderr** — stdout is the MCP transport and any byte we write there
//! that isn't JSON-RPC breaks the protocol.

use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use tracing_subscriber::EnvFilter;

use ottid_core::mcp::OttidMcpServer;

#[tokio::main]
async fn main() -> Result<()> {
    // tracing → stderr. Default to INFO; respect RUST_LOG / OTTID_LOG
    // when the user wants more verbose diagnostics. ANSI off so the
    // log stays readable when the host captures it to a file.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    tracing::info!(
        version = ottid_core::mcp::MCP_SERVER_VERSION,
        "starting ottid-mcp"
    );

    let service = OttidMcpServer::new()
        .serve(stdio())
        .await
        .inspect_err(|err| tracing::error!("rmcp serve error: {err:?}"))?;

    service.waiting().await?;
    Ok(())
}
