mod audio;
use audio::AudioController;
use tauri::{Manager, Emitter};
use tauri::tray::TrayIconBuilder;
use tauri::menu::{MenuBuilder, MenuItemBuilder, CheckMenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};

fn draw_status_dot(base_image: &tauri::image::Image<'_>, color: [u8; 4]) -> tauri::image::Image<'static> {
    let width = base_image.width();
    let height = base_image.height();
    let mut rgba = base_image.rgba().to_vec();
    
    // Draw a status dot twice as large (radius factor 0.24 instead of 0.12)
    let radius = (width as f32 * 0.24).max(4.0) as i32;
    let cx = (width as i32) - radius - 2;
    let cy = (height as i32) - radius - 2;
    
    for y in 0..(height as i32) {
        for x in 0..(width as i32) {
            let dx = x - cx;
            let dy = y - cy;
            if dx * dx + dy * dy <= radius * radius {
                let idx = ((y * (width as i32) + x) * 4) as usize;
                if idx + 3 < rgba.len() {
                    rgba[idx] = color[0];
                    rgba[idx + 1] = color[1];
                    rgba[idx + 2] = color[2];
                    rgba[idx + 3] = color[3];
                }
            }
        }
    }
    
    tauri::image::Image::new_owned(rgba, width, height)
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn list_audio_devices(controller: tauri::State<'_, AudioController>) -> Result<Vec<String>, String> {
    controller.list_devices()
}

#[tauri::command]
fn set_selected_device(
    device: Option<String>,
    controller: tauri::State<'_, AudioController>,
    app_handle: tauri::AppHandle,
) {
    controller.set_selected_device(device);
    let _ = rebuild_tray_menu(&app_handle);
}

#[tauri::command]
fn start_recording(
    controller: tauri::State<'_, AudioController>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    controller.start_recording()?;
    let _ = rebuild_tray_menu(&app_handle);
    Ok(())
}

#[tauri::command]
fn stop_recording(
    controller: tauri::State<'_, AudioController>,
    app_handle: tauri::AppHandle,
) -> Result<Option<String>, String> {
    let res = controller.stop_recording(&app_handle);
    let _ = rebuild_tray_menu(&app_handle);
    res
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
    {
        let mut s = controller.state.lock().unwrap();
        s.buffer.clear();
    }

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
        let _ = rebuild_tray_menu(&app_handle);
        Ok(format!("Deleted {} recording (.wav) files from disk.", deleted_count))
    } else {
        let _ = rebuild_tray_menu(&app_handle);
        Ok("No recordings found to clear.".to_string())
    }
}

pub fn rebuild_tray_menu(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let app_handle_clone = app_handle.clone();
    app_handle.run_on_main_thread(move || {
        if let Err(e) = rebuild_tray_menu_inner(&app_handle_clone) {
            eprintln!("Error rebuilding tray menu on main thread: {}", e);
        }
    }).map_err(|e| e.to_string())
}

fn rebuild_tray_menu_inner(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let controller = app_handle.state::<AudioController>();
    let is_recording = controller.is_recording();
    let is_saving = controller.is_saving();
    
    let base_icon = app_handle.default_window_icon().cloned();
    let tray_icon_img = if let Some(ref img) = base_icon {
        if is_recording {
            Some(draw_status_dot(img, [255, 59, 48, 255])) // iOS system red
        } else if is_saving {
            Some(draw_status_dot(img, [0, 122, 255, 255])) // iOS system blue
        } else {
            Some(img.clone())
        }
    } else {
        None
    };
    
    let selected_device = {
        let s = controller.state.lock().unwrap();
        s.selected_device.clone()
    };
    
    let toggle_label = if is_recording {
        "Stop Recording"
    } else {
        "Start Recording"
    };
    let toggle_recording_item = MenuItemBuilder::new(toggle_label)
        .id("toggle_recording")
        .build(app_handle)
        .map_err(|e| e.to_string())?;
        
    let nav_usage_item = MenuItemBuilder::new("Usage")
        .id("nav_usage")
        .build(app_handle)
        .map_err(|e| e.to_string())?;
        
    let nav_models_item = MenuItemBuilder::new("Models")
        .id("nav_models")
        .build(app_handle)
        .map_err(|e| e.to_string())?;
        
    let nav_history_item = MenuItemBuilder::new("History")
        .id("nav_transcriptions")
        .build(app_handle)
        .map_err(|e| e.to_string())?;
        
    let nav_settings_item = MenuItemBuilder::new("Settings")
        .id("nav_settings")
        .build(app_handle)
        .map_err(|e| e.to_string())?;

    let devices = controller.list_devices().unwrap_or_default();
    let mic_menu = {
        let mut builder = SubmenuBuilder::new(app_handle, "Select Microphone");
        
        let is_default_checked = selected_device.is_none();
        let default_mic_item = CheckMenuItemBuilder::new("Default System Microphone")
            .id("mic_default")
            .checked(is_default_checked)
            .build(app_handle)
            .map_err(|e| e.to_string())?;
        builder = builder.item(&default_mic_item);
        
        for device_name in devices {
            let is_checked = selected_device.as_ref().map_or(false, |d| d == &device_name);
            let id = format!("mic_device:{}", device_name);
            let device_item = CheckMenuItemBuilder::new(&device_name)
                .id(id)
                .checked(is_checked)
                .build(app_handle)
                .map_err(|e| e.to_string())?;
            builder = builder.item(&device_item);
        }
        builder.build().map_err(|e| e.to_string())?
    };
    
    let quit_item = MenuItemBuilder::new("Quit")
        .id("quit")
        .build(app_handle)
        .map_err(|e| e.to_string())?;
        
    let separator = PredefinedMenuItem::separator(app_handle).map_err(|e| e.to_string())?;
    let separator2 = PredefinedMenuItem::separator(app_handle).map_err(|e| e.to_string())?;
    let separator3 = PredefinedMenuItem::separator(app_handle).map_err(|e| e.to_string())?;
    
    let menu = MenuBuilder::new(app_handle)
        .item(&toggle_recording_item)
        .item(&separator)
        .item(&nav_usage_item)
        .item(&nav_models_item)
        .item(&nav_history_item)
        .item(&nav_settings_item)
        .item(&separator2)
        .item(&mic_menu)
        .item(&separator3)
        .item(&quit_item)
        .build()
        .map_err(|e| e.to_string())?;
        
    if let Some(tray) = app_handle.tray_by_id("main-tray") {
        let _ = tray.set_title(None::<&str>);
        if let Some(img) = tray_icon_img {
            let _ = tray.set_icon(Some(img));
        }
        tray.set_menu(Some(menu)).map_err(|e| e.to_string())?;
    } else {
        let mut builder = TrayIconBuilder::with_id("main-tray")
            .tooltip("Simple Voice")
            .menu(&menu)
            .on_menu_event(|app, event| {
                let id = event.id().0.as_str();
                handle_tray_menu_event(app, id);
            });
        if let Some(img) = tray_icon_img {
            builder = builder.icon(img);
        }
        let _tray = builder.build(app_handle).map_err(|e| e.to_string())?;
    }
    
    Ok(())
}

fn handle_tray_menu_event(app: &tauri::AppHandle, id: &str) {
    if id == "toggle_recording" {
        let controller = app.state::<AudioController>();
        if controller.is_recording() {
            if let Ok(wav_path) = controller.stop_recording(app) {
                let payload = wav_path.unwrap_or_else(|| "Recording stopped".to_string());
                let _ = app.emit("recording-stopped", payload);
            }
        } else {
            if let Ok(_) = controller.start_recording() {
                let _ = app.emit("recording-started", ());
            }
        }
        let _ = rebuild_tray_menu(app);
    } else if id.starts_with("nav_") {
        let view_name = id.trim_start_matches("nav_");
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
        let _ = app.emit("navigate", view_name);
    } else if id == "mic_default" {
        let controller = app.state::<AudioController>();
        controller.set_selected_device(None);
        let _ = app.emit("device-changed", None::<String>);
        let _ = rebuild_tray_menu(app);
    } else if id.starts_with("mic_device:") {
        let device_name = id.trim_start_matches("mic_device:").to_string();
        let controller = app.state::<AudioController>();
        controller.set_selected_device(Some(device_name.clone()));
        let _ = app.emit("device-changed", Some(device_name));
        let _ = rebuild_tray_menu(app);
    } else if id == "quit" {
        app.exit(0);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AudioController::new())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            
            let app_handle = app.handle();
            let _ = rebuild_tray_menu(app_handle);
            Ok(())
        })
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
