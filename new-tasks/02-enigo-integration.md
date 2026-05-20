# Task 02: Enigo Integration (Keyboard Simulation)

**Goal:** Implement robust, cross-platform keyboard simulation for "Auto-Paste" and "Text Injection" features using the `enigo` library.

## Sub-tasks:
- [ ] **Dependencies:** Add `enigo` to `src-tauri/Cargo.toml`.
- [ ] **Native Permissions:** Ensure Accessibility permissions (macOS) or Input permissions are handled.
- [ ] **Implementation:** Replace the current `osascript` hack in `src-tauri/src/lib.rs` with `enigo` calls for `Cmd+V`.
- [ ] **Advanced Feature:** (Optional) Add a toggle to "Type Text" instead of "Paste" for environments where clipboard is restricted.
- [ ] **Validation:** Verify that transcription text is correctly injected into active applications like VS Code, Notes, etc.
