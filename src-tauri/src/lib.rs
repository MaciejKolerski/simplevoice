mod audio;
mod error;
mod linux_shortcuts;
mod media_control;
mod stt;
use audio::AudioController;
use std::sync::Mutex;
use stt::SttController;
use tauri::menu::{
    CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::Shortcut;
use tauri_plugin_sql::{Migration, MigrationKind};
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
use tauri::State;
use base64::Engine;

/// Stores the most recent transcription text so the "Copy Last" shortcut
/// can re-copy it to the clipboard without re-transcribing.
pub struct LastTranscription {
    pub text: Mutex<Option<String>>,
}

/// Tracks which action each registered global shortcut should trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ShortcutAction {
    Record,
    CopyLast,
}

struct ShortcutEntry {
    shortcut: Shortcut,
    action: ShortcutAction,
}

pub struct ShortcutRegistry {
    entries: Mutex<Vec<ShortcutEntry>>,
}

#[derive(Serialize, FromRow)]
struct Transcription {
    id: String,
    timestamp: String,
    date: String,
    text: String,
    model: String,
    wav_path: Option<String>,
    duration_sec: Option<f64>,
}

#[derive(Serialize)]
struct UsageStats {
    total_transcriptions: i32,
    total_words: i32,
    total_duration_sec: f64,
    daily: Vec<DailyUsage>,
}

#[derive(Serialize, FromRow)]
struct DailyUsage {
    date: String,
    words_generated: i32,
    time_transcribed_sec: f64,
}

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
    let _ = controller.refresh_devices();
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
    pub gpu_enabled: bool,
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

