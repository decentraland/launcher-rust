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
    clippy::used_underscore_binding,
    clippy::missing_errors_doc
)]

mod logging;
// Public for the Layer 2 integration tests (src-tauri/tests/).
pub mod service_lifecycle;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use dcl_launcher_ipc as ipc;
use dcl_launcher_shared::environment::{deeplink_from_env, AppEnvironment};
use dcl_launcher_shared::types::{LauncherUpdate, Status};
use ipc::protocol::Command;
use log::{error, info};
use tauri::Url;
use tauri::{ipc::Channel, App, AppHandle, Manager, State};
#[cfg(target_os = "macos")]
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_updater::UpdaterExt;

/// The thin UI owns no launcher logic: it renders [`Status`] frames
/// streamed by the background service and forwards commands to it.
pub struct UiState {
    pending_deeplink: Mutex<Option<String>>,
    ui_close_notified: AtomicBool,
}

impl UiState {
    fn take_pending_deeplink(&self) -> Option<String> {
        self.pending_deeplink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    fn peek_pending_deeplink(&self) -> Option<String> {
        self.pending_deeplink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    #[cfg(target_os = "macos")]
    fn set_pending_deeplink(&self, url: String) {
        *self
            .pending_deeplink
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(url);
    }
}

type MutState = Arc<UiState>;

struct UiChannel(Channel<Status>);

impl UiChannel {
    fn send_silent(&self, status: Status) {
        info!("UI send status: {status:?}");

        if let Err(e) = self.0.send(status) {
            error!("Error during the message sending: {}", e);
        }
    }

    fn send_error(&self, message: &str) {
        self.send_silent(Status::Error {
            message: message.to_owned(),
        });
    }
}

#[tauri::command]
async fn retry(
    app: AppHandle,
    state: State<'_, MutState>,
    channel: Channel<Status>,
) -> Result<(), String> {
    info!("tauri command: retry");
    launch_internal(app, state, channel, true).await
}

#[tauri::command]
async fn launch(
    app: AppHandle,
    state: State<'_, MutState>,
    channel: Channel<Status>,
) -> Result<(), String> {
    info!("tauri command: launch");
    launch_internal(app, state, channel, false).await
}

async fn launch_internal(
    app: AppHandle,
    state: State<'_, MutState>,
    channel: Channel<Status>,
    retry_flow: bool,
) -> Result<(), String> {
    let ui_channel = UiChannel(channel);

    if let Err(e) = update_if_needed_and_restart(&app, &state, &ui_channel).await {
        error!("Cannot update the launcher: {:#}", e);
    }

    let mut client = service_lifecycle::ensure_service().await.map_err(|e| {
        error!("Cannot ensure the launcher service: {e:#}");
        let message = "Cannot start the launcher background service".to_owned();
        ui_channel.send_error(&message);
        message
    })?;

    // The UI owns argv: every flow command carries the effective arguments
    // (argv merged with `config.json`), so the service never parses its own.
    let args = AppEnvironment::cmd_args();
    let cmd = if retry_flow {
        Command::Retry { args }
    } else {
        Command::Launch {
            deeplink: state.take_pending_deeplink(),
            args,
        }
    };

    let response = client
        .request(cmd, |status| ui_channel.send_silent(status))
        .await
        .map_err(|e| {
            error!("Lost the service connection during the flow: {e:#}");
            let message = "Lost the connection to the launcher background service".to_owned();
            ui_channel.send_error(&message);
            message
        })?;

    if !response.ok {
        let message = response
            .user_message
            .unwrap_or_else(|| "Unknown launch error".to_owned());
        ui_channel.send_error(&message);
        return Err(message);
    }

    if !state.ui_close_notified.swap(true, Ordering::SeqCst) {
        let _ = client.request_silent(Command::NotifyUiClosed).await;
    }
    app.cleanup_before_exit();
    app.exit(0);

    Ok(())
}

fn current_updater(app: &AppHandle) -> tauri_plugin_updater::Result<tauri_plugin_updater::Updater> {
    info!("Begin current_updater");
    let updater_args = AppEnvironment::cmd_args();
    let use_updater_url = updater_args.use_updater_url.clone();

    // comparison to support rollbacks
    let builder = app
        .updater_builder()
        .version_comparator(move |current_version, remote| {
            if updater_args.never_trigger_updater {
                info!("Never trigger updater by flag");
                return false;
            }

            if updater_args.always_trigger_updater {
                info!("Always trigger updater by flag");
                return true;
            }

            current_version != remote.version
        });

    if let Some(url) = use_updater_url {
        info!("Use custom updater by flag with its value {}", url);
        let parsed_url: Url = Url::parse(url.as_str())?;
        return builder.endpoints(vec![parsed_url])?.build();
    }

    builder.build()
}

async fn update_if_needed_and_restart(
    app: &AppHandle,
    state: &UiState,
    channel: &UiChannel,
) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    match dcl_launcher_shared::is_running_from_dmg() {
        Ok(from_dmg) => {
            if from_dmg {
                info!("App is running from dmg, skipping update since mount is read-only");
                return Ok(());
            }
        }
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Cannot define if dmg or not, skipping update: {}",
                e
            ));
        }
    }

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

        // NSIS cannot replace a running exe: the service must be stopped and
        // confirmed dead before installing. A failure here aborts the install
        // (skip this update round), never the launch flow.
        service_lifecycle::stop_service_for_update()
            .await
            .map_err(|e| {
                anyhow::anyhow!("Cannot stop the launcher service, skipping the update: {e:#}")
            })?;

        channel.send_silent(LauncherUpdate::InstallingUpdate.into());
        update.install(content)?;
        info!("update installed");

        channel.send_silent(LauncherUpdate::RestartingApp.into());

        let mut env = app.env();

        // The service is stopped at this point, so notifyUIClosed has no
        // receiver; this restart intentionally skips the LAUNCHER_CLOSE event.
        app.cleanup_before_exit();

        // Preserve deeplink
        if let Some(deeplink) = state.peek_pending_deeplink() {
            env.args_os.push(deeplink.into());
        }

        tauri::process::restart(&env);
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn setup_deeplink(a: &App, state: &MutState) {
    {
        // consume deeplink from current in case already exists
        // which will provoke that on_open_url not be triggered
        match a.deep_link().get_current() {
            Ok(Some(urls)) => {
                if let Some(url) = urls.first() {
                    accept_deeplink(state, url.to_string());
                }
            }
            Ok(None) => {}
            Err(e) => error!("Failed to read launch deeplink via get_current: {}", e),
        }

        let state = state.clone();
        a.deep_link().on_open_url(move |event| {
            let urls = event.urls();
            match urls.first() {
                Some(url) => accept_deeplink(&state, url.to_string()),
                None => {
                    error!("No values are provided in deep link");
                }
            }
        });
    }
}

