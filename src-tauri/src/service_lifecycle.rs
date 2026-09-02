use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use dcl_launcher_ipc::pidfile;
use dcl_launcher_ipc::protocol::{Command, ResponseData, ShutdownReason};
use dcl_launcher_ipc::transport::IpcClient;
use log::{info, warn};

const CONNECT_TOTAL_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_POLL_INTERVAL: Duration = Duration::from_millis(200);
const GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const KILL_TIMEOUT: Duration = Duration::from_secs(3);

/// Ensures a compatible service is running and returns a connected client.
///
/// Flow: connect → hello → version check; an outdated (or unresponsive)
/// service is stopped (gracefully, then killed via the validated pid file)
/// and the shipped sibling binary is spawned in its place.
pub async fn ensure_service() -> Result<IpcClient> {
    if let Ok(client) = IpcClient::connect().await {
        match hello(client).await {
            Ok((client, service_version)) => {
                if service_version == dcl_launcher_shared::app_version() {
                    return Ok(client);
                }
                info!(
                    "Service version {service_version} does not match own {}; restarting it",
                    dcl_launcher_shared::app_version()
                );
                shutdown_and_wait(ShutdownReason::Outdated).await;
            }
            Err(e) => {
                warn!("The running service did not answer hello, restarting it: {e:#}");
                kill_by_pidfile().await;
            }
        }
    } else if let Some(entry) = pidfile::read() {
        if pidfile::is_alive(&entry) {
            warn!(
                "Service pid {} is alive but its endpoint is unreachable, killing it",
                entry.pid
            );
            kill_by_pidfile().await;
        }
    }

    spawn_service()?;
    let mut client = connect_with_retry(CONNECT_TOTAL_TIMEOUT).await?;

    match request_hello(&mut client).await {
        Ok(service_version) => {
            if service_version != dcl_launcher_shared::app_version() {
                warn!(
                    "Freshly spawned service reports version {service_version}, own is {}",
                    dcl_launcher_shared::app_version()
                );
            }
        }
        Err(e) => return Err(e.context("The freshly spawned service did not answer hello")),
    }

    Ok(client)
}

/// Stops the service before the auto-updater replaces binaries on disk.
///
/// On Windows NSIS cannot overwrite a running exe. An `Err` here must abort
/// the install (skip this update round), never the launch flow.
pub async fn stop_service_for_update() -> Result<()> {
    let Some(entry) = pidfile::read() else {
        return Ok(());
    };
    if !pidfile::is_alive(&entry) {
        return Ok(());
    }

    if let Ok(mut client) = IpcClient::connect().await {
        let _ = client
            .request_silent(Command::Shutdown {
                reason: ShutdownReason::Update,
            })
            .await;
    }

    if pidfile::wait_dead(&entry, GRACEFUL_STOP_TIMEOUT).await {
        return Ok(());
    }

    warn!("The service did not stop gracefully for the update, killing it");
    pidfile::kill(&entry);
    if pidfile::wait_dead(&entry, KILL_TIMEOUT).await {
        Ok(())
    } else {
        Err(anyhow!(
            "The launcher service is still running and would hold a file lock"
        ))
    }
}

/// Best-effort: a deeplink arriving while another command is in flight goes
/// through its own short-lived connection.
///
/// Only macOS delivers deeplinks to a running instance (`on_open_url`);
/// Windows always passes them via argv.
#[cfg(target_os = "macos")]
pub async fn inject_deeplink(url: String) {
    match IpcClient::connect().await {
        Ok(mut client) => {
            if let Err(e) = client.request_silent(Command::InjectDeeplink { url }).await {
                warn!("Cannot inject the deeplink into the service: {e:#}");
            }
        }
        Err(_) => {
            info!("Service is not reachable for deeplink injection; kept as pending");
        }
    }
}

/// Best-effort `notifyUIClosed` over a fresh connection (used from the exit
/// hook where no client is at hand).
pub async fn notify_ui_closed() {
    if let Ok(mut client) = IpcClient::connect().await {
        let _ = client.request_silent(Command::NotifyUiClosed).await;
    }
}

async fn hello(mut client: IpcClient) -> Result<(IpcClient, String)> {
    let service_version = request_hello(&mut client).await?;
    Ok((client, service_version))
}

async fn request_hello(client: &mut IpcClient) -> Result<String> {
    let response = client
        .request_silent(Command::Hello {
            protocol_version: dcl_launcher_ipc::PROTOCOL_VERSION,
            app_version: dcl_launcher_shared::app_version().to_owned(),
        })
        .await?;

    match response.data {
        Some(ResponseData::Hello {
            service_version, ..
        }) => Ok(service_version),
        other => Err(anyhow!("Unexpected hello response payload: {other:?}")),
    }
}

async fn shutdown_and_wait(reason: ShutdownReason) {
    if let Ok(mut client) = IpcClient::connect().await {
        let _ = client.request_silent(Command::Shutdown { reason }).await;
    }
    kill_by_pidfile().await;
}

/// Waits for the recorded service to die; escalates to a validated kill.
async fn kill_by_pidfile() {
    let Some(entry) = pidfile::read() else {
        return;
    };
    if pidfile::wait_dead(&entry, GRACEFUL_STOP_TIMEOUT).await {
        return;
    }
    warn!("Service pid {} is still alive, killing it", entry.pid);
    pidfile::kill(&entry);
    if !pidfile::wait_dead(&entry, KILL_TIMEOUT).await {
        warn!("Service pid {} survived the kill attempt", entry.pid);
    }
}

fn service_binary_path() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("Cannot resolve the current executable path")?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow!("The current executable has no parent directory"))?;
    Ok(dir.join(pidfile::SERVICE_PROCESS_NAME))
}

fn spawn_service() -> Result<()> {
    let path = service_binary_path()?;
    let forwarded_args: Vec<String> = std::env::args().skip(1).collect();

    info!("Spawning the launcher service: {}", path.display());

    let mut command = std::process::Command::new(&path);
    command
        .args(forwarded_args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
        .spawn()
        .with_context(|| format!("Cannot spawn the launcher service at {}", path.display()))?;
    Ok(())
}

async fn connect_with_retry(total: Duration) -> Result<IpcClient> {
    let Some(deadline) = tokio::time::Instant::now().checked_add(total) else {
        return IpcClient::connect()
            .await
            .context("Cannot connect to the launcher service");
    };
    loop {
        match IpcClient::connect().await {
            Ok(client) => return Ok(client),
            Err(e) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(anyhow!(
                        "The launcher service did not open its endpoint within {total:?}: {e}"
                    ));
                }
                tokio::time::sleep(CONNECT_POLL_INTERVAL).await;
            }
        }
    }
}
