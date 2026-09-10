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

use dcl_launcher_core::analytics::event::Event;
use dcl_launcher_core::app::{FlowContext, LaunchContext, ReportContext};
use dcl_launcher_core::environment::{strip_crash_report_args, AppEnvironment, Args};
use dcl_launcher_core::errors::FlowError;
use dcl_launcher_core::log::{error, info};
use dcl_launcher_core::protocols::Protocol;
use dcl_launcher_core::report_flow::{CrashReportField, ReportFlow, ReportFlowState, Screen};
use dcl_launcher_core::types::LauncherUpdate;
use dcl_launcher_core::utils;
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
            error!("Error during the message sending: {}", e);
        }
    }

    fn notify_error(&self, flow_error: &FlowError) {
        self.send_silent(flow_error.into());
    }
}

impl<T: EventChannel + ?Sized> EventChannelExt for T {}

const CONTEXT_MISMATCH: &str =
    "Context mismatch, the command must not be invoked and will be ignored";

fn launch_context(state: &AppState) -> Result<&LaunchContext, String> {
    match &state.context {
        FlowContext::Launch(ctx) => Ok(ctx),
        FlowContext::Report(_) => {
            error!("{CONTEXT_MISMATCH}");
            Err(CONTEXT_MISMATCH.to_owned())
        }
    }
}

fn report_context(state: &AppState) -> Result<&ReportContext, String> {
    match &state.context {
        FlowContext::Report(ctx) => Ok(ctx),
        FlowContext::Launch(_) => {
            error!("{CONTEXT_MISMATCH}");
            Err(CONTEXT_MISMATCH.to_owned())
        }
    }
}

type ReportHandles = (Arc<ReportFlow>, Arc<Mutex<ReportFlowState>>);

/// Clones the report flow handles so the `AppState` lock is released before the flow is awaited.
/// Mutations still serialize: every flow method holds the `ReportFlowState` lock end to end.
async fn report_handles(state: &State<'_, MutState>) -> Result<ReportHandles, String> {
    let guard = state.lock().await;
    let ctx = report_context(&guard)?;
    let handles = (ctx.flow.clone(), ctx.state.clone());
    drop(guard);
    Ok(handles)
}

#[tauri::command]
async fn retry(
    app: AppHandle,
    state: State<'_, MutState>,
    channel: Channel<types::Status>,
) -> Result<(), String> {
    info!("tauri command: retry");
    let event = Event::RETRY_FLOW_BUTTON_CLICK {
        version: utils::app_version().to_owned(),
    };
    state
        .lock()
        .await
        .analytics
        .lock()
        .await
        .track_and_flush_silent(event)
        .await;
    launch_internal(app, state, channel).await
}

/// Single entry point invoked by the UI on mount. The UI never asks which flow it is in: the
/// context selected at startup decides, and the UI renders whatever `Status` arrives.
#[tauri::command]
async fn launch(
    app: AppHandle,
    state: State<'_, MutState>,
    channel: Channel<types::Status>,
) -> Result<(), String> {
    info!("tauri command: launch");
    let is_report = matches!(state.lock().await.context, FlowContext::Report(_));
    if is_report {
        return crash_report_open(state, channel).await;
    }
    launch_internal(app, state, channel).await
}

