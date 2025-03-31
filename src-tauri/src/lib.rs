use dcl_launcher_core::{ app::AppState, channel::EventChannel, types, utils };
use tauri::{ipc::Channel, App, AppHandle, Manager, State};
use std::sync::Arc;
use tauri::async_runtime::Mutex;

type MutState = Arc<Mutex<AppState>>;

pub struct StatusChannel(Channel<types::Status>); 

impl EventChannel for StatusChannel {
    fn send(&self, status: types::Status) -> anyhow::Result<()> {
        self.0.send(status)?;
        Ok(())
    }
}

#[tauri::command]
async fn launch(app: AppHandle, state: State<'_, MutState>, channel: Channel<types::Status>) -> Result<(), String> {
    let status_channel = StatusChannel(channel);
    let guard = state.lock().await;

    let flow_state = guard.state.clone();

    guard.flow.launch(&status_channel, flow_state).await.map_err(|e| 
        {
            let message = e.to_string();
            let send_result = status_channel.send(types::Status::Error {
                message: message.clone(),
                can_retry: true,
            });
            match send_result {
                Ok(_) => {
                    // ignore
                },
                Err(e) => {
                    eprintln!("Error during the message sending: {}", e.to_string());
                },
            }

            message
        }
    )?;

    app.cleanup_before_exit();
    app.exit(0);

    Ok(())
}

fn setup(a: &mut App) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let app_state = AppState::setup()?;
    let mut_state: MutState = Arc::new(Mutex::new(app_state));
    a.manage(mut_state);
    Ok(())
}


#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args = utils::parsed_argv();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(setup)
        .invoke_handler(tauri::generate_handler![launch])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
