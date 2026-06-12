# SIMPLEVOICE.md

SimpleVoice loads `SIMPLEVOICE.md` from the workspace root as agent memory (similar to AGENTS.md / CLAUDE.md). This file is also the project's living architecture doc — read it before making changes.

## Project

**SimpleVoice** — privacy-first, fully local/offline Speech-to-Text desktop assistant. Tauri 2 + Rust backend, React 19 + TypeScript + Tailwind frontend. Records system audio (CPAL + VAD), runs multiple local ASR engines, auto-pastes or copies transcription.

- Bundle id: `com.woro.simplevoice`
- Package manager: **pnpm**
- Platforms: macOS, Linux, Windows
- Frontend check: `pnpm lint`
- Always use `pnpm tauri ...` (see scripts/tauri.js for Windows Cargo path and CRT fixes)

## Quality bar

Production-grade or it does not ship. Every change is judged against all of these:

- **Correctness**: model loading guards, VAD edge cases, concurrent shortcut handling, SQLite transaction safety, platform-specific audio/session cleanup.
- **Performance**: Rust `dev` profile uses `opt-level=1` for crate but `opt-level=3` for all dependencies (critical for Whisper.cpp and Candle). Avoid unnecessary allocations in audio hot path.
- **Security**: API keys live only in OS keyring (`keyring` crate). Never write keys to disk or `localStorage`. Validate paths in `open_folder`. No secret leakage in logs.
- **UI/UX**: Recording window must feel native (macOS NSPanel hacks). Auto-paste must be reliable across macOS accessibility, Linux wtype, and fallback simulators. Tray menu must reflect real state.
- **Architecture**: STT logic lives in `src-tauri/src/stt/` with trait-based factory. `lib.rs` is the imperative shell (commands, state, tray, shortcuts). Keep platform-specific code under `#[cfg(target_os = "...")]`.

Run `pnpm lint` before every commit. For core changes (audio, STT, shortcuts, recording window) verify with `pnpm tauri dev`.

## Conventions

- **Comments**: default to none. Code must explain itself. If needed, explain *why* in 1-2 lines.
- **No emojis** in code, comments, or docs.
- **pnpm only** — never npm, yarn, or direct `@tauri-apps/cli`.
- **Rust**: prefer existing patterns from `stt/factory.rs` and `audio.rs`. New engines must implement the `AsrEngine` trait.
- **Frontend**: React 19, Tailwind v4 (via `@tailwindcss/vite`), Lucide icons. Use existing context (`ConfigContext`) for settings.

## Architecture

### Backend (`src-tauri/src/`)

`lib.rs` is the main file. It registers all Tauri commands, manages global state (`AudioController`, `SttController`, `AppConfig`, `LastTranscription`, `ShortcutRegistry`), builds the tray menu, and handles global shortcuts.

Key modules:
- `audio.rs` — CPAL system audio capture + Voice Activity Detection (VAD). Emits events on start/stop.
- `stt/` — engine factory + implementations:
  - Whisper.cpp (ggml/gguf) via `whisper-rs` (Metal on macOS, Vulkan on Linux)
  - Candle (Whisper + Wav2Vec)
  - ONNX (Sherpa-onnx / Parakeet)
  - Nemo
- `stt/factory.rs` — `AsrFactory::load()` and `detect()`. Must call `load_model()` before local transcription. Contains guard against React StrictMode double-mount.
- `stt/chunker.rs` — silence-aware splitting of long recordings into 45-90 s chunks; `SttController::transcribe_with_progress` transcribes them sequentially (any engine, cloud included) and emits `transcription-progress`; recording auto-stops at the 90-minute safety cap (`audio.rs::RECORDING_MAX_SECS`).
- Platform specifics: macOS NSPanel + accessibility for recording window and auto-paste; Linux native shortcuts + `wtype`; Windows tray and enigo fallback.

**Critical rules**:
- Local recording is blocked until a model is loaded (`is_recording_allowed`).
- `load_model()` uses `spawn_blocking` + panic catching for GPU fallback.
- Sounds (`start.wav`, `stop.wav`, `done.wav`) are bundled via `tauri.conf.json` resources and fall back to system sounds (`afplay` / `pw-play` / rodio).
- Config (`config.json`) and recordings live in `app_local_data_dir()`. API keys exclusively in keyring under `simplevoice`.

### Frontend (`src/`)

- React 19 + Vite + Tailwind v4.
- Main views: `UsageView`, `ModelsView`, `TranscriptionsView`, `SettingsView`, `RecordingWindowView`.
- `ConfigContext` + Tauri commands for settings persistence.
- Global shortcuts registered from settings.
- Auto-paste uses platform-specific logic (accessibility on macOS).

### Recording Window

Special macOS-only behavior in `lib.rs` (`update_recording_window_visibility`, `object_setClass` to turn window into `NSPanel`, custom positioning, `recording_window_mode` in config: `always` | `recording` | `never`).

Linux/Windows use standard Tauri window.

Bar position: `data-tauri-drag-region` on the pill; macOS Cmd-hold toggles click-through (polling thread, also emits `recording-window-lock-status` scoped to the overlay for the amber glow), Linux/Windows use the lock toggle (tray + Settings). `reset_recording_window_position` restores the default top-center placement and clears `recording_window_has_custom_pos`. Settings surfaces all of this in the Recording & Feedback section.

### Linux-specific

- Global shortcuts implemented via native desktop integration + CLI flags `--toggle` and `--copy-last` on the built binary.
- Auto-paste strongly prefers `wtype` on Wayland.

## Key Gotchas

- **Rust dev profile**: `opt-level=1` (crate) + `opt-level=3` (dependencies) in `Cargo.toml` — do not change without performance testing.
- **Model loading guard**: `load_model` skips duplicate calls from React StrictMode. Respect the `loading_model_path` check.
- **Tray menu**: rebuilt on every state change (`rebuild_tray_menu`). Uses custom status dot drawing. Must reflect recording/transcribing/saving state.
- **macOS recording window**: converts to `NSPanel`, sets high level, ignores cursor events. Position saved in `config.json`. First show does dynamic bottom-center placement.
- **Sounds**: `resolve_sound_file` checks bundled resources first, then falls back to system sounds. `sound_feedback_enabled` and `pause_audio_on_record` read from `config.json`.
- **Global shortcuts on Linux**: handled in `linux_shortcuts.rs` + desktop config (example in README). Do not rely only on Tauri plugin.
- **VAD + auto-stop**: recording stops automatically on silence. Test edge cases (short utterances, background noise).
- **StrictMode in dev**: expect double `useEffect` calls for model loading and recording window init.

## Verification Order

1. `pnpm lint`
2. `pnpm tauri dev`

This file contains only verified, non-obvious facts that prevent common agent mistakes. Read it before every significant change.