fn is_pause_audio_enabled(app_handle: &tauri::AppHandle) -> bool {
    let app_local_data = match app_handle.path().app_local_data_dir() {
        Ok(dir) => dir,
        Err(_) => return false,
    };
    let config_path = app_local_data.join("config.json");
    if !config_path.exists() {
        return false;
    }
    let content = match std::fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if let Some(val) = json.get("pause_audio_on_record") {
        if let Some(b) = val.as_bool() {
            return b;
        }
        if let Some(s) = val.as_str() {
            return s == "true";
        }
    }
    false
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
        #[cfg(target_os = "linux")]
        {
            // Use pw-play (part of PipeWire, already installed on most modern Arch setups)
            let _ = std::process::Command::new("pw-play").arg(&path).spawn();
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            // Windows fallback using rodio
            std::thread::spawn(move || {
                if let Ok(file) = std::fs::File::open(&path) {
                    if let Ok(source) = rodio::Decoder::new(std::io::BufReader::new(file)) {
                        if let Ok((_stream, handle)) = rodio::OutputStream::try_default() {
                            if let Ok(sink) = rodio::Sink::try_new(&handle) {
                                sink.append(source);
                                sink.sleep_until_end();
                            }
                        }
                    }
                }
            });
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
    let pause_audio = is_pause_audio_enabled(&app_handle);
    controller.start_recording(app_handle.clone(), pause_audio)?;
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
    let path = std::path::Path::new(&path);

    // Basic validation to prevent obvious path traversal
    if path.components().any(|c| c.as_os_str() == "..") {
        return Err("Access denied: invalid path".to_string());
    }

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

    let copy_last_item = MenuItemBuilder::new("Copy Last Transcription")
        .id("copy_last")
        .enabled({
            let last = app_handle.state::<LastTranscription>();
            let t = last.text.lock().unwrap();
            t.as_ref().is_some_and(|s| !s.trim().is_empty())
        })
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
        .item(&copy_last_item)
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

/// Toggles the voice recording state.
/// If active, stops the recording and emits the transcribed payload.
/// If inactive, starts recording if allowed by configuration.
fn toggle_recording(app: &tauri::AppHandle) {
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
                let pause_audio = is_pause_audio_enabled(app);
                if controller.start_recording(app.clone(), pause_audio).is_ok() {
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

fn handle_tray_menu_event(app: &tauri::AppHandle, id: &str) {
    if id == "toggle_recording" {
        toggle_recording(app);
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
    } else if id == "copy_last" {
        let last = app.state::<LastTranscription>();
        let text = last.text.lock().unwrap().clone();
        if let Some(ref t) = text {
            if !t.trim().is_empty() {
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let _ = clipboard.set_text(t.clone());
                    play_backend_sound(app, "done");
                    let _ = app.emit("copy-last-success", t.clone());
                }
            }
        }
    } else if id == "quit" {
        app.exit(0);
    }
}

/// Re-registers all shortcuts in the registry with the OS.
/// Called whenever any shortcut changes.
fn sync_all_shortcuts(app_handle: &tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let _ = app_handle;
        Ok(())
    }

    #[cfg(not(target_os = "linux"))]
    {
        use tauri_plugin_global_shortcut::GlobalShortcutExt;

        let global_shortcut = app_handle.global_shortcut();
        let _ = global_shortcut.unregister_all();

        let registry = app_handle.state::<ShortcutRegistry>();
        let entries = registry.entries.lock().unwrap();

        for entry in entries.iter() {
            global_shortcut
                .register(entry.shortcut)
                .map_err(|e| format!("Failed to register shortcut: {}", e))?;
        }
        Ok(())
    }
}

#[tauri::command]
fn register_shortcut(shortcut_str: String, app_handle: tauri::AppHandle) -> Result<(), String> {
    let registry = app_handle.state::<ShortcutRegistry>();
    let mut entries = registry.entries.lock().unwrap();

    // Remove existing Record entry
    entries.retain(|e| e.action != ShortcutAction::Record);

    if !shortcut_str.trim().is_empty() {
        let shortcut: Shortcut = shortcut_str
            .parse()
            .map_err(|e| format!("Failed to parse shortcut '{}': {}", shortcut_str, e))?;
        entries.push(ShortcutEntry {
            shortcut,
            action: ShortcutAction::Record,
        });
    }

    drop(entries);
    let sync_res = sync_all_shortcuts(&app_handle);

    #[cfg(target_os = "linux")]
    {
        if shortcut_str.trim().is_empty() {
            let _ = linux_shortcuts::unregister_native_shortcut("toggle");
        } else {
            let exe_path = std::env::current_exe()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let toggle_cmd = format!("\"{}\" --toggle", exe_path);
            let _ = linux_shortcuts::register_native_shortcut(
                "SimpleVoice Toggle Recording",
                &toggle_cmd,
                &shortcut_str,
                "toggle",
            );
        }
    }

    sync_res?;

    println!(
        "Successfully registered global recording shortcut: {}",
        shortcut_str
    );
    Ok(())
}

#[tauri::command]
fn register_copy_shortcut(
    shortcut_str: String,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let registry = app_handle.state::<ShortcutRegistry>();
    let mut entries = registry.entries.lock().unwrap();

    // Remove existing CopyLast entry
    entries.retain(|e| e.action != ShortcutAction::CopyLast);

    if !shortcut_str.trim().is_empty() {
        let shortcut: Shortcut = shortcut_str
            .parse()
            .map_err(|e| format!("Failed to parse shortcut '{}': {}", shortcut_str, e))?;
        entries.push(ShortcutEntry {
            shortcut,
            action: ShortcutAction::CopyLast,
        });
    }

    drop(entries);
    let sync_res = sync_all_shortcuts(&app_handle);

    #[cfg(target_os = "linux")]
    {
        if shortcut_str.trim().is_empty() {
            let _ = linux_shortcuts::unregister_native_shortcut("copy");
        } else {
            let exe_path = std::env::current_exe()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let copy_cmd = format!("\"{}\" --copy-last", exe_path);
            let _ = linux_shortcuts::register_native_shortcut(
                "SimpleVoice Copy Last Transcription",
                &copy_cmd,
                &shortcut_str,
                "copy",
            );
        }
    }

    sync_res?;

    println!(
        "Successfully registered global copy-last shortcut: {}",
        shortcut_str
    );
    Ok(())
}

#[tauri::command]
fn set_last_transcription(text: String, last_transcription: tauri::State<'_, LastTranscription>) {
    let mut t = last_transcription.text.lock().unwrap();
    *t = Some(text);
}

#[tauri::command]
fn copy_last_transcription(
    last_transcription: tauri::State<'_, LastTranscription>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    let t = last_transcription.text.lock().unwrap();
    match t.as_ref() {
        Some(text) if !text.trim().is_empty() => {
            let mut clipboard = arboard::Clipboard::new()
                .map_err(|e| format!("Failed to access clipboard: {}", e))?;
            clipboard
                .set_text(text.clone())
                .map_err(|e| format!("Failed to copy to clipboard: {}", e))?;
            play_backend_sound(&app_handle, "done");
            let _ = app_handle.emit("copy-last-success", text.clone());
            Ok(text.clone())
        }
        _ => Err("No transcription available to copy.".to_string()),
    }
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

    // On Linux always start on CPU to prevent Vulkan crashes.
    // On macOS/Windows we can safely use GPU at startup.
    let use_gpu = if cfg!(target_os = "linux") {
        false
    } else {
        let app_config = app_handle.state::<AppConfig>();
        let c = app_config.active.lock().unwrap();
        c.gpu_enabled
    };
    let res =
        tauri::async_runtime::spawn_blocking(move || controller.load_model(&model_path_clone, use_gpu))
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
    samples: Option<Vec<f32>>,
    engine: String,
    provider: Option<String>,
    model: Option<String>,
    base_url: Option<String>,
    language: Option<String>,
    stt_controller: tauri::State<'_, SttController>,
    audio_controller: tauri::State<'_, AudioController>,
) -> Result<String, String> {
    let controller = stt_controller.inner().clone();
    
    let final_samples = samples.unwrap_or_else(|| {
        let s = audio_controller.state.lock().unwrap();
        s.last_samples.clone()
    });

    let text = {
        if engine == "openai-cloud" {
            let provider_name = provider.unwrap_or_else(|| "openai".to_string());
            let key = get_secure_api_key(provider_name.clone())?;
            if key.trim().is_empty() {
                return Err(format!("ASR API Key for {} is missing or empty. Please set it in models/engines settings.", provider_name));
            }
            crate::stt::cloud::transcribe_cloud(
                &final_samples,
                &key,
                Some(&provider_name),
                model.as_deref(),
                base_url.as_deref(),
                language.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())?
        } else {
            let result: Result<String, String> = tauri::async_runtime::spawn_blocking(move || {
                controller.transcribe(&final_samples, language.as_deref())
            })
            .await
            .map_err(|e| e.to_string())?;
            result?
        }
    };

    // Copy to system clipboard using arboard + wl-copy (reliable on Wayland/X11 even when minimized)
    if !text.trim().is_empty() {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(text.clone());
        }
        let _ = std::process::Command::new("wl-copy").arg(text.clone()).status();
    }

    Ok(text)
}

#[tauri::command]
fn has_last_recording_samples(
    controller: tauri::State<'_, AudioController>,
) -> Result<bool, String> {
    let s = controller.state.lock().unwrap();
    Ok(!s.last_samples.is_empty())
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

#[tauri::command]
fn get_gpu_enabled(config: tauri::State<'_, AppConfig>) -> bool {
    let c = config.active.lock().unwrap();
    c.gpu_enabled
}

#[tauri::command]
fn set_gpu_enabled(enabled: bool, config: tauri::State<'_, AppConfig>, app_handle: tauri::AppHandle) {
    {
        let mut c = config.active.lock().unwrap();
        c.gpu_enabled = enabled;
    }

    // Persist gpu_enabled to config.json
    if let Ok(app_local_data) = app_handle.path().app_local_data_dir() {
        let config_path = app_local_data.join("config.json");
        if let Ok(existing) = std::fs::read_to_string(&config_path) {
            if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&existing) {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("gpu_enabled".to_string(), serde_json::json!(enabled));
                    let _ = std::fs::write(&config_path, serde_json::to_string_pretty(&json).unwrap_or_default());
                }
            }
        } else {
            let json = serde_json::json!({ "gpu_enabled": enabled });
            let _ = std::fs::write(&config_path, serde_json::to_string_pretty(&json).unwrap_or_default());
        }
    }

    let _ = rebuild_tray_menu(&app_handle);
}

