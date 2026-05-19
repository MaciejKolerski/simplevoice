# SimpleVoice - Technical Implementation Plan

## 1. Project Overview
SimpleVoice is a lightweight, cross-platform (ARM-optimized) Speech-to-Text application designed for "vibecoders". It prioritizes local execution using quantized Whisper models while providing an optional cloud fallback (BYOK).

## 2. Tech Stack
- **Framework:** [Tauri](https://tauri.app/) (v2 preferred for improved mobile/cross-platform support).
- **Frontend:** [React](https://react.dev/) + [Vite](https://vitejs.dev/).
- **Styling:** [TailwindCSS](https://tailwindcss.com/).
- **Backend (Core):** Rust.
- **Local STT Engine:** [whisper-rs](https://github.com/tazz4843/whisper-rs) (Rust bindings for `whisper.cpp`).
- **Audio I/O:** [cpal](https://github.com/RustAudio/cpal).
- **Automation:** [enigo](https://github.com/enigo-rs/enigo) (for keyboard simulation/text injection).

## 3. System Architecture

### 3.1. Audio Pipeline (Rust)
To ensure minimal latency and high fidelity, audio capture must happen in the Rust layer, not the WebView.
- **Capture:** Use `cpal` to initialize an input stream (Mono, 16kHz - required by Whisper).
- **Pre-processing:** Implement DC offset removal and peak normalization to -1dB FS.
- **VAD (Voice Activity Detection):** **Required.** Use a lightweight VAD (e.g., `webrtc-vad` or a simple energy-based silero-lite) to prevent processing silence, which causes Whisper hallucinations.

### 3.2. Modular STT Engine Architecture
To support diverse models like **Whisper** and **Nvidia Parakeet**, we will implement a **Trait-based Adapter Pattern** in Rust.

- **STT Engine Trait:** Defines a standard interface for `initialize()`, `transcribe(pcm_data)`, and `shutdown()`.
- **Implementations:**
    - **Whisper Engine:** Uses `whisper-rs` (GGUF models). Optimized for Apple Silicon via CoreML.
    - **ONNX Engine (for Parakeet):** Uses `ort` (Rust bindings for ONNX Runtime). This allows running Nvidia Parakeet (exported to ONNX) on ARM CPUs using Accelerate/NNAPI providers.
    - **Cloud Engine (BYOK):** REST-based adapter for OpenAI, Groq, or Anthropic.

### 3.3. Model Management & Switching
- **Dynamic Loading:** Engines are initialized lazily. Switching models in the UI triggers a `shutdown()` of the current engine and `initialize()` of the new one to save RAM.
- **Model Registry:** A JSON manifest file (`models.json`) that maps model names to their respective engine types, download URLs, and required parameters (e.g., sample rate, initial prompts).

### 3.3. BYOK (Bring Your Own Key) Strategy
- **Security:** Use system-native secret storage (Keychain/Secret Service) via `keyring-rs`.
- **API Integration:** Rust-side HTTP client (`reqwest`) to hit OpenAI/Groq endpoints. This keeps the API key out of the frontend memory space as much as possible.

### 3.4. Data Persistence & Analytics
To support the **Usage** and **History** views:
- **Database:** Use `tauri-plugin-sql` (SQLite) to store:
    - `transcriptions`: id, timestamp, model_id, duration, text, audio_path (optional).
    - `daily_stats`: aggregated word counts and transcription time for chart rendering.
- **Charts:** Frontend will use a lightweight charting library (or custom SVG components as seen in design) to render the "Activity Details" bar chart.

### 3.6. View-Specific Requirements (Frontend)
The application will consist of four primary views, as defined in the UI design:

1.  **Usage View (Dashboard):**
    - **Features:** "Time Transcribed", "Words Generated", and "Active Model" stat cards.
    - **Visuals:** Pro-style activity bar chart (Activity Details) showing daily usage.
    - **Tech:** Custom SVG-based charts or lightweight library; data pulled from SQLite `daily_stats`.

2.  **Models View:**
    - **Features:** List of local models with "Quality" and "Speed" progress bars.
    - **Actions:** "Scan Directory" for new models, "Load" to switch the active STT engine.
    - **Tech:** Backend directory walker to identify GGUF/ONNX files and parse metadata.

3.  **Transcriptions View (History):**
    - **Features:** Searchable list of past transcriptions with timestamps and model metadata.
    - **Actions:** One-click "Copy" to clipboard.
    - **Tech:** Paginated SQLite queries to maintain performance with large histories.

4.  **Settings View (Preferences):**
    - **Audio:** Input device selection (enumerated from `cpal`), VAD toggle.
    - **Shortcuts:** Global hotkey configuration (stored in `tauri-plugin-store`).
    - **General:** "Launch at Login" (Tauri autostart plugin), "Menu Bar Icon" (System Tray management).

## 4. Implementation Roadmap

### Phase 1: Foundation & Custom UI
- [ ] Initialize Tauri project with React/TS/Tailwind.
- [ ] Implement **macOS-style Custom Title Bar** and Collapsible Sidebar.
- [ ] Setup **SQLite database** schema for History and Stats.

### Phase 2: Audio & VAD Core
- [ ] Implement `AudioController` in Rust with **Input Device Selection**.
- [ ] Integrate VAD (Voice Activity Detection) with a toggleable state in the UI.

### Phase 3: Modular Engines
- [ ] Implement the **Engine Adapter Trait**.
- [ ] Integrate `whisper-rs` and `ort` (ONNX Runtime).
- [ ] Implement **Model Downloader** with progress reporting to UI.

### Phase 4: Integration & UX
- [ ] Global Shortcuts: `Cmd+Shift+Space` (Record), `Cmd+Shift+C` (Copy Last).
- [ ] Clipboard injection and Keyboard simulation via `enigo`.
- [ ] **Launch at Login** and **Menu Bar Icon** (System Tray) support.

## 5. Potential Issues & Mitigations

| Issue | Mitigation |
| :--- | :--- |
| **High RAM usage** | Default to `tiny.en` or `base.en` 4-bit quantized models (<150MB). |
| **Audio Clipping** | Implement automatic gain control or normalization in the Rust pre-processing stage. |
| **Permission Denied** | Handle Microphone permissions explicitly for macOS/Windows in the Tauri setup. |
| **Dependency Bloat** | Use `sidecars` for the `whisper.cpp` binary if the Rust bindings significantly increase build time or complexity. |

## 6. Technical Design Decisions (Strict)
- **Concurrency:** All STT processing must occur on a background thread (not the main/UI thread).
- **Memory:** Audio buffers should be cleared immediately after transcription.
- **Static Linking:** Prefer static linking of C++ libraries to ensure portability on ARM devices without requiring local toolchains.
