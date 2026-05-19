# Stage 3: STT Engines & Model Management

**Goal:** Implement the transcription logic using local quantized models.

## Tasks:
- [ ] **Dependencies:** Add `whisper-rs` (Whisper bindings).
- [ ] **Engine Trait:** Create `src-tauri/src/stt/mod.rs` and define an `EngineAdapter` trait (Initialize, Transcribe, Shutdown).
- [ ] **Whisper Implementation:** Implement the trait using `whisper-rs`. Ensure parameters are optimized (Temperature 0.0, specific initial prompt for code).
- [ ] **Model Scanner:** Create a Rust function that scans `$APP_DATA/models` for `.gguf` files. Parse file sizes and generate fake/real metrics (Quality/Speed) for the frontend Models View.
- [ ] **Transcription Command:** Expose a Tauri command `transcribe_audio` that takes the buffered PCM data, runs inference on a background thread (`tokio::spawn`), and returns the text.
- [ ] **Frontend Wiring:** Connect the Rust backend to the frontend UI states (Processing... -> Success).