#[tauri::command]
fn minimize_window(window: tauri::Window) {
    let _ = window.minimize();
}

#[tauri::command]
fn maximize_window(window: tauri::Window) {
    if window.is_maximized().unwrap_or(false) {
        let _ = window.unmaximize();
    } else {
        let _ = window.maximize();
    }
}

#[tauri::command]
fn close_window(window: tauri::Window) {
    let _ = window.close();
}

#[tauri::command]
async fn save_transcription_data(
    wav_path: String,
    text: String,
    model: String,
    pool: State<'_, SqlitePool>,
) -> Result<(), String> {
    println!("DEBUG: save_transcription_data called with wav_path='{}', text='{}' (len={})", wav_path, text, text.len());

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
    .execute(&*pool)
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
    .execute(&*pool)
    .await
        .map_err(|e| e.to_string())?;

    println!("DEBUG: save_transcription_data completed successfully for id={}", id);
    Ok(())
}


#[tauri::command]
async fn clear_history_cmd(
    app_handle: tauri::AppHandle,
    pool: State<'_, SqlitePool>,
) -> Result<(), String> {
    // 1. Delete recordings from disk
    let app_local_data = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;
    let recordings_dir = app_local_data.join("recordings");
    if recordings_dir.exists() {
        std::fs::remove_dir_all(&recordings_dir).map_err(|e| e.to_string())?;
    }

    // 2. Clear SQL database using shared pool
    sqlx::query("DELETE FROM transcriptions")
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;

    sqlx::query("DELETE FROM daily_usage")
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn delete_transcription_cmd(
    id: String,
    path: Option<String>,
    _app_handle: tauri::AppHandle,
    pool: State<'_, SqlitePool>,
) -> Result<(), String> {
    // 1. Delete physical file if path is provided
    if let Some(wav_path) = path {
        let p = std::path::Path::new(&wav_path);
        if p.exists() && p.is_file() {
            let _ = std::fs::remove_file(p);
            // Try to remove parent dir if empty
            if let Some(parent) = p.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }
    }

    // 2. Delete from database using shared pool
    sqlx::query("DELETE FROM transcriptions WHERE id = ?")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn get_transcriptions(limit: Option<i32>, offset: Option<i32>, pool: State<'_, SqlitePool>) -> Result<Vec<Transcription>, String> {
    let limit = limit.unwrap_or(30);
    let offset = offset.unwrap_or(0);
    println!("DEBUG: get_transcriptions called with limit={}, offset={}", limit, offset);
    let transcriptions = sqlx::query_as::<_, Transcription>(
        "SELECT id, timestamp, date, text, model, wav_path, duration_sec FROM transcriptions ORDER BY id DESC LIMIT ? OFFSET ?"
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&*pool)
    .await
    .map_err(|e| {
        println!("ERROR: get_transcriptions failed: {}", e);
        e.to_string()
    })?;
    println!("DEBUG: get_transcriptions returned {} rows", transcriptions.len());
    Ok(transcriptions)
}

#[tauri::command]
fn get_audio_base64(path: String) -> Result<String, String> {
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let base64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(base64)
}

#[tauri::command]
fn play_wav(path: String) {
    std::thread::spawn(move || {
        if let Ok(file) = std::fs::File::open(&path) {
            if let Ok(source) = rodio::Decoder::new(std::io::BufReader::new(file)) {
                if let Ok((_stream, handle)) = rodio::OutputStream::try_default() {
                    if let Ok(sink) = rodio::Sink::try_new(&handle) {
                        sink.append(source);
                        sink.sleep_until_end();
                    }
                }
            }
        }
    });
}

#[tauri::command]
async fn get_usage_stats(pool: State<'_, SqlitePool>) -> Result<UsageStats, String> {
    let totals: (i32, i32, f64) = sqlx::query_as(
        "SELECT COUNT(*) as total_transcriptions, 
                COALESCE(SUM(words_generated), 0) as total_words, 
                COALESCE(SUM(time_transcribed_sec), 0.0) as total_duration_sec 
         FROM daily_usage"
    )
    .fetch_one(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    let daily: Vec<DailyUsage> = sqlx::query_as(
        "SELECT date, words_generated, time_transcribed_sec FROM daily_usage ORDER BY date DESC"
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(UsageStats {
        total_transcriptions: totals.0,
        total_words: totals.1,
        total_duration_sec: totals.2,
        daily,
    })
}

#[tauri::command]
fn play_done_sound(app_handle: tauri::AppHandle) {
    play_backend_sound(&app_handle, "done");
}

// ─── Cross-Platform Permission Checks ────────────────────────────────────────

/// Returns true if the application has Accessibility permissions on macOS.
/// On other platforms, always returns true since no equivalent permission is required.
#[tauri::command]
fn check_accessibility_permission() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos_accessibility_client::accessibility::application_is_trusted()
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Prompts the user to grant Accessibility permissions on macOS by showing the
/// system dialog. On other platforms, this is a no-op that returns success.
#[tauri::command]
fn request_accessibility_permission() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        macos_accessibility_client::accessibility::application_is_trusted_with_prompt();
    }
    Ok(())
}

/// Opens the System Settings to the Accessibility privacy pane (macOS) or
/// the equivalent settings page on other platforms.
#[tauri::command]
fn open_accessibility_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn()
            .map_err(|e| format!("Failed to open Accessibility settings: {}", e))?;
    }
    #[cfg(target_os = "windows")]
    {
        // Windows doesn't have an equivalent Accessibility permission page
        // but we can open Settings
        std::process::Command::new("ms-settings:easeofaccess-display")
            .spawn()
            .map_err(|e| format!("Failed to open settings: {}", e))?;
    }
    Ok(())
}

