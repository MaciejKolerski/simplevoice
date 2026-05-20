# Task 03: Autostart Support

**Goal:** Allow the user to configure the application to launch automatically upon system login.

## Sub-tasks:
- [ ] **Dependencies:** Add `tauri-plugin-autostart` to `src-tauri/Cargo.toml`.
- [ ] **Plugin Setup:** Initialize the autostart plugin in `src-tauri/src/lib.rs`.
- [ ] **Command Integration:** Expose autostart enabling/disabling commands to the frontend.
- [ ] **Settings UI:** Connect the "Launch at Login" toggle in `SettingsView.tsx` to the autostart plugin.
- [ ] **Validation:** Verify the app is added to the system's login items/startup folder when enabled.
