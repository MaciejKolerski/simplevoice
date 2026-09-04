//! Native Wayland text injection via the `zwp_virtual_keyboard_v1` protocol.
//!
//! Direct typing uses a generated XKB keymap, while clipboard delivery sends a
//! layout-independent Ctrl+V through a fixed map. Text characters are assigned
//! only to physical keycodes that Chromium treats as printable; Chromium-based
//! apps may otherwise reinterpret a generated character on the raw Backspace,
//! Tab, or Enter keycode and delete it, drop it, or insert a newline.
//!
//! Compositor support is identical to `wtype`: it works on wlroots-based
//! compositors (Sway/Hyprland/niri, etc.) and KWin, but NOT on GNOME/Mutter,
//! which does not implement the virtual-keyboard protocol (neither does
//! `wtype`). On an unsupported compositor this returns `Err`; the caller relies
//! on the transcription already being on the clipboard for a manual paste.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::os::fd::{AsFd, FromRawFd, IntoRawFd, OwnedFd};

use wayland_client::{
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, Proxy, QueueHandle,
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

// wl_keyboard key_state values (the virtual-keyboard protocol reuses them).
const KEY_RELEASED: u32 = 0;
const KEY_PRESSED: u32 = 1;
// wl_keyboard keymap_format: XKB v1 text format.
const KEYMAP_FORMAT_XKB_V1: u32 = 1;

// XKB keycodes are offset from the wire (evdev) keycodes by 8.
const XKB_KEYCODE_OFFSET: usize = 8;

// Printable positions from the standard evdev keyboard map. Generated Unicode
// symbols must never occupy raw Backspace (14), Tab (15), Enter (28), modifiers,
// navigation keys, or function keys: Chromium/Electron may act on that raw code
// instead of the symbol from our custom keymap. Long, diverse text is split into
// multiple keymaps when these positions are exhausted.
const SAFE_TEXT_WIRE_KEYCODES: [u32; 48] = [
    30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, // A row
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, // Q row
    44, 45, 46, 47, 48, 49, 50, 51, 52, 53, // Z row
    2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, // number row
    43, 86, // backslash positions
];

const PASTE_WIRE_KEYCODE: u32 = 47;
const CONTROL_MODIFIER_MASK: u32 = 1 << 2;

// After uploading a fresh keymap there is no protocol acknowledgement that the
// compositor has compiled it AND that the focused client has recompiled its own
// copy (the client does that asynchronously when it receives the `wl_keyboard.keymap`
// event). Key events that reach the client before that swap finishes are resolved
// against the stale keymap and silently dropped — which is exactly the "first part
// of the dictation goes missing" symptom. The race is unackable, so the only cure
// is a wall-clock settle delay before the first keystroke. 90ms is reliable on
// lightweight wlroots compositors (Sway/Hyprland/niri) and the heavier KWin alike;
// raise it first if leading characters still vanish on a slow compositor.
const KEYMAP_SETTLE_MS: u64 = 90;

// Don't ship every keystroke in one buffered burst at the final roundtrip: flush
// in small batches with a short pause so the compositor drains them steadily and
// no key is lost to input coalescing on slower compositors.
const FLUSH_EVERY_N_KEYS: usize = 16;
const KEY_BATCH_PAUSE_MS: u64 = 2;

/// Type `text` into the focused Wayland surface using a virtual keyboard.
///
/// Returns `Err` if no Wayland connection is available or the compositor lacks
/// the virtual-keyboard protocol.
pub fn type_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }

    let conn = Connection::connect_to_env()
        .map_err(|e| format!("failed to connect to Wayland display: {e}"))?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();

    // Bind the registry and round-trip once so the globals (seat + manager) arrive.
    let _registry = conn.display().get_registry(&qh, ());
    let mut state = State::default();
    queue
        .roundtrip(&mut state)
        .map_err(|e| format!("Wayland roundtrip failed: {e}"))?;

    let seat = state
        .seat
        .clone()
        .ok_or_else(|| "no wl_seat advertised by the compositor".to_string())?;
    let manager = state.manager.clone().ok_or_else(|| {
        "compositor does not support zwp_virtual_keyboard_manager_v1 \
         (unsupported on GNOME/Mutter)"
            .to_string()
    })?;

    let keyboard = manager.create_virtual_keyboard(&seat, &qh, ());
    let mut time: u32 = 0;
    for chunk in split_text_for_keymaps(text) {
        let (keymap, keycode_of) = build_keymap(&chunk)?;
        upload_keymap(&keyboard, &mut queue, &mut state, &keymap)?;

        for (i, ch) in chunk.chars().enumerate() {
            let wire_keycode = keycode_of[&ch];
            keyboard.key(time, wire_keycode, KEY_PRESSED);
            time = time.wrapping_add(1);
            keyboard.key(time, wire_keycode, KEY_RELEASED);
            time = time.wrapping_add(1);

            if (i + 1) % FLUSH_EVERY_N_KEYS == 0 {
                conn.flush()
                    .map_err(|e| format!("Wayland flush (keys) failed: {e}"))?;
                std::thread::sleep(std::time::Duration::from_millis(KEY_BATCH_PAUSE_MS));
            }
        }

        queue
            .roundtrip(&mut state)
            .map_err(|e| format!("Wayland roundtrip (keys) failed: {e}"))?;
    }

    keyboard.destroy();
    queue
        .roundtrip(&mut state)
        .map_err(|e| format!("Wayland roundtrip (destroy) failed: {e}"))?;

    Ok(())
}