#[derive(serde::Serialize, Clone)]
struct PermissionsStatus {
    /// Whether Accessibility permission is granted (macOS only, always true elsewhere)
    accessibility: bool,
    /// The current platform identifier
    platform: String,
    /// Whether the current session is running under Wayland (Linux only, false elsewhere)
    is_wayland: bool,
    /// Detected Linux desktop environment (e.g. "gnome", "kde", "unknown", or "none" for non-Linux)
    desktop_env: String,
    /// The active GDK backend
    gdk_backend: String,
}

/// Returns the aggregated permissions status for the current platform.
#[tauri::command]
fn check_permissions_status() -> PermissionsStatus {
    let accessibility = {
        #[cfg(target_os = "macos")]
        {
            macos_accessibility_client::accessibility::application_is_trusted()
        }
        #[cfg(not(target_os = "macos"))]
        {
            true
        }
    };

    let platform = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    };

    let is_wayland = if cfg!(target_os = "linux") {
        std::env::var("WAYLAND_DISPLAY").is_ok()
            || std::env::var("XDG_SESSION_TYPE")
                .map(|v| v.to_lowercase() == "wayland")
                .unwrap_or(false)
    } else {
        false
    };

    let desktop_env = if cfg!(target_os = "linux") {
        linux_shortcuts::detect_desktop_environment()
    } else {
        "none".to_string()
    };

    let gdk_backend = std::env::var("GDK_BACKEND").unwrap_or_else(|_| "default".to_string());

    PermissionsStatus {
        accessibility,
        platform: platform.to_string(),
        is_wayland,
        desktop_env,
        gdk_backend,
    }
}

