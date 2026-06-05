mod audio;
mod error;
#[cfg(target_os = "linux")]
mod linux_shortcuts;
#[cfg(target_os = "linux")]
mod wayland_type;
#[cfg(target_os = "linux")]
mod evdev_shortcuts;
mod media_control;
pub mod stt;
use audio::AudioController;
use base64::Engine;
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
use std::sync::Mutex;
use stt::SttController;
use tauri::menu::{
    CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
};
use tauri::tray::TrayIconBuilder;
use tauri::State;
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::Shortcut;
use tauri_plugin_sql::{Migration, MigrationKind};

#[cfg(target_os = "macos")]
#[link(name = "AVFoundation", kind = "framework")]
extern "C" {}

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

/// Monochrome menu bar icon (waveform bars on a transparent background).
/// Used as a macOS template image so the system tints it for light/dark menu bars.
#[cfg(target_os = "macos")]
fn tray_template_image() -> Option<tauri::image::Image<'static>> {
    tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png")).ok()
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
            return Err("errors.no_model_loaded".to_string());
        }
    } else if c.engine == "openai-cloud" {
        let key_name = format!("api_key_{}", c.provider);
        let entry = keyring::Entry::new("simplevoice", &key_name);
        match entry {
            Ok(ent) => {
                if let Ok(password) = ent.get_password() {
                    if password.trim().is_empty() {
                        return Err("errors.cloud_not_configured".to_string());
                    }
                } else {
                    return Err("errors.cloud_not_configured".to_string());
                }
            }
            Err(_) => {
                return Err("errors.cloud_not_configured".to_string());
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
        if let Some(b) = val.as_bool() {
            return b;
        }
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

pub(crate) fn is_recording_window_locked(app_handle: &tauri::AppHandle) -> bool {
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
    if let Some(val) = json.get("recording_window_locked") {
        if let Some(b) = val.as_bool() {
            return b;
        }
    }
    true
}

pub(crate) fn get_recording_window_mode(app_handle: &tauri::AppHandle) -> String {
    let app_local_data = match app_handle.path().app_local_data_dir() {
        Ok(dir) => dir,
        Err(_) => return "always".to_string(),
    };
    let config_path = app_local_data.join("config.json");
    if !config_path.exists() {
        return "always".to_string();
    }
    let content = match std::fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(_) => return "always".to_string(),
    };
    let json: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return "always".to_string(),
    };
    if let Some(val) = json.get("recording_window_mode") {
        if let Some(s) = val.as_str() {
            return s.to_string();
        }
    }
    "always".to_string()
}

#[cfg(target_os = "macos")]
extern "C" {
    fn object_setClass(
        obj: *mut objc2::runtime::AnyObject,
        new_class: &objc2::runtime::AnyClass,
    ) -> *const objc2::runtime::AnyClass;
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn save_recording_window_position(app_handle: &tauri::AppHandle, x: i32, y: i32) {
    let app_local_data = match app_handle.path().app_local_data_dir() {
        Ok(dir) => dir,
        Err(_) => return,
    };
    let config_path = app_local_data.join("config.json");
    let _ = std::fs::create_dir_all(&app_local_data);
    let _guard = CONFIG_FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let mut json = if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            serde_json::from_str::<serde_json::Value>(&content)
                .unwrap_or_else(|_| serde_json::json!({}))
        } else {
            serde_json::json!({})
        }
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = json.as_object_mut() {
        obj.insert("recording_window_x".to_string(), serde_json::json!(x));
        obj.insert("recording_window_y".to_string(), serde_json::json!(y));
        obj.insert(
            "recording_window_has_custom_pos".to_string(),
            serde_json::json!(true),
        );
    }

    if let Ok(serialized) = serde_json::to_string_pretty(&json) {
        let _ = std::fs::write(&config_path, serialized);
     }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn get_recording_window_position(app_handle: &tauri::AppHandle) -> Option<(i32, i32)> {
    let app_local_data = app_handle.path().app_local_data_dir().ok()?;
    let config_path = app_local_data.join("config.json");
    if !config_path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(&config_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;

    let has_custom = json.get("recording_window_has_custom_pos")?.as_bool()?;
    if !has_custom {
        return None;
    }

    let x = json.get("recording_window_x")?.as_i64()? as i32;
    let y = json.get("recording_window_y")?.as_i64()? as i32;

    Some((x, y))
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
static WINDOW_INITIALIZED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(target_os = "macos")]
pub(crate) fn update_recording_window_visibility(app: &tauri::AppHandle) {
    let mode = get_recording_window_mode(app);
    let controller = app.state::<AudioController>();
    let is_recording = controller.is_recording();

    if let Some(window) = app.get_webview_window("recording_window") {
        let should_show = match mode.as_str() {
            "always" => true,
            "recording" => is_recording,
            "never" | _ => false,
        };

        if should_show {
            // Position the window bottom-center dynamically only on the very first show
            if !WINDOW_INITIALIZED.load(std::sync::atomic::Ordering::Relaxed) {
                let mut positioned = false;
                if let Some((x, y)) = get_recording_window_position(app) {
                    let _ = window.set_position(tauri::Position::Physical(
                        tauri::PhysicalPosition::new(x, y),
                    ));
                    positioned = true;
                }

                if !positioned {
                    if let Some(monitor) = window.current_monitor().ok().flatten() {
                        let size = monitor.size();
                        let pos = monitor.position();
                        let scale_factor = monitor.scale_factor();

                        let win_w = 200.0;

                        let x = pos.x + ((size.width as f64 - win_w * scale_factor) / 2.0) as i32;
                        let y = pos.y + (36.0 * scale_factor) as i32;

                        let _ = window.set_position(tauri::Position::Physical(
                            tauri::PhysicalPosition::new(x, y),
                        ));
                    }
                }
                WINDOW_INITIALIZED.store(true, std::sync::atomic::Ordering::Relaxed);
            }

            let _ = window.show();
            let _ = window.set_ignore_cursor_events(true);

            if let Ok(ns_win) = window.ns_window() {
                unsafe {
                    use objc2::msg_send;
                    if let Some(panel_class) = objc2::runtime::AnyClass::get(
                        std::ffi::CStr::from_bytes_with_nul(b"NSPanel\0").unwrap(),
                    ) {
                        let obj_ptr = ns_win as *mut objc2::runtime::AnyObject;
                        let _ = object_setClass(obj_ptr, panel_class);
                    }
                    let _: () = msg_send![ns_win as *mut objc2::runtime::AnyObject, setStyleMask: 128 as usize];
                    let _: () = msg_send![ns_win as *mut objc2::runtime::AnyObject, setCollectionBehavior: 273 as usize];
                    let _: () = msg_send![ns_win as *mut objc2::runtime::AnyObject, setLevel: 1000 as isize];
                    let _: () = msg_send![
                        ns_win as *mut objc2::runtime::AnyObject,
                        orderFrontRegardless
                    ];
                }
            }
        } else {
            let _ = window.hide();
        }
    }
}

/// Re-pins the macOS traffic-light buttons of the main window to the inset
/// configured in `tauri.macos.conf.json` (x: 12, y: 21).
///
/// Tauri's `trafficLightPosition` only positions the buttons once at window
/// creation; tao re-applies it from its NSView `drawRect:`, but the content
/// view of a wry window is a WKWebView, so that hook rarely fires. After the
/// window is shown AppKit re-lays out the buttons, and the timing differs
/// between a `tauri dev` binary and a bundled `.app` — so the release build
/// drifts lower than dev. Re-running tao's exact inset math on the relevant
/// window events keeps the buttons pinned identically in both.
#[cfg(target_os = "macos")]
fn reposition_main_traffic_lights<R: tauri::Runtime>(window: &tauri::Window<R>) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_core_foundation::{CGPoint, CGRect};

    // Keep in sync with `trafficLightPosition` in src-tauri/tauri.macos.conf.json.
    // This runtime re-pin is authoritative once the window is shown, so editing
    // only the JSON would be silently reverted to these values on the next event.
    const INSET_X: f64 = 12.0;
    const INSET_Y: f64 = 21.0;

    let Ok(ns_window_ptr) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window_ptr as *mut AnyObject;

    unsafe {
        // NSWindowButton discriminants: Close = 0, Miniaturize = 1, Zoom = 2.
        let close: *mut AnyObject = msg_send![ns_window, standardWindowButton: 0usize];
        let miniaturize: *mut AnyObject = msg_send![ns_window, standardWindowButton: 1usize];
        let zoom: *mut AnyObject = msg_send![ns_window, standardWindowButton: 2usize];
        if close.is_null() || miniaturize.is_null() || zoom.is_null() {
            return;
        }

        // The title bar container view is two superviews above the buttons.
        let close_superview: *mut AnyObject = msg_send![close, superview];
        if close_superview.is_null() {
            return;
        }
        let title_bar_container: *mut AnyObject = msg_send![close_superview, superview];
        if title_bar_container.is_null() {
            return;
        }

        let close_rect: CGRect = msg_send![close, frame];
        let window_rect: CGRect = msg_send![ns_window, frame];

        // Grow the title bar container so the buttons sit INSET_Y below the top.
        let title_bar_height = close_rect.size.height + INSET_Y;
        let mut title_bar_rect: CGRect = msg_send![title_bar_container, frame];
        title_bar_rect.size.height = title_bar_height;
        title_bar_rect.origin.y = window_rect.size.height - title_bar_height;
        let _: () = msg_send![title_bar_container, setFrame: title_bar_rect];

        // Preserve the system's button spacing, anchor the leftmost at INSET_X.
        let miniaturize_rect: CGRect = msg_send![miniaturize, frame];
        let space_between = miniaturize_rect.origin.x - close_rect.origin.x;

        for (i, &button) in [close, miniaturize, zoom].iter().enumerate() {
            let button_rect: CGRect = msg_send![button, frame];
            let origin = CGPoint {
                x: INSET_X + (i as f64) * space_between,
                y: button_rect.origin.y,
            };
            let _: () = msg_send![button, setFrameOrigin: origin];
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(crate) fn update_recording_window_visibility(app: &tauri::AppHandle) {
    let mode = get_recording_window_mode(app);
    let controller = app.state::<AudioController>();
    let is_recording = controller.is_recording();

    if let Some(window) = app.get_webview_window("recording_window") {
        let should_show = match mode.as_str() {
            "always" => true,
            "recording" => is_recording,
            "never" | _ => false,
        };

        if should_show {
            if !WINDOW_INITIALIZED.load(std::sync::atomic::Ordering::Relaxed) {
                let mut positioned = false;
                if let Some((x, y)) = get_recording_window_position(app) {
                    let _ = window.set_position(tauri::Position::Physical(
                        tauri::PhysicalPosition::new(x, y),
                    ));
                    positioned = true;
                }

                if !positioned {
                    if let Some(monitor) = window.current_monitor().ok().flatten() {
                        let size = monitor.size();
                        let pos = monitor.position();
                        let scale_factor = monitor.scale_factor();

                        let win_w = 200.0;

                        let x = pos.x + ((size.width as f64 - win_w * scale_factor) / 2.0) as i32;
                        let y = pos.y + (size.height as f64 * 0.05) as i32;

                        let _ = window.set_position(tauri::Position::Physical(
                            tauri::PhysicalPosition::new(x, y),
                        ));
                    }
                }
                WINDOW_INITIALIZED.store(true, std::sync::atomic::Ordering::Relaxed);
            }

            let _ = window.show();
            let _ = window.set_always_on_top(true);
            let _ = window.set_skip_taskbar(true);

            let locked = is_recording_window_locked(app);
            let _ = window.set_ignore_cursor_events(locked);
        } else {
            let _ = window.hide();
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub(crate) fn update_recording_window_visibility(_app: &tauri::AppHandle) {}

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
    update_recording_window_visibility(&app_handle);
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
        update_recording_window_visibility(&app_handle);
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
    let _ = app_handle.emit("transcribing-status", active);
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

#[derive(serde::Deserialize, Clone)]
struct TrayLabels {
    start_recording: String,
    stop_recording: String,
    copy_last: String,
    usage: String,
    models: String,
    history: String,
    settings: String,
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    lock_window: String,
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    unlock_window: String,
    select_microphone: String,
    default_microphone: String,
    quit: String,
}

impl Default for TrayLabels {
    fn default() -> Self {
        TrayLabels {
            start_recording: "Start Recording".into(),
            stop_recording: "Stop Recording".into(),
            copy_last: "Copy Last Transcription".into(),
            usage: "Usage".into(),
            models: "Models".into(),
            history: "History".into(),
            settings: "Settings".into(),
            lock_window: "Lock Recording Window Position".into(),
            unlock_window: "Unlock Recording Window Position".into(),
            select_microphone: "Select Microphone".into(),
            default_microphone: "Default System Microphone".into(),
            quit: "Quit".into(),
        }
    }
}

struct TrayLabelsState(std::sync::Mutex<TrayLabels>);

#[tauri::command]
fn set_tray_labels(labels: TrayLabels, app_handle: tauri::AppHandle) -> Result<(), String> {
    {
        let state = app_handle.state::<TrayLabelsState>();
        let mut current = state.0.lock().unwrap();
        *current = labels;
    }
    rebuild_tray_menu(&app_handle)
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

    let labels = app_handle
        .state::<TrayLabelsState>()
        .0
        .lock()
        .unwrap()
        .clone();

    let base_icon = app_handle.default_window_icon().cloned();

    // macOS menu bar: use a transparent monochrome template image (just the waveform
    // bars) so the system tints it for light/dark — no baked background. The colored
    // recording/processing dot needs a non-template icon, so template mode is toggled
    // per state. On Windows/Linux a transparent white icon would be invisible on light
    // trays, so those keep the full app icon (existing behaviour).
    #[cfg(target_os = "macos")]
    let (tray_icon_img, tray_is_template) = {
        let bars = tray_template_image().or_else(|| base_icon.clone());
        match bars {
            Some(bars) => {
                if is_recording {
                    (Some(draw_status_dot(&bars, [255, 59, 48, 255])), false) // iOS system red
                } else if is_saving || is_transcribing {
                    (Some(draw_status_dot(&bars, [0, 122, 255, 255])), false) // iOS system blue
                } else {
                    (Some(bars), true)
                }
            }
            None => (None, false),
        }
    };

    #[cfg(not(target_os = "macos"))]
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
        labels.stop_recording.as_str()
    } else {
        labels.start_recording.as_str()
    };
    let toggle_recording_item = MenuItemBuilder::new(toggle_label)
        .id("toggle_recording")
        .build(app_handle)
        .map_err(|e| e.to_string())?;

    let copy_last_item = MenuItemBuilder::new(labels.copy_last.as_str())
        .id("copy_last")
        .enabled({
            let last = app_handle.state::<LastTranscription>();
            let t = last.text.lock().unwrap();
            t.as_ref().is_some_and(|s| !s.trim().is_empty())
        })
        .build(app_handle)
        .map_err(|e| e.to_string())?;

    let nav_usage_item = MenuItemBuilder::new(labels.usage.as_str())
        .id("nav_usage")
        .build(app_handle)
        .map_err(|e| e.to_string())?;

    let nav_models_item = MenuItemBuilder::new(labels.models.as_str())
        .id("nav_models")
        .build(app_handle)
        .map_err(|e| e.to_string())?;

    let nav_history_item = MenuItemBuilder::new(labels.history.as_str())
        .id("nav_transcriptions")
        .build(app_handle)
        .map_err(|e| e.to_string())?;

    let nav_settings_item = MenuItemBuilder::new(labels.settings.as_str())
        .id("nav_settings")
        .build(app_handle)
        .map_err(|e| e.to_string())?;

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    let lock_item = {
        let locked = is_recording_window_locked(app_handle);
        let lock_label = if locked {
            labels.unlock_window.as_str()
        } else {
            labels.lock_window.as_str()
        };
        MenuItemBuilder::new(lock_label)
            .id("toggle_recording_window_lock")
            .build(app_handle)
            .map_err(|e| e.to_string())?
    };

    let devices = controller.list_devices().unwrap_or_default();
    let mic_menu = {
        let mut builder = SubmenuBuilder::new(app_handle, labels.select_microphone.as_str());

        let is_default_checked = selected_device.is_none();
        let default_mic_item = CheckMenuItemBuilder::new(labels.default_microphone.as_str())
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

    let quit_item = MenuItemBuilder::new(labels.quit.as_str())
        .id("quit")
        .build(app_handle)
        .map_err(|e| e.to_string())?;

    let separator = PredefinedMenuItem::separator(app_handle).map_err(|e| e.to_string())?;
    let separator2 = PredefinedMenuItem::separator(app_handle).map_err(|e| e.to_string())?;
    let separator3 = PredefinedMenuItem::separator(app_handle).map_err(|e| e.to_string())?;

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    let menu = MenuBuilder::new(app_handle)
        .item(&toggle_recording_item)
        .item(&copy_last_item)
        .item(&lock_item)
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

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
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
        #[cfg(target_os = "macos")]
        let _ = tray.set_icon_as_template(tray_is_template);
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
        #[cfg(target_os = "macos")]
        {
            builder = builder.icon_as_template(tray_is_template);
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
            update_recording_window_visibility(app);
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
                    update_recording_window_visibility(app);
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
    } else if id == "toggle_recording_window_lock" {
        let next_locked = !is_recording_window_locked(app);
        let _ = set_recording_window_locked(next_locked, app.clone());
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

/// On Wayland window-manager sessions (and unknown environments) we grab global
/// hotkeys natively via evdev rather than editing the compositor config. Full
/// desktop environments keep their dedicated native integration (gsettings,
/// kglobalshortcutsrc, xfconf), which suppresses the keypress and integrates
/// with their settings UI.
#[cfg(target_os = "linux")]
fn linux_uses_evdev(desktop_env: &str) -> bool {
    matches!(
        desktop_env,
        "niri" | "sway" | "hyprland" | "i3" | "unknown"
    )
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
        let de = linux_shortcuts::detect_desktop_environment();
        if linux_uses_evdev(&de) {
            // niri/sway/hyprland/i3/unknown: grab the key natively via evdev
            // instead of editing the compositor config.
            evdev_shortcuts::set_shortcut("toggle", &shortcut_str)?;
        } else if shortcut_str.trim().is_empty() {
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
        let de = linux_shortcuts::detect_desktop_environment();
        if linux_uses_evdev(&de) {
            evdev_shortcuts::set_shortcut("copy", &shortcut_str)?;
        } else if shortcut_str.trim().is_empty() {
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
    format: String,
    architecture: Option<String>,
    hf_model_id: Option<String>,
    needs_conversion: bool,
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
        if let Ok(info) = crate::stt::factory::AsrFactory::detect(&path, active_path.as_deref()) {
            let format_str = match info.format {
                crate::stt::traits::ModelFormat::GgmlBin => "ggml_bin".to_string(),
                crate::stt::traits::ModelFormat::Gguf => "gguf".to_string(),
                crate::stt::traits::ModelFormat::HfSafetensors => "hf_safetensors".to_string(),
                crate::stt::traits::ModelFormat::HfPytorch => "hf_pytorch".to_string(),
                crate::stt::traits::ModelFormat::Onnx => "onnx".to_string(),
                crate::stt::traits::ModelFormat::Nemo => "nemo".to_string(),
            };

            models.push(LocalModel {
                name: info.display_name,
                filename: info.filename,
                path: info.path,
                size_bytes: info.size_bytes,
                size_formatted: info.size_formatted,
                quality: info.quality_score,
                speed: info.speed_score,
                is_active: info.is_active,
                format: format_str,
                architecture: info.architecture,
                hf_model_id: info.hf_model_id,
                needs_conversion: info.needs_conversion,
            });
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
        // Guard: block duplicate concurrent loads of the same model (React StrictMode double mount)
        if s.loading_model_path.as_deref() == Some(&model_path) {
            eprintln!(
                "[load_model] Already loading {}, skipping duplicate",
                model_path
            );
            return Ok(());
        }
        s.loading_model_path = Some(model_path.clone());
    }
    let _ = app_handle.emit("model-status-changed", ());

    let controller = stt_controller.inner().clone();
    let model_path_clone = model_path.clone();

    let app_config = app_handle.state::<AppConfig>();
    let use_gpu = app_config.active.lock().unwrap().gpu_enabled;
    let res = tauri::async_runtime::spawn_blocking(move || {
        if use_gpu {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                controller.load_model(&model_path_clone, true)
            })) {
                Ok(result) => result,
                Err(_) => controller.load_model(&model_path_clone, false),
            }
        } else {
            controller.load_model(&model_path_clone, false)
        }
    })
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
    let entry = keyring::Entry::new("simplevoice", &format!("api_key_{}", provider))
        .map_err(|e| format!("Failed to access keyring: {}", e))?;
    entry
        .set_password(&key)
        .map_err(|e| format!("Failed to set key in keyring: {}", e))?;
    Ok(())
}

#[tauri::command]
fn get_secure_api_key(provider: String) -> Result<String, String> {
    let entry = keyring::Entry::new("simplevoice", &format!("api_key_{}", provider))
        .map_err(|e| format!("Failed to access keyring: {}", e))?;
    match entry.get_password() {
        Ok(pass) => Ok(pass),
        Err(keyring::Error::NoEntry) => Ok("".to_string()),
        Err(e) => Err(format!("Failed to retrieve key from keyring: {}", e)),
    }
}

#[tauri::command]
fn delete_secure_api_key(provider: String) -> Result<(), String> {
    let entry = keyring::Entry::new("simplevoice", &format!("api_key_{}", provider))
        .map_err(|e| format!("Failed to access keyring: {}", e))?;
    match entry.delete_password() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Failed to delete key from keyring: {}", e)),
    }
}

#[tauri::command]
fn has_secure_api_key(provider: String) -> Result<bool, String> {
    let entry = keyring::Entry::new("simplevoice", &format!("api_key_{}", provider))
        .map_err(|e| format!("Failed to access keyring: {}", e))?;
    match entry.get_password() {
        Ok(pass) => Ok(!pass.trim().is_empty()),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(e) => Err(format!("Failed to check key in keyring: {}", e)),
    }
}

#[tauri::command]
async fn list_cloud_models(
    provider: String,
    base_url: Option<String>,
) -> Result<Vec<String>, String> {
    let key = get_secure_api_key(provider.clone())?;
    if key.trim().is_empty() {
        return Err(format!(
            "API key for {} is missing. Set it above first.",
            provider
        ));
    }
    crate::stt::cloud::list_models(&provider, base_url.as_deref(), &key).await
}

/// macOS App Nap suppression, held for the lifetime of a transcription.
///
/// SimpleVoice runs as an `Accessory` (menu-bar) app with its main window
/// `visible: false`. Once recording stops, the process has no visible window,
/// is no longer audible, and holds no power assertions — so it satisfies every
/// macOS App Nap eligibility criterion and gets napped, which throttles the
/// main run loop. Tauri can only deliver a command's result to the webview by
/// evaluating JavaScript on that main thread (WKWebView is main-thread-only),
/// so while the app is napping the `transcribe_audio` response sits undelivered
/// until the next event (e.g. starting another recording) wakes the run loop —
/// the transcription appears to "hang" until you record again.
///
/// Holding an `NSProcessInfo` activity opts the app out of App Nap for the
/// duration of the work. This is the Apple-recommended, scoped mechanism; the
/// old `NSAppSleepDisabled` Info.plist key has been ignored since macOS 10.12.
#[cfg(target_os = "macos")]
struct AppNapGuard {
    process_info: objc2::rc::Retained<objc2_foundation::NSProcessInfo>,
    activity:
        objc2::rc::Retained<objc2::runtime::ProtocolObject<dyn objc2::runtime::NSObjectProtocol>>,
}

// The `NSProcessInfo` activity API is thread-safe (begin/end may be called from
// any thread), so the guard is safe to send across the `.await` points of the
// async command even though the underlying Objective-C smart pointers are not
// `Send` by default.
#[cfg(target_os = "macos")]
unsafe impl Send for AppNapGuard {}

#[cfg(target_os = "macos")]
impl AppNapGuard {
    fn begin(reason: &str) -> Self {
        use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
        let process_info = NSProcessInfo::processInfo();
        // `UserInitiated` opts out of App Nap (it also implies
        // `IdleSystemSleepDisabled`); transcription is short-lived work the user
        // is actively waiting on.
        let activity = process_info
            .beginActivityWithOptions_reason(NSActivityOptions::UserInitiated, &NSString::from_str(reason));
        Self { process_info, activity }
    }
}

#[cfg(target_os = "macos")]
impl Drop for AppNapGuard {
    fn drop(&mut self) {
        // SAFETY: `self.activity` is exactly the token returned by the matching
        // `beginActivityWithOptions:reason:` on this same `NSProcessInfo`.
        unsafe { self.process_info.endActivity(&self.activity) };
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
    // Keep the macOS main run loop awake until this command returns, so the
    // result is delivered to the webview immediately instead of being deferred
    // by App Nap. RAII: the activity ends on every exit path (incl. `?` errors).
    // (Named binding, not a bare `_`, so it lives for the whole function body.)
    #[cfg(target_os = "macos")]
    let _app_nap_guard = AppNapGuard::begin("SimpleVoice is transcribing audio");

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

    // Copy to system clipboard natively via arboard (cross-platform). On Wayland
    // arboard uses the wlr-data-control backend, which spawns a background server
    // to keep the selection alive after the Clipboard handle is dropped.
    if !text.trim().is_empty() {
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            let _ = clipboard.set_text(text.clone());
        }
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

/// Serializes read-modify-write access to config.json so the frontend's
/// whole-file `save_config` and the backend's `set_gpu_enabled` cannot interleave
/// and clobber each other.
static CONFIG_FILE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[tauri::command]
fn save_config(app_handle: tauri::AppHandle, config: String) -> Result<(), String> {
    let app_local_data = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&app_local_data).map_err(|e| e.to_string())?;
    let config_path = app_local_data.join("config.json");

    let _guard = CONFIG_FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    // The frontend caches the whole config at mount and writes the entire blob
    // back, so a plain overwrite silently drops keys it never loaded. Most
    // importantly `onboarding_completed` is written out-of-band: a stale settings
    // write would erase it and replay the tour on every launch. Merge the incoming
    // config over what is already on disk so unknown keys survive. `gpu_enabled` is
    // owned by the backend (set_gpu_enabled), so the on-disk value still wins for it.
    let to_write = match serde_json::from_str::<serde_json::Value>(&config) {
        Ok(incoming) => {
            let mut merged = std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                .filter(|v| v.is_object())
                .unwrap_or_else(|| serde_json::json!({}));
            let disk_gpu = merged.get("gpu_enabled").cloned();
            if let (Some(merged_obj), Some(incoming_obj)) =
                (merged.as_object_mut(), incoming.as_object())
            {
                for (k, v) in incoming_obj {
                    merged_obj.insert(k.clone(), v.clone());
                }
                match disk_gpu {
                    Some(gpu) => {
                        merged_obj.insert("gpu_enabled".to_string(), gpu);
                    }
                    None => {
                        merged_obj.remove("gpu_enabled");
                    }
                }
            }
            serde_json::to_string_pretty(&merged).map_err(|e| e.to_string())?
        }
        // Not a JSON object we can reason about: persist verbatim.
        Err(_) => config,
    };

    std::fs::write(&config_path, to_write).map_err(|e| e.to_string())?;
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
fn set_gpu_enabled(
    enabled: bool,
    config: tauri::State<'_, AppConfig>,
    app_handle: tauri::AppHandle,
) {
    {
        let mut c = config.active.lock().unwrap();
        c.gpu_enabled = enabled;
    }

    // Persist gpu_enabled to config.json. Held under the same lock as save_config
    // so the two read-modify-write paths can't interleave.
    if let Ok(app_local_data) = app_handle.path().app_local_data_dir() {
        let _ = std::fs::create_dir_all(&app_local_data);
        let config_path = app_local_data.join("config.json");
        let _guard = CONFIG_FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(existing) = std::fs::read_to_string(&config_path) {
            if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&existing) {
                if let Some(obj) = json.as_object_mut() {
                    obj.insert("gpu_enabled".to_string(), serde_json::json!(enabled));
                    let _ = std::fs::write(
                        &config_path,
                        serde_json::to_string_pretty(&json).unwrap_or_default(),
                    );
                }
            }
        } else {
            let json = serde_json::json!({ "gpu_enabled": enabled });
            let _ = std::fs::write(
                &config_path,
                serde_json::to_string_pretty(&json).unwrap_or_default(),
            );
        }
    }

    let _ = rebuild_tray_menu(&app_handle);
}

#[tauri::command]
fn set_recording_window_mode(mode: String, app_handle: tauri::AppHandle) -> Result<(), String> {
    let app_local_data = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&app_local_data).map_err(|e| e.to_string())?;
    let config_path = app_local_data.join("config.json");
    let _guard = CONFIG_FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let mut json = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
        serde_json::from_str::<serde_json::Value>(&content)
            .unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = json.as_object_mut() {
        obj.insert("recording_window_mode".to_string(), serde_json::json!(mode));
        if mode == "never" {
            obj.insert(
                "recording_window_has_custom_pos".to_string(),
                serde_json::json!(false),
            );
            obj.remove("recording_window_x");
            obj.remove("recording_window_y");
            #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
            {
                WINDOW_INITIALIZED.store(false, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let pretty = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
        std::fs::write(&config_path, pretty).map_err(|e| e.to_string())?;
    }

    update_recording_window_visibility(&app_handle);
    Ok(())
}

#[tauri::command]
fn is_recording_window_locked_cmd(app_handle: tauri::AppHandle) -> bool {
    is_recording_window_locked(&app_handle)
}

#[tauri::command]
fn set_recording_window_locked(locked: bool, app_handle: tauri::AppHandle) -> Result<(), String> {
    let app_local_data = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&app_local_data).map_err(|e| e.to_string())?;
    let config_path = app_local_data.join("config.json");
    let _guard = CONFIG_FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let mut json = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
        serde_json::from_str::<serde_json::Value>(&content)
            .unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = json.as_object_mut() {
        obj.insert("recording_window_locked".to_string(), serde_json::json!(locked));
    }

    let serialized = serde_json::to_string_pretty(&json).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, serialized).map_err(|e| e.to_string())?;

    if let Some(window) = app_handle.get_webview_window("recording_window") {
        let _ = window.set_ignore_cursor_events(locked);
    }

    let _ = app_handle.emit("recording-window-lock-status", locked);
    let _ = rebuild_tray_menu(&app_handle);

    Ok(())
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

    // 2. Fetch transcription info to update daily_usage before deleting
    let trans_opt: Option<(String, Option<f64>, String)> =
        sqlx::query_as("SELECT date, duration_sec, text FROM transcriptions WHERE id = ?")
            .bind(&id)
            .fetch_optional(&*pool)
            .await
            .map_err(|e| e.to_string())?;

    if let Some((date_str, duration_opt, text_str)) = trans_opt {
        let word_count = text_str.split_whitespace().count() as i32;
        let duration = duration_opt.unwrap_or(0.0);

        sqlx::query(
            "UPDATE daily_usage
             SET words_generated = CASE WHEN words_generated > ? THEN words_generated - ? ELSE 0 END,
                 time_transcribed_sec = CASE WHEN time_transcribed_sec > ? THEN time_transcribed_sec - ? ELSE 0.0 END
             WHERE date = ?"
        )
        .bind(word_count)
        .bind(word_count)
        .bind(duration)
        .bind(duration)
        .bind(&date_str)
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;

        // Clean up empty usage rows
        sqlx::query(
            "DELETE FROM daily_usage WHERE date = ? AND words_generated = 0 AND time_transcribed_sec <= 0.0"
        )
        .bind(&date_str)
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;
    }

    // 3. Delete from database using shared pool
    sqlx::query("DELETE FROM transcriptions WHERE id = ?")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
async fn get_transcriptions(
    limit: Option<i32>,
    offset: Option<i32>,
    pool: State<'_, SqlitePool>,
) -> Result<Vec<Transcription>, String> {
    let limit = limit.unwrap_or(30);
    let offset = offset.unwrap_or(0);
    let transcriptions = sqlx::query_as::<_, Transcription>(
        "SELECT id, timestamp, date, text, model, wav_path, duration_sec FROM transcriptions ORDER BY id DESC LIMIT ? OFFSET ?"
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(&*pool)
    .await
    .map_err(|e| {
        e.to_string()
    })?;
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
    let totals: (i32, f64) = sqlx::query_as(
        "SELECT COALESCE(SUM(words_generated), 0) as total_words,
                COALESCE(SUM(time_transcribed_sec), 0.0) as total_duration_sec
         FROM daily_usage",
    )
    .fetch_one(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    let total_transcriptions: (i32,) = sqlx::query_as("SELECT COUNT(*) FROM transcriptions")
        .fetch_one(&*pool)
        .await
        .map_err(|e| e.to_string())?;

    let daily: Vec<DailyUsage> = sqlx::query_as(
        "SELECT date, words_generated, time_transcribed_sec FROM daily_usage ORDER BY date DESC",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(UsageStats {
        total_transcriptions: total_transcriptions.0,
        total_words: totals.0,
        total_duration_sec: totals.1,
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
    /// Whether Microphone permission is granted (macOS only, always true elsewhere)
    microphone: bool,
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

    let microphone = {
        #[cfg(target_os = "macos")]
        {
            unsafe {
                let name = std::ffi::CStr::from_bytes_with_nul(b"AVCaptureDevice\0").unwrap();
                let cls = objc2::runtime::AnyClass::get(name);
                if let Some(cls) = cls {
                    let media_type = objc2_foundation::ns_string!("soun");
                    let status: objc2::ffi::NSInteger =
                        objc2::msg_send![cls, authorizationStatusForMediaType: media_type];
                    // AVAuthorizationStatusAuthorized is 3
                    status == 3
                } else {
                    false
                }
            }
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

    let desktop_env = {
        #[cfg(target_os = "linux")]
        {
            linux_shortcuts::detect_desktop_environment()
        }
        #[cfg(not(target_os = "linux"))]
        {
            "none".to_string()
        }
    };

    let gdk_backend = std::env::var("GDK_BACKEND").unwrap_or_else(|_| "default".to_string());

    PermissionsStatus {
        accessibility,
        microphone,
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
        // On Wayland we type the text directly via the virtual-keyboard protocol
        // (more reliable than Ctrl+V simulation, and works even if our window is
        // hidden). Give the previously focused app a moment to settle first.
        std::thread::sleep(std::time::Duration::from_millis(200));
        #[cfg(target_os = "linux")]
        {
            // Native injection only fails on compositors that lack the
            // virtual-keyboard protocol (e.g. GNOME). The text is already on the
            // clipboard, so the user can paste manually; surface the reason.
            return wayland_type::type_text(&text)
                .map_err(|e| format!("Native Wayland typing failed: {e}"));
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = &text;
            return Ok(());
        }
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
            } else if argv.iter().any(|arg| {
                arg == "--copy-last" || arg == "copy-last" || arg == "--copy" || arg == "copy"
            }) {
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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
        .manage(TrayLabelsState(std::sync::Mutex::new(TrayLabels::default())))
        .manage(stt::downloader::DownloadRegistry::default())
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = window.hide();
                }
                tauri::WindowEvent::Moved(pos) => {
                    if window.label() == "recording_window" {
                        save_recording_window_position(window.app_handle(), pos.x, pos.y);
                    }
                }
                #[cfg(target_os = "macos")]
                tauri::WindowEvent::Resized(_)
                | tauri::WindowEvent::Focused(true)
                | tauri::WindowEvent::ScaleFactorChanged { .. }
                | tauri::WindowEvent::ThemeChanged(_) => {
                    if window.label() == "main" {
                        reposition_main_traffic_lights(window);
                    }
                }
                _ => {}
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
                update_recording_window_visibility(app.handle());

                // Monitor the Command key state on macOS to toggle window click-through (allow dragging when Cmd is held)
                let app_handle = app.handle().clone();
                std::thread::spawn(move || {
                    let mut last_command_state = false;
                    loop {
                        std::thread::sleep(std::time::Duration::from_millis(150));
                        if let Some(window) = app_handle.get_webview_window("recording_window") {
                            if let Ok(visible) = window.is_visible() {
                                if visible {
                                    let command_pressed = unsafe {
                                        use objc2::msg_send;
                                        use objc2::runtime::AnyClass;
                                        if let Some(nsevent_class) = AnyClass::get(
                                            std::ffi::CStr::from_bytes_with_nul(b"NSEvent\0")
                                                .unwrap(),
                                        ) {
                                            let flags: usize =
                                                msg_send![nsevent_class, modifierFlags];
                                            (flags & 0x0010_0000) != 0
                                        } else {
                                            false
                                        }
                                    };

                                    if command_pressed != last_command_state {
                                        last_command_state = command_pressed;
                                        let window_clone = window.clone();
                                        let app_handle_clone = app_handle.clone();
                                        let _ = app_handle.run_on_main_thread(move || {
                                            let _ = window_clone
                                                .set_ignore_cursor_events(!command_pressed);
                                            // When Cmd key is released, save the window's current coordinates
                                            if !command_pressed {
                                                if let Ok(pos) = window_clone.outer_position() {
                                                    save_recording_window_position(
                                                        &app_handle_clone,
                                                        pos.x,
                                                        pos.y,
                                                    );
                                                }
                                            }
                                        });
                                    }
                                } else {
                                    last_command_state = false;
                                }
                            }
                        }
                    }
                });
            }

            #[cfg(target_os = "linux")]
            {
                // Repair any malformed shell comments (#) to correct C-style comments (//) in KDL config on startup
                linux_shortcuts::repair_wm_configs();

                let de = linux_shortcuts::detect_desktop_environment();
                if linux_uses_evdev(&de) {
                    // We now grab hotkeys via evdev. Remove any binds previous
                    // versions wrote into the compositor config so they don't
                    // fire the action a second time.
                    let _ = linux_shortcuts::unregister_native_shortcut("toggle");
                    let _ = linux_shortcuts::unregister_native_shortcut("copy");
                    evdev_shortcuts::init(app.handle().clone());
                }

                update_recording_window_visibility(app.handle());
            }

            #[cfg(target_os = "windows")]
            {
                update_recording_window_visibility(app.handle());
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
                let _ = std::fs::create_dir_all(&app_dir);
                let db_path = app_dir.join("simplevoice.db");
                let options = sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(db_path)
                    .create_if_missing(true);
                let pool = SqlitePool::connect_with(options)
                    .await
                    .expect("Failed to create SQLite pool");

                // Run database migrations to ensure all tables exist
                let _ = sqlx::migrate!("./migrations").run(&pool).await;

                pool
            });
            app.manage(pool);

            let _ = rebuild_tray_menu(app.handle());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_tray_labels,
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
            list_cloud_models,
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
            check_permissions_status,
            set_recording_window_mode,
            is_recording_window_locked_cmd,
            set_recording_window_locked,
            stt::converter::convert_model,
            stt::downloader::download_model,
            stt::downloader::pause_download,
            stt::downloader::cancel_download,
            stt::downloader::discard_download
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