async fn launch_internal(
    app: AppHandle,
    state: State<'_, MutState>,
    channel: Channel<types::Status>,
) -> Result<(), String> {
    let status_channel = StatusChannel(channel);
    let guard = state.lock().await;
    let flow_context = launch_context(&guard)?;
    let flow_state = flow_context.state.clone();

    if let Err(e) = update_if_needed_and_restart(&app, &guard, &status_channel).await {
        error!("Cannot update the launcher: {}", e);
    }

    flow_context
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

async fn crash_report_open(
    state: State<'_, MutState>,
    channel: Channel<types::Status>,
) -> Result<(), String> {
    info!("crash report: open");
    let status_channel = StatusChannel(channel);
    let (flow, flow_state) = report_handles(&state).await?;
    flow.open(&status_channel, flow_state)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn crash_report_set_field(
    state: State<'_, MutState>,
    channel: Channel<types::Status>,
    field: CrashReportField,
) -> Result<(), String> {
    let status_channel = StatusChannel(channel);
    let (flow, flow_state) = report_handles(&state).await?;
    flow.set_field(&status_channel, flow_state, field)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn crash_report_navigate(
    state: State<'_, MutState>,
    channel: Channel<types::Status>,
    screen: Screen,
) -> Result<(), String> {
    info!("tauri command: crash_report_navigate {screen:?}");
    let status_channel = StatusChannel(channel);
    let (flow, flow_state) = report_handles(&state).await?;
    flow.navigate(&status_channel, flow_state, screen)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn crash_report_submit(
    state: State<'_, MutState>,
    channel: Channel<types::Status>,
) -> Result<(), String> {
    info!("tauri command: crash_report_submit");
    let status_channel = StatusChannel(channel);
    let (flow, flow_state) = report_handles(&state).await?;
    flow.submit(&status_channel, flow_state)
        .await
        .map_err(|e| e.user_message)
}

/// The dialog has no close button of its own: the system one closes the window. Before the
/// process goes away the report flow still gets its dismissal (analytics, "don't show again",
/// attachment cleanup). A launch-flow window closing needs nothing here.
fn on_window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if !matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
        return;
    }
    let state = window.state::<MutState>();
    // try_lock: a launch flow may hold the lock for minutes; never stall the close for it.
    let Ok(guard) = state.try_lock() else {
        return;
    };
    let FlowContext::Report(ctx) = &guard.context else {
        return;
    };
    info!("window close requested during the crash report flow");
    let (flow, flow_state) = (ctx.flow.clone(), ctx.state.clone());
    drop(guard);
    tauri::async_runtime::block_on(flow.dismiss(flow_state, false));
}

/// RELAUNCH: dismiss and restart this executable without the crash argument, so the regular
/// launch flow runs (updater check, install, launch, fresh watchdog).
#[tauri::command]
async fn relaunch(app: AppHandle, state: State<'_, MutState>) -> Result<(), String> {
    info!("tauri command: relaunch");
    let guard = state.lock().await;
    let ctx = report_context(&guard)?;
    ctx.flow.dismiss(ctx.state.clone(), true).await;

    guard.cleanup().await;
    drop(guard);
    app.cleanup_before_exit();

    let mut env = app.env();
    env.args_os = strip_crash_report_args(env.args_os);
    tauri::process::restart(&env);
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
) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    match dcl_launcher_core::environment::macos::is_running_from_dmg() {
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

        channel.send_silent(LauncherUpdate::InstallingUpdate.into());
        update.install(content)?;
        info!("update installed");

        channel.send_silent(LauncherUpdate::RestartingApp.into());

        let mut env = app.env();

        app_state.cleanup().await;
        app.cleanup_before_exit();

        // Preserve deeplink
        if let Some(deeplink) = Protocol::value() {
            env.args_os.push(deeplink.original().into());
        }

        tauri::process::restart(&env);
    }

    Ok(())
}

#[cfg_attr(windows, allow(unused_variables))]
fn setup_deeplink(a: &App, protocol: &Protocol) {
    // Support reading from cmd args on both macOS and Windows
    let args: Vec<String> = AppEnvironment::raw_cmd_args().collect();
    protocol.try_assign_value_from_vec(&args);

    #[cfg(target_os = "macos")]
    {
        // consume deeplink from current in case already exists
        // which will provoke that on_open_url not be triggered
        match a.deep_link().get_current() {
            Ok(Some(urls)) => {
                if let Some(url) = urls.first() {
                    protocol.try_assign_value(url.to_string());
                }
            }
            Ok(None) => {}
            Err(e) => error!("Failed to read launch deeplink via get_current: {}", e),
        }

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
}

fn setup(a: &mut App) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let app_state = tauri::async_runtime::block_on(AppState::setup())
        .inspect_err(|e| error!("Error during setup: {:#}", e))?;

    setup_deeplink(a, &app_state.protocol);
    Protocol::try_seed_from_startup_location();

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
        .invoke_handler(tauri::generate_handler![
            launch,
            retry,
            crash_report_set_field,
            crash_report_navigate,
            crash_report_submit,
            relaunch
        ])
        .on_window_event(on_window_event)
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
