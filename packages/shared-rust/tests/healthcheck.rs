//! End-to-end smoke test: spawn the real STT sidecar and probe HealthCheck.
//!
//! Marked `#[ignore]` — it needs a Python interpreter with the sidecar's
//! dependencies installed, plus `PYTHONPATH` set (handled by the dev-mode
//! launcher) or `LASHON_STT_SIDECAR` pointing at a built binary. CI runs it
//! explicitly once the Python environment is ready:
//!
//! ```text
//! cargo test -p lashon-core --test healthcheck -- --ignored
//! ```

use lashon_core::sidecar::{self, SidecarState};

#[tokio::test]
#[ignore = "requires the Python STT sidecar environment (see module docs)"]
async fn sidecar_healthcheck_reports_serving() {
    let state = SidecarState::default();
    let report = sidecar::healthcheck(&state).await;
    assert!(
        report.ok,
        "STT sidecar HealthCheck was not OK: {}",
        report.detail
    );
}
