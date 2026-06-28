//! End-to-end integration test for the `lashon-mcp` stdio binary.
//!
//! Spawns the built binary as a subprocess, drives the JSON-RPC handshake
//! over its stdin/stdout, and asserts that:
//!
//! 1. The server initialises and advertises `tools` capability.
//! 2. `tools/list` returns the five Phase 1g v1 tools.
//! 3. `tools/call list_recipes` returns a non-empty array containing the
//!    bundled starters (the 10 recipes in `recipes/starters/`).
//!
//! Run with:
//!
//! ```text
//! cargo test -p lashon-core --test lashon_mcp_stdio --features mcp-server
//! ```
//!
//! Skipped in `--no-default-features` builds (the binary doesn't exist
//! when `mcp-server` is off).

#![cfg(feature = "mcp-server")]

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;

/// Locate the built `lashon-mcp` binary. `CARGO_BIN_EXE_lashon-mcp` is
/// set by Cargo for integration tests of crates that declare a `[[bin]]`
/// — it's the canonical way to find sibling binaries without baking in
/// platform-specific path logic.
fn lashon_mcp_path() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_lashon-mcp"))
}

/// `recipes/starters/` for the bundled starter library. Same resolution
/// the bin's defaults use — kept in sync by walking up from this
/// integration test's manifest dir.
fn bundled_recipes_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../recipes/starters")
}

/// One in-flight request/response exchange over the spawned subprocess.
/// Each request is a single line of JSON terminated with `\n`; each
/// response likewise.
async fn rpc_round_trip(
    stdin: &mut tokio::process::ChildStdin,
    stdout: &mut BufReader<tokio::process::ChildStdout>,
    request: Value,
) -> Value {
    let mut line = serde_json::to_string(&request).expect("serialise request");
    line.push('\n');
    stdin
        .write_all(line.as_bytes())
        .await
        .expect("write request to lashon-mcp stdin");
    stdin.flush().await.expect("flush stdin");

    // Loop until we read a non-empty line. The server may emit blank
    // lines or progress notifications we don't care about in this test.
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = timeout(Duration::from_secs(15), stdout.read_line(&mut buf))
            .await
            .expect("response within 15 s timeout")
            .expect("read response line");
        assert!(n > 0, "lashon-mcp closed stdout unexpectedly");
        let trimmed = buf.trim();
        if trimmed.is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(trimmed)
            .unwrap_or_else(|err| panic!("non-JSON line {trimmed:?}: {err}"));
        // Skip notifications — we want the response to our request.
        if value.get("id").is_some() {
            return value;
        }
    }
}

#[tokio::test]
async fn initialise_then_list_tools_then_call_list_recipes() {
    let binary = lashon_mcp_path();
    assert!(
        binary.is_file(),
        "lashon-mcp binary not found at {} — was the crate built?",
        binary.display()
    );

    let starters = bundled_recipes_dir();
    assert!(
        starters.is_dir(),
        "bundled starters dir not found at {}",
        starters.display()
    );

    // Point the spawned binary at this checkout's starter recipes, and
    // override the per-user dir to a throwaway temp path so the test
    // doesn't accidentally write into the dev's real recipes dir.
    let temp_user = std::env::temp_dir().join(format!(
        "lashon-mcp-test-user-recipes-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_user);

    let mut cmd = Command::new(&binary);
    cmd.env("LASHON_BUNDLED_RECIPES_DIR", &starters)
        .env("LASHON_USER_RECIPES_DIR", &temp_user)
        .env("RUST_LOG", "warn")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // CREATE_NO_WINDOW — the lashon-mcp binary is a console-subsystem program;
    // spawning it from a console-less parent (an IDE or background test runner)
    // pops a console window that steals foreground focus. Mirrors the production
    // spawn sites (run_command, recipe runtime, sidecar). See
    // .claude/rules/recipes.md.
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().expect("spawn lashon-mcp");

    let mut stdin = child.stdin.take().expect("stdin pipe");
    let stdout = child.stdout.take().expect("stdout pipe");
    let mut stdout = BufReader::new(stdout);

    // ----- initialize handshake -----
    let init_resp = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "lashon-mcp-tests", "version": "0.0.0" }
            }
        }),
    )
    .await;
    assert_eq!(init_resp["id"], json!(1));
    let server_info = &init_resp["result"]["serverInfo"];
    assert_eq!(
        server_info["name"],
        json!("lashon-mcp"),
        "advertised server name should be the explicitly-set MCP_SERVER_NAME: {init_resp}"
    );
    let caps = &init_resp["result"]["capabilities"];
    assert!(
        caps.get("tools").is_some(),
        "tools capability missing: {init_resp}"
    );

    // The MCP spec requires the client to send `notifications/initialized`
    // before regular requests. Notifications have no `id` and expect no
    // response — write directly without going through the round-trip helper.
    let mut notify = serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }))
    .unwrap();
    notify.push('\n');
    stdin.write_all(notify.as_bytes()).await.unwrap();
    stdin.flush().await.unwrap();

    // ----- tools/list -----
    let list_resp = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }),
    )
    .await;
    let tools = list_resp["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list missing tools array: {list_resp}"));
    let names: std::collections::HashSet<&str> =
        tools.iter().filter_map(|t| t["name"].as_str()).collect();
    for expected in [
        "list_recipes",
        "get_recipe",
        "validate_recipe",
        "save_recipe",
        "list_recipe_step_types",
    ] {
        assert!(
            names.contains(expected),
            "tools/list missing {expected:?}, got {names:?}"
        );
    }

    // ----- tools/call list_recipes -----
    let call_resp = rpc_round_trip(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {
                "name": "list_recipes",
                "arguments": {}
            }
        }),
    )
    .await;
    let content = call_resp["result"]["content"]
        .as_array()
        .unwrap_or_else(|| panic!("call response missing content: {call_resp}"));
    let text = content
        .iter()
        .find_map(|c| c["text"].as_str())
        .unwrap_or_else(|| panic!("no text content in call response: {call_resp}"));
    let listings: Value = serde_json::from_str(text).expect("list_recipes returns parseable JSON");
    let arr = listings
        .as_array()
        .expect("list_recipes returns a JSON array");
    assert!(
        arr.len() >= 10,
        "list_recipes should surface the 10 bundled starters, got {}",
        arr.len()
    );
    let ids: std::collections::HashSet<&str> =
        arr.iter().filter_map(|r| r["id"].as_str()).collect();
    assert!(
        ids.contains("send-discord-message"),
        "send-discord-message starter missing from listing: {ids:?}"
    );
    assert!(
        ids.contains("lock-workstation"),
        "lock-workstation starter missing from listing: {ids:?}"
    );

    // Cleanup
    drop(stdin);
    let _ = timeout(Duration::from_secs(5), child.wait()).await;
    let _ = child.kill().await;
    let _ = std::fs::remove_dir_all(&temp_user);
}
