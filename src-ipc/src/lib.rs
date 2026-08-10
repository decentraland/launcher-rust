#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::todo,
    clippy::dbg_macro
)]
#![allow(
    clippy::uninlined_format_args,
    clippy::missing_errors_doc,
    clippy::option_if_let_else,
    clippy::single_match_else,
    clippy::must_use_candidate,
    clippy::future_not_send,
    clippy::enum_glob_use
)]

//! IPC contract between the thin launcher UI (`src-tauri`) and the resident
//! background service (`src-service`): wire protocol (status types live in
//! `dcl_launcher_shared::types`), transport (Windows named pipes / Unix
//! domain sockets), and the `current-service-pid.txt` discovery file.

pub mod pidfile;
pub mod protocol;
pub mod transport;

pub use protocol::{Command, Frame, Response, ResponseData, ServiceState, ShutdownReason};
pub use transport::{BindError, IpcClient, IpcConnection, IpcServer};

/// Bumped on any breaking change to [`protocol::Frame`] or its payloads.
pub const PROTOCOL_VERSION: u32 = 1;
