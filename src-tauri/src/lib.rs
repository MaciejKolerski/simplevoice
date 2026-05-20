mod audio;
use audio::AudioController;
use std::sync::Mutex;
mod stt;
use stt::SttController;
mod refiner;
use tauri::menu::{
    CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};
use tauri_plugin_sql::{Migration, MigrationKind};

fn draw_status_dot(
    base_image: &tauri::image::Image<'_>,
    color: [u8; 4],
) -> tauri::image::Image<'static> {
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
fn list_audio_devices(
    controller: tauri::State<'_, AudioController>,
) -> Result<Vec<String>, String> {
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

#[derive(Debug, Clone)]
pub struct ActiveConfig {
    pub engine: String,
    pub provider: String,
}

pub struct AppConfig {
    pub active: Mutex<ActiveConfig>,
}

#[tauri::command]
fn update_active_config(engine: String, provider: String, config: tauri::State<'_, AppConfig>) {
    let mut c = config.active.lock().unwrap();
    c.engine = engine;
    c.provider = provider;
}

fn is_recording_allowed(config: &AppConfig, stt: &SttController) -> Result<(), String> {
    let c = config.active.lock().unwrap();
    if c.engine == "local" {
        let stt_state = stt.state.lock().unwrap();
        if stt_state.engine.is_none() {
            return Err(
                "No local model loaded. Please select and load a local model first.".to_string(),
            );
        }
    } else if c.engine == "openai-cloud" {
        let key_name = format!("api_key_{}", c.provider);
        let entry = keyring::Entry::new("simplevoice", &key_name);
        match entry {
            Ok(ent) => {
                if let Ok(password) = ent.get_password() {
                    if password.trim().is_empty() {
                        return Err(format!(
                            "API Key for {} is missing. Please set it in BYOK Config.",
                            c.provider.to_uppercase()
                        ));
                    }
                } else {
                    return Err(format!(
                        "API Key for {} is missing. Please set it in BYOK Config.",
                        c.provider.to_uppercase()
                    ));
                }
            }
            Err(_) => {
                return Err(format!(
                    "API Key for {} is missing. Please set it in BYOK Config.",
                    c.provider.to_uppercase()
                ));
            }
        }
    }
    Ok(())
}

fn is_sound_feedback_enabled(app_handle: &tauri::AppHandle) -> bool {
    let app_local_data = match app_handle.path().app_local_data_dir() {
        Ok(dir) => dir,
        Err(_) => return true,
    };
    let config_path = app_local_data.join("config.json");
    if !config_path.exists() {
        return true;
    }
    let content = match std::fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(_) => return true,
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return true,
    };
    if let Some(val) = json.get("sound_feedback_enabled") {
        if let Some(s) = val.as_str() {
            return s != "false";
        }
    }
    true
}

/// Resolves the sound file for the given event type.
/// Looks for start.wav / stop.wav / done.wav inside the bundled resources:
///   <app>.app/Contents/Resources/sounds/  (macOS bundle)
///   or next to the binary in dev mode.
/// Falls back to a built-in macOS system sound if the file is not present.
fn resolve_sound_file(
    app_handle: &tauri::AppHandle,
    sound_type: &str,
) -> Option<std::path::PathBuf> {
    let fname = match sound_type {
        "start" => "start.wav",
        "stop" => "stop.wav",
        "done" => "done.wav",
        _ => return None,
    };

    // Check bundled resources (works in both dev and release builds)
    if let Ok(res_dir) = app_handle.path().resource_dir() {
        let bundled = res_dir.join("sounds").join(fname);
        if bundled.exists() {
            return Some(bundled);
        }
    }

    // Fallback: built-in macOS system sounds
    #[cfg(target_os = "macos")]
    {
        let fallback = match sound_type {
            "start" => "/System/Library/Sounds/Tink.aiff",
            "stop" => "/System/Library/Sounds/Pop.aiff",
            "done" => "/System/Library/Sounds/Glass.aiff",
            _ => return None,
        };
        return Some(std::path::PathBuf::from(fallback));
    }

    #[allow(unreachable_code)]
    None
}

