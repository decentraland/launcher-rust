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
// TODO solve later
#![allow(clippy::significant_drop_tightening)]

pub mod analytics;
pub mod app;
mod auto_auth;
pub mod channel;
mod config;
mod deeplink_bridge;
pub mod environment;
pub mod errors;
pub mod flow;
pub mod installs;
pub mod instances;
mod logs;
mod monitoring;
mod processes;
pub mod protocols;
pub mod s3;
pub mod types;
pub mod utils;

pub use log;
