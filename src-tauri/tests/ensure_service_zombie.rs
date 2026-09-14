//! Layer 2: a service whose pid is alive but whose endpoint is unreachable
//! (zombie) must be killed and replaced by `ensure_service`.

mod common;

use std::time::Duration;

use anyhow::{Context, Result};
use dcl_launcher_ipc::pidfile;

#[tokio::test]
#[ignore = "e2e — run via run-e2e script"]
async fn ensure_service_replaces_a_zombie() -> Result<()> {
    let sandbox = common::init("zombie")?;

    // A real service process, name-valid and alive, but listening on a
    // DIFFERENT endpoint than this test uses -> unreachable = zombie.
    let service = common::service_binary_source_dir()?.join(pidfile::SERVICE_PROCESS_NAME);
    let zombie = std::process::Command::new(service)
        .env("DCL_LAUNCHER_BASE_DIR", &sandbox.base)
        .env(
            "DCL_LAUNCHER_IPC_ENDPOINT",
            format!("{}zombie", sandbox.endpoint),
        )
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let zombie_pid = zombie.id();

    // Wait for the zombie to write its pidfile.
    let entry = common::wait_pidfile_alive(Duration::from_secs(10)).await?;
    assert_eq!(entry.pid, zombie_pid);

    let mut client = tokio::time::timeout(
        Duration::from_secs(40),
        app_lib::service_lifecycle::ensure_service(),
    )
    .await
    .context("ensure_service timed out")??;

    let response = client
        .request_silent(dcl_launcher_ipc::protocol::Command::ViewCurrentState)
        .await?;
    assert!(response.ok);

    let fresh = common::wait_pidfile_alive(Duration::from_secs(5)).await?;
    assert_ne!(fresh.pid, zombie_pid, "the zombie must be replaced");

    app_lib::service_lifecycle::stop_service_for_update().await?;
    Ok(())
}