#[cfg(not(target_os = "macos"))]
const fn setup_deeplink(_a: &App, _state: &MutState) {
    // Windows deeplinks arrive via argv only (captured in `setup`).
}

#[cfg(target_os = "macos")]
fn accept_deeplink(state: &MutState, url: String) {
    if !dcl_launcher_shared::environment::is_deeplink(&url) {
        error!("Ignoring a non-decentraland deeplink: {}", url);
        return;
    }
    state.set_pending_deeplink(url.clone());
    tauri::async_runtime::spawn(service_lifecycle::inject_deeplink(url));
}

fn setup(a: &App) {
    logging::init();
    info!(
        "Launcher UI setup start. Version: {}",
        dcl_launcher_shared::app_version()
    );

    let state: MutState = Arc::new(UiState {
        pending_deeplink: Mutex::new(deeplink_from_env()),
        ui_close_notified: AtomicBool::new(false),
    });

    setup_deeplink(a, &state);
    a.manage(state);
}

fn notify_ui_closed_on_exit(app: &AppHandle) {
    let Some(state) = app.try_state::<MutState>() else {
        return;
    };
    if state.ui_close_notified.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::block_on(async {
        let notify = service_lifecycle::notify_ui_closed();
        if tokio::time::timeout(Duration::from_millis(500), notify)
            .await
            .is_err()
        {
            info!("notifyUIClosed timed out; the service may not be running");
        }
    });
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
        .setup(|a| {
            setup(a);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![launch, retry])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                notify_ui_closed_on_exit(app_handle);
            }
        });
}
