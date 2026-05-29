<div align="center">
  <img src="public/logo.png" width="144" height="144" alt="SimpleVoice" />
  <h1>SimpleVoice</h1>

  <p><strong>Privacy-first, local-offline Speech-to-Text & Voice Typing desktop assistant.</strong></p>

  <p>
    <img src="https://img.shields.io/badge/version-v0.1.0-blue" alt="version" />
    <img src="https://img.shields.io/badge/license-Apache--2.0-green" alt="license" />
    <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey" alt="platform" />
  </p>

  <p>
    <a href="#features">Features</a>
    ·
    <a href="#install">Install</a>
    ·
    <a href="#configuration">Configuration</a>
    ·
    <a href="#build-from-source">Build from source</a>
  </p>
</div>

---

SimpleVoice is a local-first, highly efficient Speech-to-Text (STT) and voice typing desktop assistant built on Tauri 2, Rust, and React 19. It runs speech models completely offline on your device, records system audio, and types the transcribed text directly into any active application or copies it to your clipboard. It also supports secure cloud APIs if you prefer cloud-based inference. Zero telemetry. Zero accounts. Fully open-source.

---

## Screenshots

<div align="center">
  <p><em>Premium dashboard tracking transcription metrics, usage statistics, and local engine status.</em></p>
</div>

---

## Features

### 🎙️ Native Audio Recording & VAD
- High-fidelity native audio recording utilizing the Rust-native **CPAL** library.
- Sleek, modern recording overlay window showcasing a real-time audio waveform.
- **Voice Activity Detection (VAD)**: Automatically stops recording and begins transcribing when you stop speaking.

### 🧠 Advanced ASR (Speech Recognition) Engines
- **Fully Local & Offline**:
  - **Whisper-rs**: Bindings to `whisper.cpp` featuring **Metal** hardware acceleration on macOS and **Vulkan** GPU acceleration on Linux/Windows for blazingly fast local transcriptions.
  - **Sherpa-ONNX & Parakeet**: Native support for Nvidia Parakeet TDT v3 (int8 quantized) for state-of-the-art multilingual recognition on the CPU with a tiny memory footprint.
- **Cloud (BYOK)**: Connect securely to OpenAI Whisper Cloud, OpenRouter, Anthropic, Gemini, or any custom OpenAI-compatible endpoints.

### ⌨️ Global Shortcuts & Auto-Paste
- Press a customizable global hotkey (default `CommandOrControl+Space`) to instantly toggle recording from anywhere in your operating system.
- **Auto-Paste**: Automatically simulates keypresses to type your dictation directly into the active text field.
  - **macOS**: Built-in native accessibility API simulation.
  - **Linux (Wayland)**: Native, in-process text injection via the `zwp_virtual_keyboard_v1` protocol — **no external tools required** (no `wtype`).
  - **Linux (X11) / Windows**: Employs robust keyboard simulators.
- Copy your last dictation instantly with a global hotkey (default `CommandOrControl+Shift+C`).

### 🔒 Secure Storage & Local Database
- All API keys are written directly to the native OS keychain using the Rust `keyring` crate. Keys never touch the disk or `localStorage`.
- Full history of transcriptions, durations, word counts, and WAV file paths stored locally in an **SQLite** database via Tauri's SQL plugin and `sqlx`.

### 📊 Beautiful Usage Analytics
- High-fidelity statistics dashboard showing total words generated, total time transcribed, and active engine status.
- Premium interactive charts (supporting 7-day, 30-day, and monthly all-time views) rendered with custom smooth gradients.

### 📥 Built-in Model Downloader & Manager
- Browse and download recommended local models (Whisper-cpp GGML, Parakeet ONNX) directly within the application with real-time download speed and progress reporting.
- Import custom models or place them in the dedicated models directory.

---

## Configuration

### Keyboard Shortcuts
You can configure global hotkeys under **Settings -> Keyboard Shortcuts**:
- **Toggle Recording Shortcut**: Starts and stops voice dictation globally.
- **Copy Last Transcription Shortcut**: Quickly copies the last generated text without opening the app window.

#### Linux Wayland Notes
On Wayland (e.g., Niri, Hyprland, Gnome), global shortcut capture is restricted by the compositor. For the best experience, you can bind the shortcut directly in your compositor's configuration. For example, in Niri (`~/.config/niri/config.kdl`):
```kdl
binds {
    "Mod+Space" { spawn "simplevoice" "--toggle"; }
}
```

### Linux Auto-Paste
Auto-pasting on Wayland is fully native and requires **no external tools** — text is
injected in-process through the `zwp_virtual_keyboard_v1` protocol. This works on
wlroots-based compositors (Sway, Hyprland, niri, etc.) and KWin.

GNOME/Mutter does not implement the virtual-keyboard protocol, so auto-paste is
unavailable there; the transcription is still copied to your clipboard, so you can
paste it manually with `Ctrl+V`.

---

## Build from source

### Prerequisites
- **Rust (Stable)**: [rustup.rs](https://rustup.rs)
- **Node.js (v20+) & pnpm**: [pnpm.io](https://pnpm.io)
- **System Prerequisites**: Consult the [Tauri 2 Prerequisites Guide](https://tauri.app/v2/start/prerequisites) for your platform (e.g. Build tools, WebKit2GTK on Linux).

### Run in Development
```bash
pnpm install
pnpm tauri dev
```

### Build Production Bundle
```bash
pnpm build          # TypeScript frontend check & build
pnpm tauri build    # Compile Rust backend and package app installer
```

### Validation & Lints
```bash
pnpm lint           # Run frontend TypeScript type checks
```

---

## Tech Stack
- **Frontend**: React 19, TypeScript, Vite, Lucide React, Vanilla CSS.
- **Backend**: Tauri 2, Rust, SQLite (via `tauri-plugin-sql` + `sqlx`).
- **Audio Processing**: CPAL (native audio recording), Hound (WAV file operations), Rodio (sound effects).
- **Inference Integration**: `whisper-rs` (native Whisper bindings), `ort` (ONNX Runtime bindings), native OS keyring integration.

---

## Contributing
Issues and Pull Requests are welcome! Feel free to open issues to report bugs, suggest features, or submit pull requests.

## License
SimpleVoice is open-source software licensed under the **Apache-2.0** License.