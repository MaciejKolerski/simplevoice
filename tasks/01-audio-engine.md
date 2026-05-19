# Stage 1: Core Audio Engine (Rust)

**Goal:** Capture high-quality audio from the user's microphone in the exact format required by STT models (16kHz, Mono, PCM).

## Tasks:
- [ ] **Dependencies:** Add `cpal` (audio I/O), `ringbuf` (thread-safe buffering), and `hound` (optional, for saving debug `.wav` files) to `src-tauri/Cargo.toml`.
- [ ] **Audio Module:** Create `src-tauri/src/audio.rs`.
- [ ] **Device Selection:** Implement a function to list available input devices and pass them to the frontend (Settings View).
- [ ] **Capture Stream:** Implement the `cpal` input stream. Configure it strictly for **16000 Hz** and **Mono** channel.
- [ ] **State Management:** Use Tauri's managed state to hold the audio buffer and recording status.
- [ ] **Tauri Commands:** Expose `start_recording` and `stop_recording` to the React frontend.
- [ ] **Testing:** Verify that clicking "Record" on the frontend successfully fills a Rust buffer with PCM data.
