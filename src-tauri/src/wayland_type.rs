//! Native Wayland text injection via the `zwp_virtual_keyboard_v1` protocol.
//!
//! This replaces shelling out to the external `wtype` binary. It performs the
//! exact same trick `wtype` does: build an XKB keymap on the fly that maps each
//! unique character in the text to its own keycode (so the literal Unicode
//! keysym is produced regardless of the user's real keyboard layout), upload it
//! to the compositor through an in-memory file, then emit press/release events
//! per character.
//!
//! Compositor support is identical to `wtype`: it works on wlroots-based
//! compositors (Sway/Hyprland/niri, etc.) and KWin, but NOT on GNOME/Mutter,
//! which does not implement the virtual-keyboard protocol (neither does
//! `wtype`). On an unsupported compositor this returns `Err`; the caller relies
//! on the transcription already being on the clipboard for a manual paste.

use std::collections::HashMap;
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

// XKB keycodes are offset from the wire (evdev) keycodes by 8. We assign wire
// keycode `index + 1` to each unique character, i.e. XKB keycode `index + 9`.
const XKB_KEYCODE_OFFSET: usize = 8;

// Upper bound on distinct characters we can inject in a single keymap. XKB/evdev
// keycodes top out around 255; staying well under that keeps us safe and lets the
// caller fall back (e.g. to clipboard paste) for pathological inputs.
const MAX_UNIQUE_CHARS: usize = 240;

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
/// Returns `Err` if no Wayland connection is available, the compositor lacks the
/// virtual-keyboard protocol, or the text has too many distinct characters to
/// map into a single keymap.
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

    // Build and upload the per-character keymap.
    let (keymap, index_of) = build_keymap(text)?;
    let fd = keymap_memfd(&keymap)?;
    keyboard.keymap(KEYMAP_FORMAT_XKB_V1, fd.as_fd(), keymap.len() as u32);
    // Start from a clean modifier state (no stuck Shift/Ctrl/etc.).
    keyboard.modifiers(0, 0, 0, 0);
    queue
        .roundtrip(&mut state)
        .map_err(|e| format!("Wayland roundtrip (keymap) failed: {e}"))?;

    // The roundtrip above only proves the *compositor* received and parsed the
    // keymap fd — not that the focused client has recompiled and applied it. That
    // propagation is asynchronous and unacked, so wait it out before typing;
    // otherwise the leading characters land under the old keymap and disappear.
    std::thread::sleep(std::time::Duration::from_millis(KEYMAP_SETTLE_MS));

    // Emit a press/release pair per character. `time` is just a monotonic stamp.
    let mut time: u32 = 0;
    for (i, ch) in text.chars().enumerate() {
        let wire_keycode = (index_of[&ch] + 1) as u32;
        keyboard.key(time, wire_keycode, KEY_PRESSED);
        time = time.wrapping_add(1);
        keyboard.key(time, wire_keycode, KEY_RELEASED);
        time = time.wrapping_add(1);

        // Flush in small batches rather than letting every request pile up until
        // the final roundtrip, so the compositor processes the stream steadily and
        // none of the keys are coalesced away on a busy/slow compositor.
        if (i + 1) % FLUSH_EVERY_N_KEYS == 0 {
            conn.flush()
                .map_err(|e| format!("Wayland flush (keys) failed: {e}"))?;
            std::thread::sleep(std::time::Duration::from_millis(KEY_BATCH_PAUSE_MS));
        }
    }

    keyboard.destroy();
    // Final roundtrip flushes any remaining requests and lets the compositor
    // process them before `fd`/`conn` are dropped.
    queue
        .roundtrip(&mut state)
        .map_err(|e| format!("Wayland roundtrip (flush) failed: {e}"))?;

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

/// Build the XKB keymap text plus a char→index lookup. Characters are de-duped
/// in first-seen order; each gets a dedicated keycode whose only symbol is that
/// character, so pressing it always yields the intended glyph.
fn build_keymap(text: &str) -> Result<(String, HashMap<char, usize>), String> {
    let mut order: Vec<char> = Vec::new();
    let mut index_of: HashMap<char, usize> = HashMap::new();
    for ch in text.chars() {
        if !index_of.contains_key(&ch) {
            index_of.insert(ch, order.len());
            order.push(ch);
        }
    }

    if order.len() > MAX_UNIQUE_CHARS {
        return Err(format!(
            "{} distinct characters exceeds the {} keycode limit",
            order.len(),
            MAX_UNIQUE_CHARS
        ));
    }

    let mut codes = String::new();
    let mut symbols = String::new();
    for (i, &ch) in order.iter().enumerate() {
        let name = i + 1; // <K1>, <K2>, ...
        let xkb_keycode = i + 1 + XKB_KEYCODE_OFFSET;
        codes.push_str(&format!("        <K{name}> = {xkb_keycode};\n"));
        symbols.push_str(&format!(
            "        key <K{name}> {{ [ {} ] }};\n",
            keysym_token(ch)
        ));
    }
    let maximum = order.len() + XKB_KEYCODE_OFFSET;

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

    Ok((keymap, index_of))
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
