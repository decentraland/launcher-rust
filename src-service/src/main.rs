// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
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

mod core_worker;
mod events;
mod server;
mod service;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    match service::run().await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            log::error!("Service stopped with an error: {e:#}");
            eprintln!("Service stopped with an error: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
