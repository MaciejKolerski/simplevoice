//! Native global hotkeys on Linux by reading evdev input devices directly.
//!
//! Wayland compositors deliberately stop applications from grabbing global key
//! combinations. niri in particular only supports keybinds declared in its
//! config file and exposes no runtime API to register them. To provide global
//! shortcuts without editing the user's compositor config, we read key events
//! straight from `/dev/input/event*` in background threads and match them
//! against the registered combinations.
//!
//! Trade-offs: this needs read access to the input devices (fine when the nodes
//! are world-readable, otherwise the user must be in the `input` group), and the
//! keypress is observed rather than consumed, so it still reaches the focused
//! application — pick a dedicated combination accordingly.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use evdev::{Device, EventType, KeyCode};
use tauri::{AppHandle, Emitter, Manager};

use crate::{LastTranscription, ShortcutAction};

// Modifier bitmask.
const MOD_CTRL: u8 = 1 << 0;
const MOD_SHIFT: u8 = 1 << 1;
const MOD_ALT: u8 = 1 << 2;
const MOD_META: u8 = 1 << 3;

// evdev key codes for the modifiers (from linux/input-event-codes.h).
const KEY_LEFTCTRL: u16 = 29;
const KEY_RIGHTCTRL: u16 = 97;
const KEY_LEFTSHIFT: u16 = 42;
const KEY_RIGHTSHIFT: u16 = 54;
const KEY_LEFTALT: u16 = 56;
const KEY_RIGHTALT: u16 = 100;
const KEY_LEFTMETA: u16 = 125;
const KEY_RIGHTMETA: u16 = 126;

// Drop repeated triggers of the same action within this window. Guards against
// duplicate input nodes for one physical keyboard reporting the same press.
const DEBOUNCE: Duration = Duration::from_millis(300);
// How often the manager thread rescans for (un)plugged keyboards.
const RESCAN_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Clone)]
struct Combo {
    mods: u8,
    key: u16,
    action: ShortcutAction,
}

struct State {
    app: AppHandle,
    combos: Mutex<Vec<Combo>>,
    last_fire: Mutex<HashMap<u8, Instant>>, // keyed by action discriminant
    handled: Mutex<HashSet<PathBuf>>,       // device nodes with a live listener
}

static STATE: OnceLock<State> = OnceLock::new();

/// Start the evdev listener. Safe to call once at startup; spawns a manager
/// thread that watches for keyboard devices (including hotplugged ones).
pub fn init(app: AppHandle) {
    let state = State {
        app,
        combos: Mutex::new(Vec::new()),
        last_fire: Mutex::new(HashMap::new()),
        handled: Mutex::new(HashSet::new()),
    };
    if STATE.set(state).is_err() {
        return; // already initialized
    }

    std::thread::Builder::new()
        .name("evdev-hotkey-manager".into())
        .spawn(manager_loop)
        .ok();
}

/// Register (or clear, when `shortcut_str` is empty) the combination for an
/// action. `action_id` is "toggle" or "copy". Returns an error if the shortcut
/// string cannot be parsed.
pub fn set_shortcut(action_id: &str, shortcut_str: &str) -> Result<(), String> {
    let Some(state) = STATE.get() else {
        return Err("evdev hotkeys not initialized".into());
    };
    let action = action_for_id(action_id)?;

    let mut combos = state.combos.lock().unwrap();
    combos.retain(|c| c.action != action);

    let trimmed = shortcut_str.trim();
    if !trimmed.is_empty() {
        let (mods, key) = parse_shortcut(trimmed)?;
        combos.push(Combo { mods, key, action });
    }
    Ok(())
}

fn action_for_id(action_id: &str) -> Result<ShortcutAction, String> {
    match action_id {
        "toggle" => Ok(ShortcutAction::Record),
        "copy" => Ok(ShortcutAction::CopyLast),
        "movebar" => Ok(ShortcutAction::MoveBar),
        other => Err(format!("unknown shortcut action '{other}'")),
    }
}

fn action_key(action: &ShortcutAction) -> u8 {
    match action {
        ShortcutAction::Record => 0,
        ShortcutAction::CopyLast => 1,
        ShortcutAction::MoveBar => 2,
    }
}