// ─── Keyboard Simulation ─────────────────────────────────────────────────────

#[tauri::command]
fn paste_text(text: String) -> Result<(), String> {
    let is_wayland = if cfg!(target_os = "linux") {
        std::env::var("WAYLAND_DISPLAY").is_ok()
            || std::env::var("XDG_SESSION_TYPE")
                .map(|v| v.to_lowercase() == "wayland")
                .unwrap_or(false)
    } else {
        false
    };

    if is_wayland {
        // On Wayland we type the text directly with wtype (more reliable than Ctrl+V simulation).
        // No window popup, works even if app is minimized.
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = std::process::Command::new("wtype").arg(text).status();
        return Ok(());
    }

    // X11/macOS/Windows: simulate Ctrl+V / Cmd+V with enigo
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};

    let settings = Settings::default();
    let mut enigo = Enigo::new(&settings).map_err(|e| {
        #[cfg(target_os = "macos")]
        {
            format!(
                "Failed to initialize keyboard simulation. \
                 Please grant Accessibility permissions in System Settings > \
                 Privacy & Security > Accessibility. Error: {}",
                e
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            format!("Failed to initialize keyboard simulation: {}", e)
        }
    })?;

    #[cfg(target_os = "macos")]
    {
        enigo
            .key(Key::Meta, Direction::Press)
            .map_err(|e| format!("Keyboard simulation failed (Meta press): {}", e))?;
        enigo
            .key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| format!("Keyboard simulation failed (V click): {}", e))?;
        enigo
            .key(Key::Meta, Direction::Release)
            .map_err(|e| format!("Keyboard simulation failed (Meta release): {}", e))?;
    }
    #[cfg(target_os = "windows")]
    {
        enigo
            .key(Key::Control, Direction::Press)
            .map_err(|e| format!("Keyboard simulation failed (Ctrl press): {}", e))?;
        enigo
            .key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| format!("Keyboard simulation failed (V click): {}", e))?;
        enigo
            .key(Key::Control, Direction::Release)
            .map_err(|e| format!("Keyboard simulation failed (Ctrl release): {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        enigo
            .key(Key::Control, Direction::Press)
            .map_err(|e| format!("Keyboard simulation failed (Ctrl press): {}", e))?;
        enigo
            .key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| format!("Keyboard simulation failed (V click): {}", e))?;
        enigo
            .key(Key::Control, Direction::Release)
            .map_err(|e| format!("Keyboard simulation failed (Ctrl release): {}", e))?;
    }
    Ok(())
}
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    {
        // Disable DMA-BUF renderer in WebKitGTK to prevent black screens/GBM failures on some GPUs
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    let global_shortcut_plugin = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                println!("Global shortcut pressed: {:?}", shortcut);

                // Determine action by looking up the shortcut in the registry
                let action = {
                    let registry = app.state::<ShortcutRegistry>();
                    let entries = registry.entries.lock().unwrap();
                    entries
                        .iter()
                        .find(|e| e.shortcut == *shortcut)
                        .map(|e| e.action.clone())
                };

                match action {
                    Some(ShortcutAction::CopyLast) => {
                        let last = app.state::<LastTranscription>();
                        let text = last.text.lock().unwrap().clone();
                        if let Some(ref t) = text {
                            if !t.trim().is_empty() {
                                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                    let _ = clipboard.set_text(t.clone());
                                    play_backend_sound(app, "done");
                                    let _ = app.emit("copy-last-success", t.clone());
                                }
                            }
                        }
                    }
                    Some(ShortcutAction::Record) | None => {
                        toggle_recording(app);
                    }
                }
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
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // Check if the application was invoked with the toggle argument
            if argv.iter().any(|arg| arg == "--toggle" || arg == "toggle") {
                toggle_recording(app);
            } else if argv.iter().any(|arg| arg == "--copy-last" || arg == "copy-last" || arg == "--copy" || arg == "copy") {
                let last_transcription = app.state::<LastTranscription>();
                let _ = copy_last_transcription(last_transcription, app.clone());
            } else if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(global_shortcut_plugin)
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:simplevoice.db", migrations)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .manage(AudioController::new())
        .manage(SttController::new())
        .manage(AppConfig {
            active: Mutex::new(ActiveConfig {
                engine: "local".to_string(),
                provider: "openai".to_string(),
                gpu_enabled: !cfg!(target_os = "linux"),
            }),
        })
        .manage(LastTranscription {
            text: Mutex::new(None),
        })
        .manage(ShortcutRegistry {
            entries: Mutex::new(Vec::new()),
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            {
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                if let Some(window) = app.get_webview_window("main") {
                    // Enforce macOS constraints programmatically since they are removed from tauri.conf.json
                    let _ = window.set_max_size(Some(tauri::Size::Logical(tauri::LogicalSize {
                        width: 1200.0,
                        height: 900.0,
                    })));
                    let _ = window.set_maximizable(false);
                }
            }

            #[cfg(target_os = "linux")]
            {
                // Repair any malformed shell comments (#) to correct C-style comments (//) in KDL config on startup
                linux_shortcuts::repair_wm_configs();

                if let Some(window) = app.get_webview_window("main") {
                    // Set window size on Linux to be different from macOS (e.g. 950x700)
                    let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
                        width: 950.0,
                        height: 700.0,
                    }));
                }
            }

            let app_handle = app.handle().clone();

            // Load persisted gpu_enabled from config.json
            if let Ok(app_local_data) = app_handle.path().app_local_data_dir() {
                let config_path = app_local_data.join("config.json");
                if let Ok(content) = std::fs::read_to_string(&config_path) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(enabled) = json.get("gpu_enabled").and_then(|v| v.as_bool()) {
                            if let Some(app_config) = app_handle.try_state::<AppConfig>() {
                                let mut c = app_config.active.lock().unwrap();
                                c.gpu_enabled = enabled;
                            }
                        }
                    }
                }
            }

            // Initialize SQLite connection pool once (fixes connection leak)
            let pool = tauri::async_runtime::block_on(async {
                let app_dir = app_handle
                    .path()
                    .app_config_dir()
                    .expect("Failed to get app config directory");
                let db_path = app_dir.join("simplevoice.db");
                let db_url = format!("sqlite:{}", db_path.to_string_lossy());
                SqlitePool::connect(&db_url)
                    .await
                    .expect("Failed to create SQLite pool")
            });
            app.manage(pool);

            let _ = rebuild_tray_menu(app.handle());

            // Delayed GPU reload after startup — only on Linux
            let gpu_enabled = cfg!(target_os = "linux") && {
                let app_config = app.state::<AppConfig>();
                let c = app_config.active.lock().unwrap();
                c.gpu_enabled
            };

            if gpu_enabled {
                let stt_controller = app.state::<SttController>().inner().clone();
                let app_handle = app.handle().clone();

                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                    let active_path = {
                        let s = stt_controller.state.lock().unwrap();
                        s.active_model_path.clone()
                    };

                    if let Some(path) = active_path {
                        let _ = stt_controller.load_model(&path, true);
                        let _ = app_handle.emit("model-status-changed", ());
                    }
                });
            }

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
            get_gpu_enabled,
            set_gpu_enabled,
            get_active_model,
            get_models_dir,
            transcribe_audio,
            has_last_recording_samples,
            set_secure_api_key,
            get_secure_api_key,
            delete_secure_api_key,
            has_secure_api_key,
            minimize_window,
            maximize_window,
            close_window,
            update_active_config,
            open_folder,
            save_config,
            load_config,
            clear_history_cmd,
            delete_transcription_cmd,
            save_transcription_data,
            get_transcriptions,
            get_audio_base64,
            play_wav,
            get_usage_stats,
            set_transcribing,
            get_model_status,
            play_done_sound,
            paste_text,
            register_copy_shortcut,
            set_last_transcription,
            copy_last_transcription,
            check_accessibility_permission,
            request_accessibility_permission,
            open_accessibility_settings,
            check_permissions_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}