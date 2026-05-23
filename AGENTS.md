# AGENTS.md

## Commands

- Use `pnpm` exclusively (pnpm-lock.yaml)
- `pnpm tauri dev` — full dev workflow (starts Vite + Tauri)
- `pnpm build` — TypeScript build before `pnpm tauri build`
- `pnpm lint` — runs `tsc --noEmit --strict` (only type checking, no ESLint)

## Architecture

- Tauri 2 + React 19/TypeScript frontend (`src/`, Vite on port 1420)
- All core logic lives in `src-tauri/src/lib.rs` (1500+ LOC: commands, state, DB, shortcuts, tray)
- STT backends in `src-tauri/src/stt/` (whisper-rs, sherpa-onnx, parakeet, cloud)
- SQLite via `tauri-plugin-sql` + `sqlx` (`src-tauri/migrations/01_init.sql`)
- Native audio (`cpal`), media keys, global shortcuts, autostart, clipboard simulation

## Key Gotchas

- `src-tauri/src/main.rs` only calls `simplevoice_app_lib::run()`
- `whisper-rs` must have `metal` feature **only on macOS** (see Cargo.toml). On Linux it fails with "Foundation" framework error.
- Sound feedback on Linux uses `pw-play` (PipeWire). 
- Audio playback in TranscriptionsView (<audio> element) requires `gst-plugins-good` on Linux (`sudo pacman -S gst-plugins-good`) to avoid "autoaudiosink not found" error.
- Frontend ignores `**/src-tauri/**` in Vite watch
- macOS-specific code for accessibility and media remote
- Rust rebuilds are slow due to heavy native deps (ort, sherpa-onnx, whisper-rs, cpal, etc.)
- `LastTranscription` state and global shortcut registry live in `lib.rs`

## Verification

Run `pnpm lint` after frontend changes. Use rust-analyzer for backend.

**Preserve this file.** It contains the only non-obvious workflow details.
