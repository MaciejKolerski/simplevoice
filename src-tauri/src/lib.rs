mod audio;
use audio::AudioController;
use tauri::Manager;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn list_audio_devices(controller: tauri::State<'_, AudioController>) -> Result<Vec<String>, String> {
    controller.list_devices()
}

#[tauri::command]
fn set_selected_device(device: Option<String>, controller: tauri::State<'_, AudioController>) {
    controller.set_selected_device(device);
}

#[tauri::command]
fn start_recording(controller: tauri::State<'_, AudioController>) -> Result<(), String> {
    controller.start_recording()
}

#[tauri::command]
fn stop_recording(
    controller: tauri::State<'_, AudioController>,
    app_handle: tauri::AppHandle,
) -> Result<Option<String>, String> {
    controller.stop_recording(&app_handle)
}

#[tauri::command]
fn get_recording_status(controller: tauri::State<'_, AudioController>) -> bool {
    controller.is_recording()
}

#[tauri::command]
fn clear_app_files(
    controller: tauri::State<'_, AudioController>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    // 1. Clear the active buffer in state
    {
        let mut s = controller.state.lock().unwrap();
        s.buffer.clear();
    }

    // 2. Clear WAV files in local data directory
    let app_local_data = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;

    if app_local_data.exists() {
        let mut deleted_count = 0;
        let entries = std::fs::read_dir(&app_local_data).map_err(|e| e.to_string())?;
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == "wav" {
                            if std::fs::remove_file(&path).is_ok() {
                                deleted_count += 1;
                            }
                        }
                    }
                }
            }
        }
        Ok(format!("Deleted {} recording (.wav) files from disk.", deleted_count))
    } else {
        Ok("No recordings found to clear.".to_string())
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AudioController::new())
        .invoke_handler(tauri::generate_handler![
            greet,
            list_audio_devices,
            set_selected_device,
            start_recording,
            stop_recording,
            get_recording_status,
            clear_app_files
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
