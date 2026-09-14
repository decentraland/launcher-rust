use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use anyhow::{Context, Result};
use dcl_launcher_core::utils;
use dcl_launcher_ipc::pidfile;
use dcl_launcher_ipc::transport::{BindError, IpcServer};
use log::{info, warn};
use tokio::sync::mpsc;

use crate::core_worker;
use crate::events::EventsHub;
use crate::server::{ConnectionContext, handle_connection};

const FLUSH_TIMEOUT: Duration = Duration::from_secs(3);

pub async fn run() -> Result<()> {
    // Binding the endpoint is the authoritative single-instance lock.
    let mut ipc_server = match IpcServer::bind() {
        Ok(server) => server,
        Err(BindError::AlreadyRunning) => {
            // Logs are not initialized yet (core owns them); stderr is all we have.
            eprintln!("Another launcher service instance is already running, exiting");
            return Ok(());
        }
        Err(BindError::Io(e)) => return Err(e).context("Cannot bind the service IPC endpoint"),
    };

    let events = EventsHub::new();
    let (core, setup_rx) = core_worker::start(events.clone());
    setup_rx
        .await
        .context("The core worker died during setup")?
        .context("Cannot setup the launcher core")?;

    pidfile::write_own(utils::app_version())?;
    info!(
        "Launcher service is running: pid {}, version {}",
        std::process::id(),
        utils::app_version()
    );

    let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
    let ctx = ConnectionContext {
        core: core.clone(),
        events,
        shutdown: shutdown_tx,
        first_hello_consumed: Arc::new(AtomicBool::new(false)),
    };

    let shutdown_reason = loop {
        tokio::select! {
            reason = shutdown_rx.recv() => break reason,
            accepted = ipc_server.accept() => match accepted {
                Ok(connection) => {
                    tokio::spawn(handle_connection(connection, ctx.clone()));
                }
                Err(e) => {
                    warn!("Cannot accept a UI connection: {e}");
                    tokio::time::sleep(Duration::from_millis(200)).await;
                }
            },
        }
    };

    info!("Shutting down the launcher service: {shutdown_reason:?}");

    // Let in-flight response frames drain before tearing the process down.
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Flush analytics WITHOUT firing LAUNCHER_CLOSE: the service's own end of
    // life is not a UI close.
    if tokio::time::timeout(FLUSH_TIMEOUT, core.flush())
        .await
        .is_err()
    {
        warn!("Analytics flush timed out during shutdown");
    }

    pidfile::remove();
    IpcServer::cleanup_endpoint();
    Ok(())
}
