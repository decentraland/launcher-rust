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
#![allow(clippy::uninlined_format_args, clippy::used_underscore_binding)]

use dcl_launcher_core::environment::{AppEnvironment, Args};
use dcl_launcher_core::errors::FlowError;
use dcl_launcher_core::log::{error, info};
use dcl_launcher_core::protocols::Protocol;
use dcl_launcher_core::types::LauncherUpdate;
use dcl_launcher_core::{app::AppState, channel::EventChannel, types};
use std::env;
use std::sync::Arc;
use tauri::async_runtime::Mutex;
use tauri::Url;
use tauri::{ipc::Channel, App, AppHandle, Manager, State};
#[cfg(unix)]
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_updater::UpdaterExt;

type MutState = Arc<Mutex<AppState>>;

pub struct StatusChannel(Channel<types::Status>);

impl EventChannel for StatusChannel {
    fn send(&self, status: types::Status) -> anyhow::Result<()> {
        self.0.send(status)?;
        Ok(())
    }
}

trait EventChannelExt: EventChannel {
    fn send_silent(&self, status: types::Status) {
        if let Err(e) = self.send(status) {
            error!("Error during the message sending: {}", e.to_string());
        }
    }

    fn notify_error(&self, flow_error: &FlowError) {
        self.send_silent(flow_error.into());
    }
}

impl<T: EventChannel + ?Sized> EventChannelExt for T {}

#[tauri::command]
async fn launch(
    app: AppHandle,
    state: State<'_, MutState>,
    channel: Channel<types::Status>,
) -> Result<(), String> {
    let status_channel = StatusChannel(channel);
    let guard = state.lock().await;

    let flow_state = guard.state.clone();

    if let Err(e) = update_if_needed_and_restart(&app, &guard, &status_channel).await {
        error!("Cannot update the launcher: {}", e);
    }

    guard
        .flow
        .launch(&status_channel, flow_state)
        .await
        .map_err(|e| {
            status_channel.notify_error(&e);
            e.user_message
        })?;

    guard.cleanup().await;
    drop(guard);
    app.cleanup_before_exit();
    app.exit(0);

    Ok(())
}

fn current_updater(app: &AppHandle) -> tauri_plugin_updater::Result<tauri_plugin_updater::Updater> {
    let args: Args = AppEnvironment::cmd_args();

    // comparison to support rollbacks
    let builder = app
        .updater_builder()
        .version_comparator(move |current_version, remote| {
            if args.never_trigger_updater {
                info!("Never trigger updater by flag");
                return false;
            }

            if args.always_trigger_updater {
                info!("Always trigger updater by flag");
                return true;
            }

            current_version != remote.version
        });

    if let Some(url) = args.use_updater_url {
        info!("Use custom updater by flag with its value {}", url);
        let parsed_url: Url = Url::parse(url.as_str())?;
        return builder.endpoints(vec![parsed_url])?.build();
    }

    builder.build()
}

async fn update_if_needed_and_restart(
    app: &AppHandle,
    app_state: &AppState,
    channel: &StatusChannel,
) -> tauri_plugin_updater::Result<()> {
    channel.send_silent(LauncherUpdate::CheckingForUpdate.into());
    if let Some(update) = current_updater(app)?.check().await? {
        let mut downloaded: usize = 0;

        let content = update
            .download(
                |chunk_length, content_length| {
                    downloaded = downloaded.saturating_add(chunk_length);
                    info!("downloaded {downloaded} from {content_length:?}");
                    match content_length {
                        Some(length) => {
                            let current = (downloaded as u64).saturating_mul(100);
                            let percentage = current.checked_div(length);

                            match percentage {
                                Some(p) => {
                                    let progress: u8 = p.min(100) as u8;

                                    channel.send_silent(
                                        LauncherUpdate::Downloading {
                                            progress: Some(progress),
                                        }
                                        .into(),
                                    );
                                }
                                None => {
                                    channel.send_silent(
                                        LauncherUpdate::Downloading { progress: None }.into(),
                                    );
                                }
                            }
                        }
                        None => {
                            channel
                                .send_silent(LauncherUpdate::Downloading { progress: None }.into());
                        }
                    }
                },
                || {
                    info!("download finished");
                    channel.send_silent(LauncherUpdate::DownloadFinished.into());
                },
            )
            .await?;

        channel.send_silent(LauncherUpdate::InstallingUpdate.into());
        update.install(content)?;
        info!("update installed");

        channel.send_silent(LauncherUpdate::RestartingApp.into());
        app_state.cleanup().await;
        app.restart();
    }

    Ok(())
}

#[cfg_attr(windows, allow(unused_variables))]
fn setup_deeplink(a: &App, protocol: &Protocol) {
    #[cfg(target_os = "macos")]
    {
        let protocol = protocol.clone();
        a.deep_link().on_open_url(move |event| {
            let urls = event.urls();
            match urls.first() {
                Some(url) => {
                    protocol.try_assign_value(url.to_string());
                }
                None => {
                    error!("No values are provided in deep link");
                }
            }
        });
    }

    #[cfg(target_os = "windows")]
    {
        let args: Vec<String> = AppEnvironment::raw_cmd_args().collect();
        protocol.try_assign_value_from_vec(&args);
    }
}

fn setup(a: &mut App) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let app_state = tauri::async_runtime::block_on(AppState::setup())
        .inspect_err(|e| error!("Error during setup: {:#}", e))?;

    setup_deeplink(a, &app_state.protocol);

    let mut_state: MutState = Arc::new(Mutex::new(app_state));
    a.manage(mut_state);
    Ok(())
}

/// Run the Tauri application.
///
/// # Panics
///
/// This function will panic if the Tauri application fails to run,
/// which can happen if there is an error generating the context or initializing plugins.
#[allow(clippy::expect_used)]
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_deep_link::init())
        .setup(setup)
        .invoke_handler(tauri::generate_handler![launch])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
