//! Layer 2: the auto-updater's pre-install hook. Graceful stop of a healthy
//! service; kill-escalation when the recorded service cannot be reached
//! over IPC (its endpoint differs -> connect fails -> wait -> validated kill).

mod common;

use std::time::Duration;

use anyhow::{Context, Result};
use dcl_launcher_ipc::pidfile;

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn stop_for_update_graceful_then_kill_escalation() -> Result<()> {
    let sandbox = common::init("stopupd")?;

    // (a) Graceful: a healthy reachable service stops on request.
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

    app_lib::service_lifecycle::stop_service_for_update().await?;
    assert!(!pidfile::is_alive(&entry), "the service must be dead");
    assert!(
        pidfile::read().is_none(),
        "a graceful shutdown removes the pidfile"
    );

    // (b) Escalation: the recorded service is alive but unreachable (wrong
    // endpoint) -> graceful path cannot connect -> validated kill.
    let service = common::service_binary_source_dir()?.join(pidfile::SERVICE_PROCESS_NAME);
    let unreachable = std::process::Command::new(service)
        .env("DCL_LAUNCHER_BASE_DIR", &sandbox.base)
        .env(
            "DCL_LAUNCHER_IPC_ENDPOINT",
            format!("{}other", sandbox.endpoint),
        )
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let unreachable_pid = unreachable.id();
    let entry = common::wait_pidfile_alive(Duration::from_secs(10)).await?;
    assert_eq!(entry.pid, unreachable_pid);

    tokio::time::timeout(
        Duration::from_secs(30),
        app_lib::service_lifecycle::stop_service_for_update(),
    )
    .await
    .context("stop_service_for_update timed out")??;
    assert!(
        !pidfile::is_alive(&entry),
        "the unreachable service must be killed"
    );

    Ok(())
}