/// Paste the current clipboard contents into the focused Wayland surface.
/// The V key uses its standard physical code, so both the generated keymap and
/// any raw-code handling in the target application agree on Ctrl+V.
pub fn paste() -> Result<(), String> {
    let conn = Connection::connect_to_env()
        .map_err(|e| format!("failed to connect to Wayland display: {e}"))?;
    let mut queue = conn.new_event_queue();
    let qh = queue.handle();

    let _registry = conn.display().get_registry(&qh, ());
    let mut state = State::default();
    queue
        .roundtrip(&mut state)
        .map_err(|e| format!("Wayland roundtrip failed: {e}"))?;

    let seat = state
        .seat
        .clone()
        .ok_or_else(|| "no wl_seat advertised by the compositor".to_string())?;
    let manager = state.manager.clone().ok_or_else(|| {
        "compositor does not support zwp_virtual_keyboard_manager_v1 \
         (unsupported on GNOME/Mutter)"
            .to_string()
    })?;
    let keyboard = manager.create_virtual_keyboard(&seat, &qh, ());

    upload_keymap(&keyboard, &mut queue, &mut state, &build_paste_keymap())?;
    keyboard.modifiers(CONTROL_MODIFIER_MASK, 0, 0, 0);
    keyboard.key(0, PASTE_WIRE_KEYCODE, KEY_PRESSED);
    keyboard.key(1, PASTE_WIRE_KEYCODE, KEY_RELEASED);
    keyboard.modifiers(0, 0, 0, 0);
    keyboard.destroy();
    queue
        .roundtrip(&mut state)
        .map_err(|e| format!("Wayland roundtrip (paste) failed: {e}"))?;

    Ok(())
}

/// Map the Unicode scalar to the XKB symbol token used in the keymap. XKB parses
/// the `U<hex>` notation into the canonical keysym, so this works for arbitrary
/// characters; newlines/tabs are mapped to their functional keysyms so they
/// behave as Enter/Tab rather than literal control characters.
fn keysym_token(ch: char) -> String {
    match ch {
        '\n' | '\r' => "Return".to_string(),
        '\t' => "Tab".to_string(),
        _ => format!("U{:04X}", ch as u32),
    }
}

fn fixed_wire_keycode(ch: char) -> Option<u32> {
    match ch {
        '\t' => Some(15),
        '\n' | '\r' => Some(28),
        ' ' => Some(57),
        _ => None,
    }
}

fn split_text_for_keymaps(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut chunk = String::new();
    let mut unique = HashSet::new();

    for ch in text.chars() {
        let needs_slot = fixed_wire_keycode(ch).is_none() && !unique.contains(&ch);
        if needs_slot && unique.len() == SAFE_TEXT_WIRE_KEYCODES.len() {
            chunks.push(std::mem::take(&mut chunk));
            unique.clear();
        }
        chunk.push(ch);
        if fixed_wire_keycode(ch).is_none() {
            unique.insert(ch);
        }
    }

    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    chunks
}

