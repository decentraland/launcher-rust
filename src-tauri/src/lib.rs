use dcl_launcher_core::errors::FlowError;
use dcl_launcher_core::log::{error, info};
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
    app.cleanup_before_exit();
    app.exit(0);

    Ok(())
}

fn current_updater(app: &AppHandle) -> tauri_plugin_updater::Result<tauri_plugin_updater::Updater> {
    const KEY_UPDATER_URL: &str = "--use-updater-url";
    const KEY_ALWAYS_TRIGGER_UPDATER: &str = "--always-trigger-updater";
    const KEY_NEVER_TRIGGER_UPDATER: &str = "--never-trigger-updater";

    let args: Vec<String> = env::args().collect();

    // comparison to support rollbacks
    let compare_args = args.clone();
    let builder = app
        .updater_builder()
        .version_comparator(move |current_version, remote| {
            if compare_args.iter().any(|a| a == KEY_NEVER_TRIGGER_UPDATER) {
                info!("Never trigger updater by flag {}", KEY_UPDATER_URL);
                return false;
            }

            if compare_args.iter().any(|a| a == KEY_ALWAYS_TRIGGER_UPDATER) {
                info!("Always trigger updater by flag {}", KEY_UPDATER_URL);
                return true;
            }

            current_version != remote.version
        });

    if let Some(pos) = args.iter().position(|a| a == KEY_UPDATER_URL) {
        let url = args.get(pos + 1);
        match url {
            Some(url) => {
                info!(
                    "Use custom updater by flag {} with its value {}",
                    KEY_UPDATER_URL, url
                );
                let parsed_url: Url = Url::parse(url)?;
                return builder.endpoints(vec![parsed_url])?.build();
            }
            None => {
                error!(
                    "Flag {} is provided but its value is missed",
                    KEY_UPDATER_URL
                )
            }
        }
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
        let mut downloaded = 0;

        let content = update
            .download(
                |chunk_length, content_length| {
                    downloaded += chunk_length;
                    info!("downloaded {downloaded} from {content_length:?}");
                    match content_length {
                        Some(l) => {
                            let progress: u8 = ((downloaded as f64 / l as f64) * 100.0) as u8;
                            channel.send_silent(
                                LauncherUpdate::Downloading {
                                    progress: Some(progress),
                                }
                                .into(),
                            );
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
fn setup_deeplink(a: &mut App) {
    #[cfg(target_os = "macos")]
    {
        a.deep_link().on_open_url(|event| {
            let urls = event.urls();
            match urls.first() {
                Some(url) => {
                    dcl_launcher_core::protocols::Protocol::try_assign_value(url.to_string());
                }
                None => {
                    error!("No values are provided in deep link")
                }
            }
        });
    }

    #[cfg(target_os = "windows")]
    {
        let args: Vec<String> = std::env::args().collect();
        dcl_launcher_core::protocols::Protocol::try_assign_value_from_vec(&args);
    }
}

fn setup(a: &mut App) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let app_state = tauri::async_runtime::block_on(AppState::setup())
        .inspect_err(|e| error!("Error during setup: {:#}", e))?;

    setup_deeplink(a);

    let mut_state: MutState = Arc::new(Mutex::new(app_state));
    a.manage(mut_state);
    Ok(())
}

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
