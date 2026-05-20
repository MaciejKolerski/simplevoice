# Task 01: SQLite Database Migration

**Goal:** Replace the current file-based JSON storage with a robust SQLite database for better performance and analytical capabilities.

## Sub-tasks:
- [ ] **Dependencies:** Add `tauri-plugin-sql` to `src-tauri/Cargo.toml`.
- [ ] **Plugin Initialization:** Register the SQL plugin in `src-tauri/src/lib.rs`.
- [ ] **Migrations:** Define the initial schema for `transcriptions` and `usage_stats`.
- [ ] **Data Migration:** (Optional/Cleanup) Transition existing `data.json` entries into the new database.
- [ ] **Backend Implementation:**
    - [ ] Update `save_transcription_data` to insert into SQLite.
    - [ ] Update `load_history` to fetch from SQLite with pagination support.
    - [ ] Implement `clear_history_cmd` using SQL `DELETE`.
    - [ ] Add commands to fetch aggregated stats for the `UsageView`.
- [ ] **Frontend Integration:**
    - [ ] Update `UsageView.tsx` to call SQL-backed commands.
    - [ ] Update `TranscriptionsView.tsx` to handle database-driven history.
- [ ] **Validation:** Verify that data persists correctly across app restarts and charts reflect real data.
