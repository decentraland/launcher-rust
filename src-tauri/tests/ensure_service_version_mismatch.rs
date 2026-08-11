//! Layer 2: a running service reporting a different version must be shut
//! down (`shutdown{outdated}`) and replaced by the shipped binary.
//! A fake service built on `IpcServer` is the only practical way to produce
//! an arbitrary version — real binaries have theirs baked in at compile time.

mod common;

use std::time::Duration;

use anyhow::{Context, Result};
use dcl_launcher_ipc::protocol::{Command, Frame, Response, ResponseData};
use dcl_launcher_ipc::transport::IpcServer;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "e2e — run via run-e2e script"]
async fn ensure_service_restarts_an_outdated_service() -> Result<()> {
    let sandbox = common::init("vermm")?;

    // Fake "old" service: answers hello with v0.0.1, honors shutdown by
    // dropping the endpoint (so the real binary can bind it afterwards).
    let mut server = IpcServer::bind_to(Some(&sandbox.endpoint))
        .map_err(|e| anyhow::anyhow!("cannot bind the fake service: {e}"))?;
    let saw_shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw_shutdown_clone = saw_shutdown.clone();

    // `ensure_service` keeps its hello connection open while it opens a
    // second one for the shutdown — the fake must serve connections
    // CONCURRENTLY (like the real service) or it deadlocks.
    let shutdown_signal = std::sync::Arc::new(tokio::sync::Notify::new());
    let fake = {
        let shutdown_signal = shutdown_signal.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = shutdown_signal.notified() => return, // drops `server` -> endpoint freed
                    accepted = server.accept() => {
                        let Ok(connection) = accepted else { return };
                        let saw_shutdown = saw_shutdown_clone.clone();
                        let shutdown_signal = shutdown_signal.clone();
                        tokio::spawn(async move {
                            let (mut reader, mut writer) = connection.split();
                            while let Ok(Some(frame)) = reader.read().await {
                                let Frame::Req { id, cmd } = frame else {
                                    continue;
                                };
                                let (result, is_shutdown) = match cmd {
                                    Command::Hello { .. } => (
                                        Response::ok_with(ResponseData::Hello {
                                            service_version: "0.0.1".to_owned(),
                                            protocol_version: dcl_launcher_ipc::PROTOCOL_VERSION,
                                        }),
                                        false,
                                    ),
                                    Command::Shutdown { .. } => (Response::ok_empty(), true),
                                    _ => (Response::ok_empty(), false),
                                };
                                let _ = writer.write(&Frame::Res { id, result }).await;
                                if is_shutdown {
                                    saw_shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
                                    shutdown_signal.notify_one();
                                    return;
                                }
                            }
                        });
                    }
                }
            }
        })
    };

    let mut client = tokio::time::timeout(
        Duration::from_secs(40),
        app_lib::service_lifecycle::ensure_service(),
    )
    .await
    .context("ensure_service timed out")??;
    let _ = fake.await;

    assert!(
        saw_shutdown.load(std::sync::atomic::Ordering::SeqCst),
        "the outdated service must receive shutdown{{outdated}}"
    );

    let response = client.request_silent(Command::ViewCurrentState).await?;
    assert!(response.ok);

    let entry = common::wait_pidfile_alive(Duration::from_secs(5)).await?;
    assert_eq!(
        entry.version,
        dcl_launcher_shared::app_version(),
        "the shipped binary must now be running"
    );

    app_lib::service_lifecycle::stop_service_for_update().await?;
    Ok(())
}