fn manager_loop() {
    loop {
        scan_and_spawn();
        std::thread::sleep(RESCAN_INTERVAL);
    }
}

fn scan_and_spawn() {
    let Some(state) = STATE.get() else { return };
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let is_event_node = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with("event"))
            .unwrap_or(false);
        if !is_event_node {
            continue;
        }
        if state.handled.lock().unwrap().contains(&path) {
            continue;
        }

        // A read error here is almost always missing permission on the node.
        let device = match Device::open(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if !is_keyboard(&device) {
            continue;
        }

        state.handled.lock().unwrap().insert(path.clone());
        let listener_path = path.clone();
        std::thread::Builder::new()
            .name("evdev-hotkey".into())
            .spawn(move || device_loop(listener_path, device))
            .ok();
    }
}

fn is_keyboard(device: &Device) -> bool {
    device
        .supported_keys()
        .map(|keys| keys.contains(KeyCode::KEY_ENTER) || keys.contains(KeyCode::KEY_A))
        .unwrap_or(false)
}

fn device_loop(path: PathBuf, mut device: Device) {
    // Currently held keys on THIS device, so modifier state is per-keyboard.
    let mut pressed: HashSet<u16> = HashSet::new();

    'outer: loop {
        let events = match device.fetch_events() {
            Ok(events) => events,
            Err(_) => break 'outer, // device unplugged or read error
        };
        for ev in events {
            if ev.event_type() != EventType::KEY {
                continue;
            }
            let code = ev.code();
            match ev.value() {
                1 => {
                    pressed.insert(code);
                    if !is_modifier(code) {
                        handle_press(code, &pressed);
                    }
                }
                0 => {
                    pressed.remove(&code);
                }
                _ => {} // value 2 == autorepeat: ignore
            }
        }
    }

    if let Some(state) = STATE.get() {
        state.handled.lock().unwrap().remove(&path);
    }
}

fn is_modifier(code: u16) -> bool {
    matches!(
        code,
        KEY_LEFTCTRL
            | KEY_RIGHTCTRL
            | KEY_LEFTSHIFT
            | KEY_RIGHTSHIFT
            | KEY_LEFTALT
            | KEY_RIGHTALT
            | KEY_LEFTMETA
            | KEY_RIGHTMETA
    )
}

fn mods_from_pressed(pressed: &HashSet<u16>) -> u8 {
    let mut mods = 0u8;
    if pressed.contains(&KEY_LEFTCTRL) || pressed.contains(&KEY_RIGHTCTRL) {
        mods |= MOD_CTRL;
    }
    if pressed.contains(&KEY_LEFTSHIFT) || pressed.contains(&KEY_RIGHTSHIFT) {
        mods |= MOD_SHIFT;
    }
    if pressed.contains(&KEY_LEFTALT) || pressed.contains(&KEY_RIGHTALT) {
        mods |= MOD_ALT;
    }
    if pressed.contains(&KEY_LEFTMETA) || pressed.contains(&KEY_RIGHTMETA) {
        mods |= MOD_META;
    }
    mods
}

fn handle_press(code: u16, pressed: &HashSet<u16>) {
    let Some(state) = STATE.get() else { return };
    let mods = mods_from_pressed(pressed);

    // Exact modifier match so e.g. Ctrl+Shift+Space does not fire Ctrl+Space.
    let matched: Vec<ShortcutAction> = {
        let combos = state.combos.lock().unwrap();
        combos
            .iter()
            .filter(|c| c.key == code && c.mods == mods)
            .map(|c| c.action.clone())
            .collect()
    };

    for action in matched {
        if debounce_ok(state, &action) {
            dispatch(&state.app, &action);
        }
    }
}

fn debounce_ok(state: &State, action: &ShortcutAction) -> bool {
    let key = action_key(action);
    let mut last = state.last_fire.lock().unwrap();
    let now = Instant::now();
    if let Some(prev) = last.get(&key) {
        if now.duration_since(*prev) < DEBOUNCE {
            return false;
        }
    }
    last.insert(key, now);
    true
}

