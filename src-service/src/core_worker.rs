use std::cell::{Cell, RefCell};
use std::rc::Rc;

use anyhow::{Context, Result, anyhow};
use dcl_launcher_core::analytics::event::Event;
use dcl_launcher_core::app::AppState;
use dcl_launcher_core::channel::EventChannel;
use dcl_launcher_core::utils;
use dcl_launcher_shared::types::Status;
use log::{error, info};
use tokio::sync::{mpsc, oneshot};

use crate::events::EventsHub;

/// All interaction with `dcl-launcher-core` happens on a dedicated thread
/// with a single-threaded runtime + `LocalSet`: core allows
/// `clippy::future_not_send`, so its futures must never be required to be
/// `Send`. Connection handlers talk to this worker through [`CoreHandle`].
pub enum CoreRequest {
    RunFlow {
        retry: bool,
        done: oneshot::Sender<Result<(), String>>,
    },
    TrackUiOpened,
    TrackUiClosed,
    Flush {
        done: oneshot::Sender<()>,
    },
}

#[derive(Clone)]
pub struct CoreHandle {
    tx: mpsc::UnboundedSender<CoreRequest>,
}

impl CoreHandle {
    /// Runs the launch flow and resolves with the flow result; a request
    /// arriving while a flow is in flight joins it instead of starting a
    /// second run. `Err` carries the user-facing message.
    pub async fn run_flow(&self, retry: bool) -> Result<(), String> {
        let (done, wait) = oneshot::channel();
        if self.tx.send(CoreRequest::RunFlow { retry, done }).is_err() {
            return Err("The launcher service is shutting down".to_owned());
        }
        wait.await
            .unwrap_or_else(|_| Err("The launcher service dropped the request".to_owned()))
    }

    pub fn track_ui_opened(&self) {
        let _ = self.tx.send(CoreRequest::TrackUiOpened);
    }

    pub fn track_ui_closed(&self) {
        let _ = self.tx.send(CoreRequest::TrackUiClosed);
    }

    /// Flushes the analytics queue without firing `LAUNCHER_CLOSE` — the
    /// service's own end of life is not a UI close.
    pub async fn flush(&self) {
        let (done, wait) = oneshot::channel();
        if self.tx.send(CoreRequest::Flush { done }).is_ok() {
            let _ = wait.await;
        }
    }
}

/// Spawns the core worker thread. The returned receiver resolves once
/// `AppState::setup()` finished, with its result.
pub fn start(events: EventsHub) -> (CoreHandle, oneshot::Receiver<Result<()>>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let (setup_tx, setup_rx) = oneshot::channel();

    std::thread::Builder::new()
        .name("core-worker".to_owned())
        .spawn(move || worker_thread(rx, setup_tx, events))
        .map_or_else(
            |e| {
                error!("Cannot spawn the core worker thread: {e}");
            },
            |_| (),
        );

    (CoreHandle { tx }, setup_rx)
}

fn worker_thread(
    rx: mpsc::UnboundedReceiver<CoreRequest>,
    setup_tx: oneshot::Sender<Result<()>>,
    events: EventsHub,
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();

    let runtime = match runtime {
        Ok(runtime) => runtime,
        Err(e) => {
            let _ = setup_tx.send(Err(anyhow!("Cannot build the core worker runtime: {e}")));
            return;
        }
    };

    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, worker_loop(rx, setup_tx, events));
}

async fn worker_loop(
    mut rx: mpsc::UnboundedReceiver<CoreRequest>,
    setup_tx: oneshot::Sender<Result<()>>,
    events: EventsHub,
) {
    let app_state = match AppState::setup().await.context("Cannot setup app state") {
        Ok(app_state) => {
            let _ = setup_tx.send(Ok(()));
            Rc::new(app_state)
        }
        Err(e) => {
            let _ = setup_tx.send(Err(e));
            return;
        }
    };

    // Installer-referrer deeplink: seeds the protocol global; an explicit
    // deeplink arriving later with a launch command overwrites it.
    dcl_launcher_core::protocols::Protocol::try_seed_from_startup_location();

    let flow_running = Rc::new(Cell::new(false));
    let flow_waiters: FlowWaiters = Rc::new(RefCell::new(Vec::new()));

    while let Some(request) = rx.recv().await {
        match request {
            CoreRequest::RunFlow { retry, done } => {
                if retry {
                    tokio::task::spawn_local(track(
                        app_state.clone(),
                        Event::RETRY_FLOW_BUTTON_CLICK {
                            version: utils::app_version().to_owned(),
                        },
                    ));
                }
                if flow_running.get() {
                    flow_waiters.borrow_mut().push(done);
                } else {
                    flow_running.set(true);
                    tokio::task::spawn_local(run_flow_task(
                        app_state.clone(),
                        events.clone(),
                        done,
                        flow_waiters.clone(),
                        flow_running.clone(),
                    ));
                }
            }
            CoreRequest::TrackUiOpened => {
                tokio::task::spawn_local(track(
                    app_state.clone(),
                    Event::LAUNCHER_OPEN {
                        version: utils::app_version().to_owned(),
                    },
                ));
            }
            CoreRequest::TrackUiClosed => {
                tokio::task::spawn_local(track(
                    app_state.clone(),
                    Event::LAUNCHER_CLOSE {
                        version: utils::app_version().to_owned(),
                    },
                ));
            }
            CoreRequest::Flush { done } => {
                let app_state = app_state.clone();
                tokio::task::spawn_local(async move {
                    app_state.analytics.lock().await.cleanup().await;
                    let _ = done.send(());
                });
            }
        }
    }

    info!("Core worker stopped: all request senders dropped");
}

type FlowWaiters = Rc<RefCell<Vec<oneshot::Sender<Result<(), String>>>>>;

async fn run_flow_task(
    app_state: Rc<AppState>,
    events: EventsHub,
    done: oneshot::Sender<Result<(), String>>,
    waiters: FlowWaiters,
    running: Rc<Cell<bool>>,
) {
    let channel = BroadcastChannel {
        events: events.clone(),
    };
    let result = app_state
        .flow
        .launch(&channel, app_state.state.clone())
        .await;

    let outcome = match result {
        Ok(()) => {
            events.set_idle();
            Ok(())
        }
        Err(flow_error) => {
            events.publish(Status::Error {
                message: flow_error.user_message.clone(),
            });
            Err(flow_error.user_message)
        }
    };

    // Same-thread LocalSet: no waiter can be pushed between clearing the
    // flag and draining, so every joiner gets exactly this outcome.
    running.set(false);
    let _ = done.send(outcome.clone());
    for waiter in waiters.borrow_mut().drain(..) {
        let _ = waiter.send(outcome.clone());
    }
}

async fn track(app_state: Rc<AppState>, event: Event) {
    app_state
        .analytics
        .lock()
        .await
        .track_and_flush_silent(event)
        .await;
}

/// Bridges core's sync [`EventChannel`] into the broadcast hub.
struct BroadcastChannel {
    events: EventsHub,
}

impl EventChannel for BroadcastChannel {
    fn send(&self, status: Status) -> Result<()> {
        self.events.publish(status);
        Ok(())
    }
}