/// Build an XKB keymap plus a character-to-wire-keycode lookup. Whitespace uses
/// its matching physical key; every other symbol occupies a printable raw code.
fn build_keymap(text: &str) -> Result<(String, HashMap<char, u32>), String> {
    let mut keycode_of = HashMap::new();
    let mut symbols_by_wire = BTreeMap::new();
    let mut next_safe = 0;

    for ch in text.chars() {
        if keycode_of.contains_key(&ch) {
            continue;
        }
        let wire_keycode = if let Some(keycode) = fixed_wire_keycode(ch) {
            keycode
        } else {
            let keycode = SAFE_TEXT_WIRE_KEYCODES
                .get(next_safe)
                .copied()
                .ok_or_else(|| {
                    format!(
                        "more than {} distinct printable characters in one keymap",
                        SAFE_TEXT_WIRE_KEYCODES.len()
                    )
                })?;
            next_safe += 1;
            keycode
        };

        keycode_of.insert(ch, wire_keycode);
        symbols_by_wire
            .entry(wire_keycode)
            .or_insert_with(|| keysym_token(ch));
    }

    let mut codes = String::new();
    let mut symbols = String::new();
    for (&wire_keycode, token) in &symbols_by_wire {
        let xkb_keycode = wire_keycode as usize + XKB_KEYCODE_OFFSET;
        codes.push_str(&format!("        <K{wire_keycode}> = {xkb_keycode};\n"));
        symbols.push_str(&format!(
            "        key <K{wire_keycode}> {{ [ {token} ] }};\n"
        ));
    }
    let maximum = symbols_by_wire
        .last_key_value()
        .map(|(&wire_keycode, _)| wire_keycode as usize + XKB_KEYCODE_OFFSET)
        .unwrap_or(XKB_KEYCODE_OFFSET);

    let keymap = format!(
        "xkb_keymap {{\n\
         xkb_keycodes \"(unnamed)\" {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20minimum = 8;\n\
         \x20\x20\x20\x20\x20\x20\x20\x20maximum = {maximum};\n\
         {codes}    }};\n\
         xkb_types \"(unnamed)\" {{ include \"complete\" }};\n\
         xkb_compat \"(unnamed)\" {{ include \"complete\" }};\n\
         xkb_symbols \"(unnamed)\" {{\n\
         {symbols}    }};\n\
         }};\n\0"
    );

    Ok((keymap, keycode_of))
}

fn build_paste_keymap() -> String {
    let xkb_keycode = PASTE_WIRE_KEYCODE as usize + XKB_KEYCODE_OFFSET;
    format!(
        "xkb_keymap {{\n\
         xkb_keycodes \"(unnamed)\" {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20minimum = 8;\n\
         \x20\x20\x20\x20\x20\x20\x20\x20maximum = {xkb_keycode};\n\
         \x20\x20\x20\x20\x20\x20\x20\x20<K{PASTE_WIRE_KEYCODE}> = {xkb_keycode};\n\
         \x20\x20\x20\x20}};\n\
         xkb_types \"(unnamed)\" {{ include \"complete\" }};\n\
         xkb_compat \"(unnamed)\" {{ include \"complete\" }};\n\
         xkb_symbols \"(unnamed)\" {{\n\
         \x20\x20\x20\x20\x20\x20\x20\x20key <K{PASTE_WIRE_KEYCODE}> {{ [ U0076 ] }};\n\
         \x20\x20\x20\x20}};\n\
         }};\n\0"
    )
}

fn upload_keymap(
    keyboard: &ZwpVirtualKeyboardV1,
    queue: &mut wayland_client::EventQueue<State>,
    state: &mut State,
    keymap: &str,
) -> Result<(), String> {
    let fd = keymap_memfd(keymap)?;
    keyboard.keymap(KEYMAP_FORMAT_XKB_V1, fd.as_fd(), keymap.len() as u32);
    keyboard.modifiers(0, 0, 0, 0);
    queue
        .roundtrip(state)
        .map_err(|e| format!("Wayland roundtrip (keymap) failed: {e}"))?;
    std::thread::sleep(std::time::Duration::from_millis(KEYMAP_SETTLE_MS));
    Ok(())
}

