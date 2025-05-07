use dcl_launcher_core::log::error;
use dcl_launcher_core::types::FlowError;
use dcl_launcher_core::{app::AppState, channel::EventChannel, types};
use std::sync::Arc;
use tauri::async_runtime::Mutex;
use tauri::{ipc::Channel, App, AppHandle, Manager, State};
use tauri_plugin_deep_link::DeepLinkExt;

type MutState = Arc<Mutex<AppState>>;

pub struct StatusChannel(Channel<types::Status>);

impl EventChannel for StatusChannel {
    fn send(&self, status: types::Status) -> anyhow::Result<()> {
        self.0.send(status)?;
        Ok(())
    }
}

fn notify_error<T: EventChannel>(flow_error: &FlowError, channel: &T) {
    let send_result = channel.send(flow_error.into());
    match send_result {
        Ok(_) => {
            // ignore
        }
        Err(e) => {
            error!("Error during the message sending: {}", e.to_string());
        }
    }
}

#[tauri::command]
async fn launch(
    app: AppHandle,
    state: State<'_, MutState>,
    channel: Channel<types::Status>,
) -> Result<(), String> {
    let status_channel = StatusChannel(channel);
    let guard = state.lock().await;

    let flow_state = guard.state.clone();

    guard
        .flow
        .launch(&status_channel, flow_state)
        .await
        .map_err(|e| {
            notify_error(&e, &status_channel);
            e.user_message
        })?;

    guard.cleanup().await;
    app.cleanup_before_exit();
    app.exit(0);

    Ok(())
}

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
    let app_state = tauri::async_runtime::block_on(AppState::setup())?;

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
