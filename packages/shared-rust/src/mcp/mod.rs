//! M9 Phase 1g — Ottid as MCP server (ADR-0028).
//!
//! Exposes Ottid's tool catalogue and recipe management as MCP tools so
//! any agent host (Claude Desktop, Cursor, GPT-via-MCP-client, etc.) can
//! drive Ottid over the Model Context Protocol. The stdio binary
//! `ottid-mcp` ([`src/bin/ottid_mcp.rs`](../bin/ottid_mcp.rs)) is the
//! consumer; this module is the SDK-shaped server it serves.
//!
//! Lives behind the `mcp-server` Cargo feature so a library-only
//! consumer of `ottid-core` doesn't pull the rmcp transport stack.
//!
//! ## Security posture
//!
//! - **Default-off in the Hub.** The Tauri shell never spawns
//!   `ottid-mcp` automatically; the user enables it from the Hub MCP
//!   Server tab (Phase 1g Hub work, follow-up PR) and then runs
//!   Claude Desktop with the copied config snippet.
//! - **Safe set always on.** The recipe-management tools and
//!   read-only safe-tools are exposed unconditionally. Interactive
//!   and destructive tools (`click_element`, `run_shell`, `file_*`)
//!   are NOT exposed by this module and will not ship in Phase 1g —
//!   they require the M11+ confirmation modal in the Tauri shell,
//!   which the stdio binary has no path to.
//! - **Trust boundary = the spawning agent.** stdio MCP runs as a
//!   child process of the agent host. The host's identity is the
//!   trust boundary; Ottid does not re-authenticate per request.
//!   This matches the STT sidecar's "loopback trust + per-process
//!   token" model (`docs/adr/0010`) adapted to stdio's process
//!   parentage instead of TCP loopback.
//!
//! ## Tool naming
//!
//! Unprefixed snake_case (`list_recipes`, not `ottid.list_recipes`).
//! MCP convention is one server per dotted prefix and the prefix is
//! the client's responsibility — Claude Desktop already labels
//! Ottid's tools as "Ottid: list_recipes" in its UI. Matching
//! Ottid's internal dispatcher names also means recipe authors can
//! use the same name from voice + from MCP without translation.

pub mod recipe_tools;
pub mod server;

pub use server::{OttidMcpServer, MCP_SERVER_NAME, MCP_SERVER_VERSION};
