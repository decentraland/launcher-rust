use std::sync::{Arc, Mutex};

use dcl_launcher_ipc as ipc;
use tokio::sync::broadcast;

/// Fan-out point for status events plus the last-known service state that
/// backs the `viewCurrentState` command.
#[derive(Clone)]
pub struct EventsHub {
    tx: broadcast::Sender<ipc::Status>,
    current: Arc<Mutex<ipc::ServiceState>>,
}

impl EventsHub {
    const CHANNEL_CAPACITY: usize = 128;

    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(Self::CHANNEL_CAPACITY);
        Self {
            tx,
            current: Arc::new(Mutex::new(ipc::ServiceState::Idle)),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ipc::Status> {
        self.tx.subscribe()
    }

    /// Publishes a status event to every connected UI and refreshes the
    /// current-state snapshot derived from it.
    pub fn publish(&self, status: ipc::Status) {
        let state = match &status {
            ipc::Status::State { step } => ipc::ServiceState::Busy { step: step.clone() },
            ipc::Status::Error { message } => ipc::ServiceState::Errored {
                message: message.clone(),
            },
        };
        self.set_state(state);
        // Send errors only mean there is no subscriber right now.
        let _ = self.tx.send(status);
    }

    pub fn set_idle(&self) {
        self.set_state(ipc::ServiceState::Idle);
    }

    pub fn snapshot(&self) -> ipc::ServiceState {
        match self.current.lock() {
            Ok(guard) => guard.clone(),
            Err(e) => {
                log::error!("Cannot lock the current-state snapshot: {e}");
                ipc::ServiceState::Idle
            }
        }
    }

    fn set_state(&self, state: ipc::ServiceState) {
        match self.current.lock() {
            Ok(mut guard) => *guard = state,
            Err(e) => log::error!("Cannot lock the current-state snapshot: {e}"),
        }
    }
}
