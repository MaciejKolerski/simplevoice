# Task 04: "Copy Last" Global Shortcut

**Goal:** Implement a secondary global shortcut to copy the most recent transcription to the clipboard without re-transcribing.

## Sub-tasks:
- [ ] **Shortcut Registration:** Add `Cmd+Shift+C` (or configurable) as a secondary global shortcut.
- [ ] **State Management:** Keep track of the last transcribed text in a globally accessible Rust state.
- [ ] **Command Execution:** Implement the logic to copy the stored "last text" to the system clipboard when the shortcut is triggered.
- [ ] **Feedback:** Provide a visual or audio cue (optional) that the text was copied.
- [ ] **Validation:** Verify that pressing the shortcut copies the last transcription even when the window is hidden.