fn dispatch(app: &AppHandle, action: &ShortcutAction) {
    match action {
        ShortcutAction::Record => crate::toggle_recording(app),
        ShortcutAction::CopyLast => {
            let last = app.state::<LastTranscription>();
            let text = last.text.lock().unwrap().clone();
            if let Some(t) = text {
                if !t.trim().is_empty() {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(t.clone());
                    }
                    crate::play_backend_sound(app, "done");
                    let _ = app.emit("copy-last-success", t);
                }
            }
        }
        ShortcutAction::MoveBar => {
            let next = !crate::is_recording_window_locked(app);
            let _ = crate::set_recording_window_locked(next, app.clone());
        }
    }
}

fn parse_shortcut(s: &str) -> Result<(u8, u16), String> {
    let mut mods = 0u8;
    let mut key: Option<u16> = None;

    for token in s.split('+') {
        let token = token.trim();
        if token.is_empty() {
            continue;
        }
        match token.to_ascii_lowercase().as_str() {
            // CommandOrControl/Command resolve to Control on Linux.
            "control" | "ctrl" | "commandorcontrol" | "command" | "cmd" => mods |= MOD_CTRL,
            "shift" => mods |= MOD_SHIFT,
            "alt" | "option" => mods |= MOD_ALT,
            "super" | "meta" | "win" | "windows" => mods |= MOD_META,
            _ => {
                let code = key_name_to_code(token)
                    .ok_or_else(|| format!("unsupported shortcut key '{token}'"))?;
                if key.is_some() {
                    return Err("a shortcut may contain only one non-modifier key".into());
                }
                key = Some(code);
            }
        }
    }

    match key {
        Some(code) => Ok((mods, code)),
        None => Err("shortcut has no non-modifier key".into()),
    }
}

/// Map a frontend key name (`KeyboardEvent.key` style: uppercase letters,
/// "Space", "ArrowUp", "F5", literal punctuation, …) to its evdev key code.
fn key_name_to_code(name: &str) -> Option<u16> {
    // Single-character tokens: letters, digits, punctuation.
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.clone().next()) {
        match c {
            'A'..='Z' => return Some(letter_code(c)),
            'a'..='z' => return Some(letter_code(c.to_ascii_uppercase())),
            '0'..='9' => return Some(digit_code(c)),
            '-' => return Some(12),
            '=' => return Some(13),
            ',' => return Some(51),
            '.' => return Some(52),
            '/' => return Some(53),
            ';' => return Some(39),
            '\'' => return Some(40),
            '`' => return Some(41),
            '[' => return Some(26),
            ']' => return Some(27),
            '\\' => return Some(43),
            ' ' => return Some(57),
            _ => return None,
        }
    }

    match name.to_ascii_uppercase().as_str() {
        "SPACE" => Some(57),
        "ENTER" | "RETURN" => Some(28),
        "TAB" => Some(15),
        "ESCAPE" | "ESC" => Some(1),
        "BACKSPACE" => Some(14),
        "DELETE" | "DEL" => Some(111),
        "INSERT" => Some(110),
        "HOME" => Some(102),
        "END" => Some(107),
        "PAGEUP" => Some(104),
        "PAGEDOWN" => Some(109),
        "ARROWUP" | "UP" => Some(103),
        "ARROWDOWN" | "DOWN" => Some(108),
        "ARROWLEFT" | "LEFT" => Some(105),
        "ARROWRIGHT" | "RIGHT" => Some(106),
        "CAPSLOCK" => Some(58),
        other => other
            .strip_prefix('F')
            .and_then(|n| n.parse::<u8>().ok())
            .and_then(fkey_code),
    }
}

fn letter_code(c: char) -> u16 {
    // A..Z evdev codes, indexed by letter offset.
    const LETTERS: [u16; 26] = [
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17,
        45, 21, 44,
    ];
    LETTERS[(c as u8 - b'A') as usize]
}

fn digit_code(c: char) -> u16 {
    // Top-row digits: '1'=2 … '9'=10, '0'=11.
    if c == '0' {
        11
    } else {
        2 + (c as u16 - '1' as u16)
    }
}

fn fkey_code(n: u8) -> Option<u16> {
    match n {
        1..=10 => Some(59 + (n as u16 - 1)), // F1=59 … F10=68
        11 => Some(87),
        12 => Some(88),
        _ => None,
    }
}
