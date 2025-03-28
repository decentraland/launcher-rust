use dcl_launcher_core::{ app::AppState, channel::EventChannel, types, utils };
use tauri::{ipc::Channel, App, Manager, State};
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
async fn launch(state: State<'_, MutState>, channel: Channel<types::Status>) -> Result<(), ()> {
    let status_channel = StatusChannel(channel);
    let guard = state.lock().await;

    let flow_state = guard.state.clone();

    //TODO expose error
    guard.flow.launch(&status_channel, flow_state).await.map_err(|_| {return ();} )?;

    //TODO remove message
    let _result = status_channel.send(types::Status::Error {
        message: "not implemented".into(),
        can_retry: true,
    });
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