fn play_backend_sound(app_handle: &tauri::AppHandle, sound_type: &str) {
    if !is_sound_feedback_enabled(app_handle) {
        return;
    }
    if let Some(path) = resolve_sound_file(app_handle, sound_type) {
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("afplay").arg(&path).spawn();
        }
    }
}

#[tauri::command]
fn start_recording(
    controller: tauri::State<'_, AudioController>,
    stt: tauri::State<'_, SttController>,
    config: tauri::State<'_, AppConfig>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    is_recording_allowed(&config, &stt)?;
    controller.start_recording(app_handle.clone())?;
    play_backend_sound(&app_handle, "start");
    let _ = rebuild_tray_menu(&app_handle);
    Ok(())
}

#[tauri::command]
fn stop_recording(
    controller: tauri::State<'_, AudioController>,
    app_handle: tauri::AppHandle,
) -> Result<Option<String>, String> {
    let res = controller.stop_recording(&app_handle);
    if res.is_ok() {
        play_backend_sound(&app_handle, "stop");
    }
    let _ = rebuild_tray_menu(&app_handle);
    res
}

#[tauri::command]
fn set_transcribing(
    active: bool,
    controller: tauri::State<'_, AudioController>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    controller.set_transcribing(active);
    let _ = rebuild_tray_menu(&app_handle);
    Ok(())
}

