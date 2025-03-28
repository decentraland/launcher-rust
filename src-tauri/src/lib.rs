use flow::LaunchFlowState;
use tauri::{ipc::Channel, App, Manager, State};
use std::sync::Arc;
use fern;
use tauri::async_runtime::Mutex;
mod s3;
mod types;
mod utils;
mod flow;
mod installs;
mod analytics;
mod environment;
mod protocols;

type MutState = Arc<Mutex<AppState>>;

struct AppState {
    flow: flow::LaunchFlow,
    state: Arc<Mutex<flow::LaunchFlowState>>,
}

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
    fern::Dispatch::new()
        // Perform allocation-free log formatting
        .format(|out, message, record| {
            out.finish(format_args!(
                    "[{} {} {}] {}",
                    humantime::format_rfc3339(std::time::SystemTime::now()),
                    record.level(),
                    record.target(),
                    message
            ))
        })
        .level(log::LevelFilter::Trace)
        .chain(std::io::stdout())
        .chain(fern::log_file("output.log")?)
        .apply()?;

    //TODO pass real client
    let analytics = Arc::new(Mutex::new(analytics::Analytics::new(None)));

    let installs_hub = Arc::new(Mutex::new(installs::InstallsHub::new(analytics)));

    let flow = flow::LaunchFlow::new(installs_hub);
    let flow_state = LaunchFlowState::default();
    let app_state = AppState {
        flow,
        state: Arc::new(Mutex::new(flow_state)),
    };
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
