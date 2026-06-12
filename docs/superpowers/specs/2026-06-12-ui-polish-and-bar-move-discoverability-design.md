# UI Polish and Recording-Bar Move Discoverability — Design

Date: 2026-06-12
Status: approved

## Goal

Two-part improvement of the SimpleVoice desktop UI:

1. Make moving the recording bar (the wavebar overlay window) discoverable and consistently controllable on every platform.
2. A full polish pass over the existing UI: consistency, interaction states, responsiveness at the minimum window size, micro-interactions, empty states, and i18n coverage. No redesign — the current visual language stays.

## Background (verified in code)

Moving the bar already works on all platforms, but is undiscoverable:

- The overlay pill has `data-tauri-drag-region` and renders an amber border glow when movable (`src/views/RecordingWindowView.tsx:308-311`).
- Position persists automatically: `WindowEvent::Moved` saves coordinates for the `recording_window` label (`src-tauri/src/lib.rs:2793-2796`), stored as `recording_window_x/y` + `recording_window_has_custom_pos` in `config.json` (`save_recording_window_position`, `lib.rs:425`).
- macOS: a background thread polls the ⌘ key every 150 ms; while held, the window stops ignoring cursor events so it can be dragged; on release the position is saved (`lib.rs:2824-2873`). Nothing in the UI mentions this, and the amber glow is never triggered on macOS (the thread does not emit any event).
- Linux/Windows: the tray menu has a "Unlock window" item (`toggle_recording_window_lock`, cfg-gated at `lib.rs:1020-1032`) backed by the `set_recording_window_locked` command (`lib.rs:2082-2115`), which persists `recording_window_locked`, toggles click-through, emits `recording-window-lock-status`, and rebuilds the tray. Default is locked. Nothing outside the tray mentions this.
- SettingsView has no control or text about bar position at all.

## Part 1 — Bar-move discoverability

### Settings UI (recording-window section of `SettingsView`)

New "Bar position" group inside the existing recording-window settings section:

- **macOS:** a static hint row — "Hold ⌘ and drag the bar to move it" — with ⌘ rendered as a keycap badge using the same visual pattern as the existing shortcut keycaps in SettingsView. No new toggle on macOS (the ⌘ mechanism stays the only one).
- **Linux/Windows:** a Switch "Unlock bar position" that invokes the existing `set_recording_window_locked` command, plus help text: "When unlocked, drag the bar to move it. The amber glow means it can be moved." The switch state stays in sync with the tray item because the command already emits `recording-window-lock-status` and rebuilds the tray; SettingsView subscribes to that event and reads the initial state via `is_recording_window_locked_cmd`.
- **All platforms:** a "Reset position" button.

Platform gating follows the existing pattern used by the macOS-only permissions section.

### Backend changes (Rust, `lib.rs`)

- **New command `reset_recording_window_position`:** sets `recording_window_has_custom_pos` to `false` in `config.json` (under `CONFIG_FILE_LOCK`), then re-runs the default-placement logic (same math as the first-show branch of `update_recording_window_visibility`) and applies it to the window if it exists. Registered for all platforms.
- **macOS ⌘ feedback:** the existing ⌘-polling thread additionally emits `recording-window-lock-status` (movable = ⌘ held → emit `locked=false`; released → `locked=true`) so the overlay pill glows amber exactly while it can be dragged. The overlay already listens to this event and renders the glow; no overlay changes needed beyond what exists.
  - Note: this reuses the lock-status event purely as a visual signal on macOS; it does not write `recording_window_locked` to config there.

### i18n

All new strings go through i18next with keys in `en.json`, `pl.json`, `de.json`. The hint must render the platform-appropriate wording; it never shows macOS text on Linux/Windows or vice versa.

## Part 2 — UI polish pass

Scope: all main views (`UsageView`, `ModelsView`, `TranscriptionsView`, `SettingsView`), layout components (`Sidebar`, `TitleBar`), and the shared `components/ui` primitives. The themes below come from a code audit; exact file:line findings are re-verified during implementation (the audit list is the input, not gospel).

1. **Shared `SettingRow` component.** The `flex justify-between items-center gap-6 p-5 border-b ...` row pattern is duplicated ~11× across SettingsView and ModelsView. Extract one component with label / description / control slots and use it everywhere the pattern occurs. This is the lever that makes the rest of the consistency work stick.
2. **Design-token consistency.** Replace ad-hoc colors with the tokens defined in `App.css` (`--surface*`, `--border`, etc.): unify select backgrounds (`bg-black` vs `bg-secondary`), ad-hoc `white/NN` borders and fills where a token exists. Normalize the border-radius scale: `rounded-md` for small interactive elements, `rounded-xl` for cards/sections, `rounded-2xl` only for floating surfaces (HUD, overlay panels). One consistent help-text size instead of the current 11px/13px/`text-xs` mix.
3. **Interaction states.** `focus-visible` rings on every interactive element (the inline shortcut-capture buttons in SettingsView currently have none); tooltips on all icon-only buttons (delete-model button has `title` only); keyboard activation (Enter/Space) for the expandable transcription rows; fix the disabled live-transcription section so nested controls are truly unfocusable (currently `pointer-events-none` blocks clicks but not keyboard focus).
4. **Responsiveness at the 800×600 minimum window.** SettingsView columns get a usable layout below `lg`; fixed `w-72` inputs in ModelsView provider rows become flexible (`max-w` + wrap) so labels don't collide at 800 px; UsageView stat-card grid checked at the same width.
5. **Micro-interactions.** One animation system: keep the existing `fadeIn` keyframes from `App.css` as the standard for view/panel entrances, replace stray `animate-in` utility usages; consistent `transition-colors` on hoverable controls.
6. **Empty and error states.** Empty state for the UsageView chart when the selected range has no data; an empty-state card for the cloud models list (matching the local-models empty state); consistent error presentation between inline alerts and dialogs.
7. **i18n coverage.** Route hardcoded user-facing strings through `t()` (e.g. provider labels) — technical model identifiers ("Whisper Tiny (GGML)" etc.) stay as-is. `pnpm check:i18n` must pass.
8. **README screenshot sync.** `assets/screenshot-*.svg` are hand-built SVG traces of UsageView and RecordingWindowView. If either view's visuals change in this work, the SVGs are re-synced 1:1 in the same change.

## Out of scope

- Light theme.
- Navigation/layout redesign (sidebar + 4 views stays).
- New views or features beyond the bar-position group.
- Implementing a settings-side lock toggle on macOS.

## Error handling

- `reset_recording_window_position` returns `Result<(), String>`; the Settings button surfaces failure via the existing toast (sonner) pattern.
- The macOS ⌘ emit is best-effort (ignore emit errors, as the surrounding thread already does for window calls).
- Lock-state reads keep their current conservative fallback (default to locked on any config read error).

## Testing and verification

- `pnpm lint` (tsc strict) and `pnpm check:i18n` after every task.
- `pnpm tauri dev` on macOS: verify the hint renders, ⌘ glow appears while held, reset re-centers the bar, position persists.
- Resize the main window to 800×600 and walk all four views.
- Linux/Windows paths are code-reviewed against the existing tray flow (no hardware at hand); the settings switch uses the exact same command the tray uses.
- README SVGs re-checked against the final UI.

## Decisions log

- Hint lives in Settings only (no overlay tooltip, no onboarding step) — user's choice.
- Full platform support chosen; discovered Linux/Windows drag already exists, so the work there is surfacing it in Settings, not building it.
- "Hint + consistent control" approach approved: settings hint (macOS) + settings switch mirroring tray (Linux/Windows) + reset button (all) + amber ⌘ feedback (macOS).
