use app::AppState;
use tauri::{ipc::Channel, App, Manager, State};
use std::sync::Arc;
use tauri::async_runtime::Mutex;

mod s3;
mod types;
mod utils;
mod flow;
mod installs;
mod analytics;
mod environment;
mod protocols;
mod app;

type MutState = Arc<Mutex<AppState>>;

#[tauri::command]
async fn launch(state: State<'_, MutState>, channel: Channel<types::Status>) -> Result<(), ()> {
    let guard = state.lock().await;

    let flow_state = guard.state.clone();
    //TODO expose error
    guard.flow.launch(&channel, flow_state).await.map_err(|_| {return ();} )?;

    //TODO remove message
    let _result = channel.send(types::Status::Error {
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
