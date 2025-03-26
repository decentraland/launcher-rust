use tauri::ipc::Channel;
mod types;

#[tauri::command]
async fn launch( channel: Channel<types::Status>) -> Result<(), ()> {
    let _result = channel.send(types::Status::Error { message: "not implemented".into(), can_retry: true });
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![launch])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