#[tauri::command]
fn open_folder(path: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn rebuild_tray_menu(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let app_handle_clone = app_handle.clone();
    app_handle
        .run_on_main_thread(move || {
            if let Err(e) = rebuild_tray_menu_inner(&app_handle_clone) {
                eprintln!("Error rebuilding tray menu on main thread: {}", e);
            }
        })
        .map_err(|e| e.to_string())
}

fn rebuild_tray_menu_inner(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let controller = app_handle.state::<AudioController>();
    let is_recording = controller.is_recording();
    let is_saving = controller.is_saving();
    let is_transcribing = controller.is_transcribing();

    let base_icon = app_handle.default_window_icon().cloned();
    let tray_icon_img = if let Some(ref img) = base_icon {
        if is_recording {
            Some(draw_status_dot(img, [255, 59, 48, 255])) // Use iOS system red for recording state
        } else if is_saving || is_transcribing {
            Some(draw_status_dot(img, [0, 122, 255, 255])) // Use iOS system blue for processing state
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
            let is_checked = selected_device.as_ref() == Some(&device_name);
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
            let stt = app.state::<SttController>();
            let config = app.state::<AppConfig>();
            match is_recording_allowed(&config, &stt) {
                Ok(_) => {
                    if controller.start_recording(app.clone()).is_ok() {
                        let _ = app.emit("recording-started", ());
                    }
                }
                Err(reason) => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                    let _ = app.emit("recording-failed-to-start", reason);
                }
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

#[tauri::command]
fn register_shortcut(shortcut_str: String, app_handle: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

    let global_shortcut = app_handle.global_shortcut();

    let _ = global_shortcut.unregister_all();

    if shortcut_str.trim().is_empty() {
        return Ok(());
    }

    let shortcut: Shortcut = shortcut_str
        .parse()
        .map_err(|e| format!("Failed to parse shortcut '{}': {}", shortcut_str, e))?;

    global_shortcut
        .register(shortcut)
        .map_err(|e| format!("Failed to register shortcut '{}': {}", shortcut_str, e))?;

    println!(
        "Successfully registered global recording shortcut: {}",
        shortcut_str
    );
    Ok(())
}

#[tauri::command]
fn set_vad_enabled(
    enabled: bool,
    controller: tauri::State<'_, AudioController>,
) -> Result<(), String> {
    let mut s = controller.state.lock().unwrap();
    s.vad_enabled = enabled;
    Ok(())
}

#[derive(serde::Serialize, Clone)]
struct LocalModel {
    name: String,
    filename: String,
    path: String,
    size_bytes: u64,
    size_formatted: String,
    quality: u8,
    speed: u8,
    is_active: bool,
}

#[tauri::command]
fn scan_models(
    app_handle: tauri::AppHandle,
    stt_controller: tauri::State<'_, SttController>,
) -> Result<Vec<LocalModel>, String> {
    let app_local_data = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;

    let models_dir = app_local_data.join("models");
    std::fs::create_dir_all(&models_dir).map_err(|e| e.to_string())?;

    let active_path = {
        let s = stt_controller.state.lock().unwrap();
        s.active_model_path.clone()
    };

    let mut models = Vec::new();
    let entries = std::fs::read_dir(&models_dir).map_err(|e| e.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .is_some_and(|ext| ext == "gguf" || ext == "bin")
        {
            let filename = path.file_name().unwrap().to_string_lossy().to_string();
            let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);

            let size_formatted = if size_bytes >= 1_073_741_824 {
                format!("{:.2} GB", size_bytes as f64 / 1_073_741_824.0)
            } else {
                format!("{:.0} MB", size_bytes as f64 / 1_048_576.0)
            };

            let filename_lower = filename.to_lowercase();
            let (quality, speed, name) =
                if filename_lower.contains("large") || size_bytes > 2_000_000_000 {
                    (95, 40, "Whisper Large")
                } else if filename_lower.contains("medium") || size_bytes > 1_000_000_000 {
                    (85, 60, "Whisper Medium")
                } else if filename_lower.contains("small") || size_bytes > 400_000_000 {
                    (75, 80, "Whisper Small")
                } else if filename_lower.contains("base") || size_bytes > 140_000_000 {
                    (65, 90, "Whisper Base")
                } else {
                    (50, 98, "Whisper Tiny")
                };

            let display_name = format!("{} ({})", name, filename);
            let is_active = Some(path.to_string_lossy().to_string()) == active_path;

            models.push(LocalModel {
                name: display_name,
                filename,
                path: path.to_string_lossy().to_string(),
                size_bytes,
                size_formatted,
                quality,
                speed,
                is_active,
            });
        } else if path.is_dir() {
            let has_tokens = path.join("tokens.txt").exists();
            let has_onnx = if let Ok(sub_entries) = std::fs::read_dir(&path) {
                sub_entries
                    .filter_map(|e| e.ok())
                    .any(|e| e.path().extension().is_some_and(|ext| ext == "onnx"))
            } else {
                false
            };

            if has_tokens && has_onnx {
                let mut size_bytes = 0u64;
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub_entry in sub_entries.filter_map(|e| e.ok()) {
                        if sub_entry.path().is_file() {
                            size_bytes += sub_entry.metadata().map(|m| m.len()).unwrap_or(0);
                        }
                    }
                }

                let size_formatted = if size_bytes >= 1_073_741_824 {
                    format!("{:.2} GB", size_bytes as f64 / 1_073_741_824.0)
                } else {
                    format!("{:.0} MB", size_bytes as f64 / 1_048_576.0)
                };

                let filename = path.file_name().unwrap().to_string_lossy().to_string();
                let filename_lower = filename.to_lowercase();

                let (name, quality, speed) = if filename_lower.contains("moonshine")
                    || path.join("preprocess.onnx").exists()
                {
                    ("Moonshine ASR", 90, 85)
                } else if filename_lower.contains("canary") {
                    ("NVIDIA Canary-Qwen", 94, 60)
                } else if filename_lower.contains("parakeet") || path.join("joiner.onnx").exists() {
                    ("NVIDIA Parakeet TDT", 88, 92)
                } else {
                    ("ONNX Model", 80, 70)
                };

                let display_name = format!("{} ({})", name, filename);
                let is_active = Some(path.to_string_lossy().to_string()) == active_path;

                models.push(LocalModel {
                    name: display_name,
                    filename,
                    path: path.to_string_lossy().to_string(),
                    size_bytes,
                    size_formatted,
                    quality,
                    speed,
                    is_active,
                });
            }
        }
    }

    Ok(models)
}

#[derive(serde::Serialize, Clone)]
struct ModelStatus {
    active: Option<String>,
    loading: Option<String>,
}

#[tauri::command]
async fn load_model(
    model_path: String,
    stt_controller: tauri::State<'_, SttController>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    {
        let mut s = stt_controller.state.lock().unwrap();
        s.loading_model_path = Some(model_path.clone());
    }
    let _ = app_handle.emit("model-status-changed", ());

    let controller = stt_controller.inner().clone();
    let model_path_clone = model_path.clone();

    let res =
        tauri::async_runtime::spawn_blocking(move || controller.load_model(&model_path_clone))
            .await;

    {
        let mut s = stt_controller.state.lock().unwrap();
        s.loading_model_path = None;
    }
    let _ = app_handle.emit("model-status-changed", ());

    match res {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
fn get_active_model(
    stt_controller: tauri::State<'_, SttController>,
) -> Result<Option<String>, String> {
    let s = stt_controller.state.lock().unwrap();
    Ok(s.active_model_path.clone())
}

#[tauri::command]
fn get_model_status(
    stt_controller: tauri::State<'_, SttController>,
) -> Result<ModelStatus, String> {
    let s = stt_controller.state.lock().unwrap();
    Ok(ModelStatus {
        active: s.active_model_path.clone(),
        loading: s.loading_model_path.clone(),
    })
}

#[tauri::command]
fn get_models_dir(app_handle: tauri::AppHandle) -> Result<String, String> {
    let app_local_data = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;
    let models_dir = app_local_data.join("models");
    Ok(models_dir.to_string_lossy().to_string())
}

#[tauri::command]
fn set_secure_api_key(provider: String, key: String) -> Result<(), String> {
    if key.trim().is_empty() {
        return delete_secure_api_key(provider);
    }
    let entry = keyring::Entry::new("simplevoice-app", &format!("api_key_{}", provider))
        .map_err(|e| format!("Failed to access keyring: {}", e))?;
    entry
        .set_password(&key)
        .map_err(|e| format!("Failed to set key in keyring: {}", e))?;
    Ok(())
}

#[tauri::command]
fn get_secure_api_key(provider: String) -> Result<String, String> {
    let entry = keyring::Entry::new("simplevoice-app", &format!("api_key_{}", provider))
        .map_err(|e| format!("Failed to access keyring: {}", e))?;
    match entry.get_password() {
        Ok(pass) => Ok(pass),
        Err(keyring::Error::NoEntry) => Ok("".to_string()),
        Err(e) => Err(format!("Failed to retrieve key from keyring: {}", e)),
    }
}

#[tauri::command]
fn delete_secure_api_key(provider: String) -> Result<(), String> {
    let entry = keyring::Entry::new("simplevoice-app", &format!("api_key_{}", provider))
        .map_err(|e| format!("Failed to access keyring: {}", e))?;
    match entry.delete_password() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Failed to delete key from keyring: {}", e)),
    }
}

#[tauri::command]
fn has_secure_api_key(provider: String) -> Result<bool, String> {
    let entry = keyring::Entry::new("simplevoice-app", &format!("api_key_{}", provider))
        .map_err(|e| format!("Failed to access keyring: {}", e))?;
    match entry.get_password() {
        Ok(pass) => Ok(!pass.trim().is_empty()),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(e) => Err(format!("Failed to check key in keyring: {}", e)),
    }
}

#[tauri::command]
async fn transcribe_audio(
    samples: Vec<f32>,
    engine: String,
    provider: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    language: Option<String>,
    stt_controller: tauri::State<'_, SttController>,
) -> Result<String, String> {
    let controller = stt_controller.inner().clone();
    tauri::async_runtime::spawn(async move {
        if engine == "openai-cloud" {
            let provider_name = provider.unwrap_or_else(|| "openai".to_string());
            let key = get_secure_api_key(provider_name.clone())?;
            if key.trim().is_empty() {
                return Err(format!("ASR API Key for {} is missing or empty. Please set it in models/engines settings.", provider_name));
            }
            crate::stt::cloud::transcribe_cloud(
                &samples,
                &key,
                model.as_deref(),
                base_url.as_deref(),
                language.as_deref(),
            )
            .await
        } else {
            tauri::async_runtime::spawn_blocking(move || {
                controller.transcribe(&samples, language.as_deref())
            })
            .await
            .map_err(|e| e.to_string())?
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn refine_transcription(
    text: String,
    provider: String,
    model: String,
    prompt: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn(async move {
        let key = get_secure_api_key(provider.clone())?;
        if key.trim().is_empty() {
            return Err(format!(
                "API Key for {} is missing or empty. Please set it in preferences.",
                provider
            ));
        }
        crate::refiner::refine_text(&text, &provider, &model, &key, &prompt).await
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn get_last_recording_samples(
    controller: tauri::State<'_, AudioController>,
) -> Result<Vec<f32>, String> {
    let s = controller.state.lock().unwrap();
    Ok(s.last_samples.clone())
}

#[tauri::command]
fn save_config(app_handle: tauri::AppHandle, config: String) -> Result<(), String> {
    let app_local_data = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&app_local_data).map_err(|e| e.to_string())?;
    let config_path = app_local_data.join("config.json");
    std::fs::write(&config_path, config).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn load_config(app_handle: tauri::AppHandle) -> Result<String, String> {
    let app_local_data = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;
    let config_path = app_local_data.join("config.json");
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
        Ok(content)
    } else {
        Ok("{}".to_string())
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct TranscriptionItem {
    id: String,
    timestamp: String,
    date: String,
    text: String,
    model: String,
    wav_path: Option<String>,
    duration_sec: Option<f64>,
}

#[tauri::command]
async fn save_transcription_data(
    wav_path: String,
    text: String,
    model: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let wav_path_buf = std::path::PathBuf::from(&wav_path);
    let parent_dir = wav_path_buf
        .parent()
        .ok_or("No parent directory for wav file")?;

    let id = parent_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| chrono::Local::now().timestamp().to_string());

    let duration_sec = if let Ok(reader) = hound::WavReader::open(&wav_path) {
        let spec = reader.spec();
        Some(reader.duration() as f64 / spec.sample_rate as f64)
    } else {
        None
    };

    let now = chrono::Local::now();
    let timestamp = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let date = now.format("%Y-%m-%d").to_string();
    let dur_val = duration_sec.unwrap_or(0.0);
    let word_count = text.split_whitespace().count() as i32;

    // Use sqlx directly to the same database file used by the plugin
    let app_dir = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?;
    let db_path = app_dir.join("simplevoice.db");
    let db_url = format!("sqlite:{}", db_path.to_string_lossy());

    let pool = sqlx::SqlitePool::connect(&db_url)
        .await
        .map_err(|e| e.to_string())?;

    // 1. Insert Transcription
    sqlx::query(
        "INSERT OR IGNORE INTO transcriptions (id, timestamp, date, text, model, wav_path, duration_sec) VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&id)
    .bind(&timestamp)
    .bind(&date)
    .bind(&text)
    .bind(&model)
    .bind(&wav_path)
    .bind(dur_val)
    .execute(&pool)
    .await
    .map_err(|e| e.to_string())?;

    // 2. Update Daily Usage
    sqlx::query(
        "INSERT INTO daily_usage (date, words_generated, time_transcribed_sec)
         VALUES (?, ?, ?)
         ON CONFLICT(date) DO UPDATE SET
         words_generated = words_generated + excluded.words_generated,
         time_transcribed_sec = time_transcribed_sec + excluded.time_transcribed_sec",
    )
    .bind(&date)
    .bind(word_count)
    .bind(dur_val)
    .execute(&pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

async fn run_json_migration(app_handle: &tauri::AppHandle) -> Result<(), String> {
    let app_dir = app_handle
        .path()
        .app_config_dir()
        .map_err(|e| e.to_string())?;
    let db_path = app_dir.join("simplevoice.db");
    let db_url = format!("sqlite:{}", db_path.to_string_lossy());

    // Ensure parent dir exists (plugin might not have created it yet if we run too early)
    let _ = std::fs::create_dir_all(&app_dir);

    let pool = sqlx::SqlitePool::connect(&db_url)
        .await
        .map_err(|e| e.to_string())?;

    let app_local_data = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;
    let recordings_dir = app_local_data.join("recordings");

    if !recordings_dir.exists() {
        return Ok(());
    }

    if let Ok(entries) = std::fs::read_dir(&recordings_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let data_json_path = path.join("data.json");
                if data_json_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&data_json_path) {
                        if let Ok(item) = serde_json::from_str::<TranscriptionItem>(&content) {
                            let word_count = item.text.split_whitespace().count() as i32;
                            let dur_val = item.duration_sec.unwrap_or(0.0);
                            let date_val = if item.date.len() > 10 {
                                &item.date[0..10]
                            } else {
                                &item.date
                            };

                            let _ = sqlx::query(
                                "INSERT OR IGNORE INTO transcriptions (id, timestamp, date, text, model, wav_path, duration_sec) VALUES (?, ?, ?, ?, ?, ?, ?)"
                            )
                            .bind(&item.id)
                            .bind(&item.timestamp)
                            .bind(&item.date)
                            .bind(&item.text)
                            .bind(&item.model)
                            .bind(&item.wav_path)
                            .bind(dur_val)
                            .execute(&pool)
                            .await;

                            let _ = sqlx::query(
                                "INSERT INTO daily_usage (date, words_generated, time_transcribed_sec)
                                 VALUES (?, ?, ?)
                                 ON CONFLICT(date) DO UPDATE SET
                                 words_generated = words_generated + excluded.words_generated,
                                 time_transcribed_sec = time_transcribed_sec + excluded.time_transcribed_sec"
                            )
                            .bind(date_val)
                            .bind(word_count)
                            .bind(dur_val)
                            .execute(&pool)
                            .await;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
fn save_history(_app_handle: tauri::AppHandle, _history: String) -> Result<(), String> {
    // No-op fallback
    Ok(())
}

#[tauri::command]
fn load_history(app_handle: tauri::AppHandle) -> Result<String, String> {
    let app_local_data = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;
    let recordings_dir = app_local_data.join("recordings");
    if !recordings_dir.exists() {
        return Ok("[]".to_string());
    }

    let mut items = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&recordings_dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                let data_json_path = path.join("data.json");
                if data_json_path.exists() {
                    if let Ok(content) = std::fs::read_to_string(&data_json_path) {
                        if let Ok(mut item) = serde_json::from_str::<TranscriptionItem>(&content) {
                            // If duration_sec is missing (old recording), compute and save it back
                            if item.duration_sec.is_none() {
                                if let Some(ref w_path) = item.wav_path {
                                    if let Ok(reader) = hound::WavReader::open(w_path) {
                                        let spec = reader.spec();
                                        let dur =
                                            reader.duration() as f64 / spec.sample_rate as f64;
                                        item.duration_sec = Some(dur);
                                        // Try to save the updated item back to disk
                                        if let Ok(updated) = serde_json::to_string_pretty(&item) {
                                            let _ = std::fs::write(&data_json_path, updated);
                                        }
                                    }
                                }
                            }
                            items.push(item);
                        }
                    }
                }
            }
        }
    }

    // Sort items by directory/id name (which is YYYY-MM-DD_HH-mm-ss) descending
    items.sort_by(|a, b| b.id.cmp(&a.id));

    let serialized = serde_json::to_string(&items).map_err(|e| e.to_string())?;
    Ok(serialized)
}

/// Plays the "done" chime to notify the user that transcription text is ready.
#[tauri::command]
fn play_done_sound(app_handle: tauri::AppHandle) {
    play_backend_sound(&app_handle, "done");
}

/// Simulates Cmd+V in the previously-focused application so the clipboard
/// text is pasted automatically. Requires macOS Accessibility permission.
#[tauri::command]
fn paste_text() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        // Use osascript to press Cmd+V in whichever app currently has keyboard focus.
        // A small delay lets the clipboard write from the webview flush first.
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(80));
            let script = r#"tell application "System Events" to keystroke "v" using command down"#;
            let _ = std::process::Command::new("osascript")
                .arg("-e")
                .arg(script)
                .output();
        });
    }

    Ok(())
}

#[tauri::command]
fn clear_history_cmd(app_handle: tauri::AppHandle) -> Result<(), String> {
    let app_local_data = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;
    let recordings_dir = app_local_data.join("recordings");
    if recordings_dir.exists() {
        std::fs::remove_dir_all(&recordings_dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn delete_file_cmd(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if p.exists() && p.is_file() {
        std::fs::remove_file(p).map_err(|e| e.to_string())?;

        // Also try to remove the parent directory if it's empty (to clean up the timestamp folder)
        if let Some(parent) = p.parent() {
            let _ = std::fs::remove_dir(parent); // We ignore errors here in case it's not empty
        }
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let global_shortcut_plugin = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                println!("Global shortcut pressed: {:?}", shortcut);
                let controller = app.state::<AudioController>();
                if controller.is_recording() {
                    if let Ok(wav_path) = controller.stop_recording(app) {
                        play_backend_sound(app, "stop");
                        let payload = wav_path.unwrap_or_else(|| "Recording stopped".to_string());
                        let _ = app.emit("recording-stopped", payload);
                    }
                } else {
                    let stt = app.state::<SttController>();
                    let config = app.state::<AppConfig>();
                    match is_recording_allowed(&config, &stt) {
                        Ok(_) => {
                            if controller.start_recording(app.clone()).is_ok() {
                                play_backend_sound(app, "start");
                                let _ = app.emit("recording-started", ());
                            }
                        }
                        Err(reason) => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                            let _ = app.emit("recording-failed-to-start", reason);
                        }
                    }
                }
                let _ = rebuild_tray_menu(app);
            }
        })
        .build();

    let migrations = vec![Migration {
        version: 1,
        description: "create_initial_tables",
        sql: include_str!("../migrations/01_init.sql"),
        kind: MigrationKind::Up,
    }];

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(global_shortcut_plugin)
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:simplevoice.db", migrations)
                .build(),
        )
        .manage(AudioController::new())
        .manage(SttController::new())
        .manage(AppConfig {
            active: Mutex::new(ActiveConfig {
                engine: "local".to_string(),
                provider: "openai".to_string(),
            }),
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let _ = run_json_migration(&app_handle).await;
            });

            let app_handle = app.handle();
            let _ = rebuild_tray_menu(app_handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_audio_devices,
            set_selected_device,
            start_recording,
            stop_recording,
            register_shortcut,
            set_vad_enabled,
            scan_models,
            load_model,
            get_active_model,
            get_models_dir,
            transcribe_audio,
            get_last_recording_samples,
            refine_transcription,
            set_secure_api_key,
            get_secure_api_key,
            delete_secure_api_key,
            has_secure_api_key,
            update_active_config,
            open_folder,
            save_config,
            load_config,
            save_history,
            load_history,
            clear_history_cmd,
            delete_file_cmd,
            save_transcription_data,
            set_transcribing,
            get_model_status,
            play_done_sound,
            paste_text
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