/// Write the keymap into an anonymous in-memory file and return its descriptor.
/// The compositor mmaps this fd to read the keymap.
fn keymap_memfd(keymap: &str) -> Result<OwnedFd, String> {
    let name = c"simplevoice-keymap";
    let raw = unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC) };
    if raw < 0 {
        return Err(format!(
            "memfd_create failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // Take ownership immediately so the fd is closed on any early return.
    let mut file = unsafe { std::fs::File::from_raw_fd(raw) };
    file.write_all(keymap.as_bytes())
        .map_err(|e| format!("failed to write keymap: {e}"))?;
    file.flush()
        .map_err(|e| format!("failed to flush keymap: {e}"))?;
    // The compositor mmaps from offset 0, so the file position is irrelevant.
    Ok(unsafe { OwnedFd::from_raw_fd(file.into_raw_fd()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chromium_regression_never_assigns_text_to_backspace_tab_or_enter() {
        let (_, backspace_case) = build_keymap("abcdefghijklm?").unwrap();
        let text = "abcdefghijklmnopqrstuvwxyz?ó łśćąęźżń";
        let (_, keycode_of) = build_keymap(text).unwrap();

        for ch in text.chars().filter(|ch| fixed_wire_keycode(*ch).is_none()) {
            assert!(SAFE_TEXT_WIRE_KEYCODES.contains(&keycode_of[&ch]));
        }
        assert_ne!(backspace_case[&'?'], 14);
        assert_ne!(keycode_of[&'ó'], 28);
        assert_eq!(keycode_of[&'ó'], 47);
    }

    #[test]
    fn preserves_whitespace_on_matching_physical_keys() {
        let (_, keycode_of) = build_keymap("a b\tc\nd\r").unwrap();

        assert_eq!(keycode_of[&' '], 57);
        assert_eq!(keycode_of[&'\t'], 15);
        assert_eq!(keycode_of[&'\n'], 28);
        assert_eq!(keycode_of[&'\r'], 28);
    }

    #[test]
    fn splits_diverse_unicode_without_changing_the_text() {
        let text: String = (0..SAFE_TEXT_WIRE_KEYCODES.len() + 1)
            .map(|offset| char::from_u32(0x1000 + offset as u32).unwrap())
            .collect();
        let chunks = split_text_for_keymaps(&text);

        assert_eq!(chunks.concat(), text);
        assert_eq!(chunks.len(), 2);
        for chunk in chunks {
            let unique: HashSet<_> = chunk.chars().collect();
            assert!(unique.len() <= SAFE_TEXT_WIRE_KEYCODES.len());
            build_keymap(&chunk).unwrap();
        }
    }

    #[test]
    fn paste_map_uses_the_standard_physical_v_key() {
        let keymap = build_paste_keymap();
        assert!(keymap.contains("<K47> = 55;"));
        assert!(keymap.contains("key <K47> { [ U0076 ] };"));
    }
}

/// Wayland globals we care about, collected during the registry roundtrip.
#[derive(Default)]
struct State {
    seat: Option<wl_seat::WlSeat>,
    manager: Option<ZwpVirtualKeyboardManagerV1>,
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == wl_seat::WlSeat::interface().name {
                let v = version.min(7);
                state.seat = Some(registry.bind::<wl_seat::WlSeat, _, _>(name, v, qh, ()));
            } else if interface == ZwpVirtualKeyboardManagerV1::interface().name {
                state.manager =
                    Some(registry.bind::<ZwpVirtualKeyboardManagerV1, _, _>(name, 1, qh, ()));
            }
        }
    }
}

// These interfaces deliver no events we need to act on.
impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpVirtualKeyboardManagerV1,
        _: <ZwpVirtualKeyboardManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpVirtualKeyboardV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpVirtualKeyboardV1,
        _: <ZwpVirtualKeyboardV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
