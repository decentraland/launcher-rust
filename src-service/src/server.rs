use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use dcl_launcher_core::protocols::Protocol;
use dcl_launcher_core::utils;
use dcl_launcher_ipc as ipc;
use dcl_launcher_ipc::protocol::{Command, Frame, Response, ResponseData, ShutdownReason};
use dcl_launcher_ipc::transport::IpcConnection;
use log::{info, warn};
use tokio::sync::mpsc;

use crate::core_worker::CoreHandle;
use crate::events::EventsHub;

#[derive(Clone)]
pub struct ConnectionContext {
    pub core: CoreHandle,
    pub events: EventsHub,
    pub shutdown: mpsc::Sender<ShutdownReason>,
    /// `AppState::setup()` already fired `LAUNCHER_OPEN` for the UI that
    /// started the service; later hellos fire their own.
    pub first_hello_consumed: Arc<AtomicBool>,
}

pub async fn handle_connection(connection: IpcConnection, ctx: ConnectionContext) {
    let (mut reader, mut writer) = connection.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<Frame>();

    let writer_task = tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            if let Err(e) = writer.write(&frame).await {
                warn!("Cannot write to a UI connection, closing: {e:#}");
                break;
            }
        }
    });

    let mut event_rx = ctx.events.subscribe();
    let event_out = out_tx.clone();
    let event_task = tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(status) => {
                    if event_out.send(Frame::Event { status }).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                    warn!("A UI connection lagged behind by {missed} status events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    loop {
        match reader.read().await {
            Ok(Some(Frame::Req { id, cmd })) => {
                let result = handle_command(cmd, &ctx).await;
                if out_tx.send(Frame::Res { id, result }).is_err() {
                    break;
                }
            }
            Ok(Some(other)) => {
                warn!("Ignoring a non-request frame from a UI: {other:?}");
            }
            Ok(None) => break,
            Err(e) => {
                warn!("Cannot read from a UI connection, closing: {e:#}");
                break;
            }
        }
    }

    event_task.abort();
    drop(out_tx);
    let _ = writer_task.await;
}

async fn handle_command(cmd: Command, ctx: &ConnectionContext) -> Response {
    match cmd {
        Command::Hello {
            protocol_version,
            app_version,
        } => {
            info!(
                "UI hello: app version {app_version}, protocol {protocol_version} (own protocol {})",
                ipc::PROTOCOL_VERSION
            );
            if ctx.first_hello_consumed.swap(true, Ordering::SeqCst) {
                ctx.core.track_ui_opened();
            }
            Response::ok_with(ResponseData::Hello {
                service_version: utils::app_version().to_owned(),
                protocol_version: ipc::PROTOCOL_VERSION,
            })
        }
        Command::Launch { deeplink } => {
            if let Some(url) = deeplink {
                Protocol::new().try_assign_value(url);
            }
            run_flow(ctx, false).await
        }
        Command::Retry => run_flow(ctx, true).await,
        Command::ViewCurrentState => Response::ok_with(ResponseData::CurrentState {
            state: ctx.events.snapshot(),
        }),
        Command::InjectDeeplink { url } => {
            Protocol::new().try_assign_value(url);
            Response::ok_empty()
        }
        Command::Shutdown { reason } => {
            info!("Shutdown requested over IPC: {reason:?}");
            if ctx.shutdown.send(reason).await.is_err() {
                warn!("Shutdown already in progress");
            }
            Response::ok_empty()
        }
        Command::NotifyUiClosed => {
            ctx.core.track_ui_closed();
            Response::ok_empty()
        }
    }
}

async fn run_flow(ctx: &ConnectionContext, retry: bool) -> Response {
    match ctx.core.run_flow(retry).await {
        Ok(()) => Response::ok_empty(),
        Err(user_message) => Response::err(user_message),
    }
}
