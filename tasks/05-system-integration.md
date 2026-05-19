# Stage 5: System Integration & Polish (Vibe-Coding Features)

**Goal:** Make the app a true desktop citizen with global shortcuts and text injection.

## Tasks:
- [ ] **Dependencies:** Install `tauri-plugin-global-shortcut`, `tauri-plugin-autostart`, and add `enigo` (for keyboard simulation) and `arboard` (for clipboard) to Cargo.toml.
- [ ] **Global Shortcuts:** Register `Cmd+Shift+Space` in Rust to globally toggle the `start/stop_recording` commands, even when the app is unfocused.
- [ ] **Keyboard Injection:** Implement a Rust function using `enigo` that simulates pasting (`Cmd+V` or typing) the transcribed text directly into the user's active window (e.g., VS Code).
- [ ] **Clipboard:** Copy the transcribed text to the system clipboard automatically.
- [ ] **System Tray (Menu Bar):** Configure a minimal System Tray icon that allows hiding/showing the main window.
- [ ] **Autostart:** Wire up the "Launch at Login" toggle in Settings to the autostart plugin.
