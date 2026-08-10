//! Layer 2: a stale pidfile (dead/recycled pid) must not stop `ensure_service`
//! from spawning a fresh service.

mod common;

use std::time::Duration;

use anyhow::{Context, Result};
use dcl_launcher_ipc::pidfile;

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn ensure_service_recovers_from_stale_pidfile() -> Result<()> {
    let sandbox = common::init("stale")?;
    let _ = &sandbox.endpoint;

    // A leftover pidfile pointing at a pid that cannot exist.
    let stale = pidfile::ServicePid {
        pid: u32::MAX.saturating_sub(3),
        process_name: pidfile::SERVICE_PROCESS_NAME.to_owned(),
        version: "0.0.1".to_owned(),
        started_at_unix_secs: 0,
    };
    std::fs::create_dir_all(dcl_launcher_shared::app_dir())?;
    std::fs::write(pidfile::path(), serde_json::to_string(&stale)?)?;

    let mut client = tokio::time::timeout(
        Duration::from_secs(30),
        app_lib::service_lifecycle::ensure_service(),
    )
    .await
    .context("ensure_service timed out")??;

    let response = client
        .request_silent(dcl_launcher_ipc::protocol::Command::ViewCurrentState)
        .await?;
    assert!(response.ok);

    let entry = common::wait_pidfile_alive(Duration::from_secs(5)).await?;
    assert_ne!(entry.pid, stale.pid, "the pidfile must be re-written");
    assert_eq!(entry.version, dcl_launcher_shared::app_version());

    app_lib::service_lifecycle::stop_service_for_update().await?;
    Ok(())
}
