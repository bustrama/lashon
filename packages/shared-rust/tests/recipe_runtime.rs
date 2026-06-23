//! Integration tests for the M9 Phase 1b recipe runtime
//! (`lashon_core::recipes::runtime`).
//!
//! Exercises the executor end-to-end with [`AlwaysAllow`] / [`AlwaysDeny`]
//! confirmation handlers on flows that don't touch the desktop —
//! clipboard read/write, slot interpolation, run_shell with capture,
//! confirmation gating, error surface on bad interpolation. Mouse /
//! keyboard / window steps are covered by the unit tests in the
//! module itself; the integration tests focus on the
//! `execute_recipe → step → ConfirmHandler` boundary that the unit
//! tests can't exercise without spinning up a temp clipboard state.

use std::collections::HashMap;

use lashon_core::recipes::{
    execute_recipe_for_os, AlwaysAllow, AlwaysDeny, OsSteps, Recipe, RuntimeError, Step,
};

/// Builds a recipe with the given step list as its Windows variant.
/// Keeps the test sites tight: only the steps under test vary per case.
fn recipe_with_windows_steps(steps: Vec<Step>) -> Recipe {
    Recipe {
        version: 1,
        id: "integration-fixture".into(),
        name: "Integration fixture".into(),
        description: "Test-only recipe.".into(),
        long_description: None,
        author: None,
        recipe_version: "1.0.0".into(),
        tags: vec![],
        intents: vec![],
        parameters: vec![],
        permissions: vec![],
        os_steps: OsSteps {
            windows: Some(steps),
            macos: None,
            linux: None,
        },
    }
}

/// On Windows the integration test exercises the real `powershell.exe`
/// path; on POSIX hosts (CI macOS / Linux runners) it exercises
/// `/bin/sh`. The recipe step's shell command needs to be portable
/// across both; `echo` is on every shell.
fn echo_command() -> String {
    "echo hello".to_string()
}

#[tokio::test]
#[ignore = "needs a real clipboard backend — X11/Wayland on Linux, blocked on headless CI"]
async fn clipboard_write_then_read_round_trips_through_runtime() {
    // Seed the clipboard with a known sentinel via a `clipboard_set`
    // step, then read it back via `clipboard_get_into` to a recipe
    // variable. Asserting the variable lands proves the step-local
    // var path works end-to-end.
    //
    // Skipped by default — `arboard::Clipboard::new()` panics on
    // Ubuntu CI ("X11 server connection timed out") and there's no
    // useful headless clipboard backend. Same posture as
    // `tests/inject.rs`'s ignored test. Run on a desktop with:
    // `cargo test -p lashon-core --test recipe_runtime -- --ignored`.
    let recipe = recipe_with_windows_steps(vec![
        Step::ClipboardSet {
            text: "lashon-runtime-integration-sentinel".into(),
            comment: None,
        },
        Step::ClipboardGetInto {
            var: "stash".into(),
            comment: None,
        },
    ]);
    let run = execute_recipe_for_os(&recipe, "windows", HashMap::new(), &AlwaysAllow)
        .await
        .expect("clipboard round-trip succeeds");
    assert_eq!(run.steps_executed, 2);
    assert_eq!(
        run.variables.get("stash").map(String::as_str),
        Some("lashon-runtime-integration-sentinel"),
        "clipboard_get_into must populate the named recipe var"
    );
}

#[tokio::test]
async fn run_shell_with_always_allow_executes_and_captures() {
    let recipe = recipe_with_windows_steps(vec![Step::RunShell {
        command: echo_command(),
        timeout_ms: 10_000,
        capture_into: Some("out".into()),
        dry_run: false,
        comment: None,
    }]);
    let run = execute_recipe_for_os(&recipe, "windows", HashMap::new(), &AlwaysAllow)
        .await
        .expect("AlwaysAllow lets the run_shell complete");
    assert_eq!(run.steps_executed, 1);
    let captured = run
        .variables
        .get("out")
        .map(String::as_str)
        .expect("capture_into must populate the named var");
    assert!(
        captured.contains("hello"),
        "captured stdout must include the echoed string: {captured:?}"
    );
}

#[tokio::test]
async fn run_shell_with_always_deny_aborts_without_executing() {
    let recipe = recipe_with_windows_steps(vec![
        Step::RunShell {
            command: echo_command(),
            timeout_ms: 10_000,
            capture_into: Some("would_be_captured".into()),
            dry_run: false,
            comment: None,
        },
        // A second step we expect NEVER to run because the first
        // aborts on denial.
        Step::ClipboardSet {
            text: "should-not-land".into(),
            comment: None,
        },
    ]);
    let err = execute_recipe_for_os(&recipe, "windows", HashMap::new(), &AlwaysDeny)
        .await
        .expect_err("AlwaysDeny must abort the run");
    match err {
        RuntimeError::Denied { kind, index } => {
            assert_eq!(kind, "run_shell");
            assert_eq!(index, 0, "denial must fire on the first run_shell step");
        }
        other => panic!("expected Denied, got {other:?}"),
    }
}

#[tokio::test]
async fn slot_values_substitute_into_shell_capture() {
    // Slot interpolation needs to survive through to the captured
    // output — `{{ greeting }}` interpolates to "hello", the shell
    // echoes "hello", `out` captures "hello". This exercises the
    // pipeline from `args` → `interpolate` → `run_shell` →
    // `capture_into` in one shot.
    let recipe = recipe_with_windows_steps(vec![Step::RunShell {
        command: "echo {{ greeting }}".into(),
        timeout_ms: 10_000,
        capture_into: Some("out".into()),
        dry_run: false,
        comment: None,
    }]);
    let mut args = HashMap::new();
    args.insert("greeting".to_string(), "hola".to_string());
    let run = execute_recipe_for_os(&recipe, "windows", args, &AlwaysAllow)
        .await
        .unwrap();
    let captured = run.variables.get("out").map(String::as_str).unwrap_or("");
    assert!(
        captured.contains("hola"),
        "interpolated slot must reach the captured stdout: {captured:?}"
    );
}

#[tokio::test]
async fn unknown_slot_aborts_before_side_effects() {
    // The first step has an unknown `{{ ghost }}` reference; it must
    // abort *before* attempting any clipboard side effect, so the
    // sentinel from a real environment's clipboard would survive the
    // aborted run. The test asserts the right error variant rather
    // than poking the OS clipboard.
    let recipe = recipe_with_windows_steps(vec![Step::ClipboardSet {
        text: "{{ ghost }}".into(),
        comment: None,
    }]);
    let err = execute_recipe_for_os(&recipe, "windows", HashMap::new(), &AlwaysAllow)
        .await
        .unwrap_err();
    match err {
        RuntimeError::UnknownInterpolation { name } => assert_eq!(name, "ghost"),
        other => panic!("expected UnknownInterpolation, got {other:?}"),
    }
}

#[tokio::test]
async fn wait_ms_step_returns_in_roughly_the_right_time() {
    // Lower bound only — sleep precision varies across CI runners and
    // a tight upper bound creates flake. 0 ms `wait_ms` should also
    // be valid (degenerate but legal).
    let started = std::time::Instant::now();
    let recipe = recipe_with_windows_steps(vec![Step::WaitMs {
        ms: 100,
        comment: None,
    }]);
    execute_recipe_for_os(&recipe, "windows", HashMap::new(), &AlwaysAllow)
        .await
        .unwrap();
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(80),
        "wait_ms 100 should sleep at least ~80 ms (allowing scheduler jitter)"
    );
}
