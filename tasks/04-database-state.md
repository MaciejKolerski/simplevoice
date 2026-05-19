# Stage 4: Database & Analytics

**Goal:** Persist usage data and transcription history to support the Dashboard and History views.

## Tasks:
- [ ] **Dependencies:** Install `@tauri-apps/plugin-sql` via pnpm and add `tauri-plugin-sql` to Cargo.toml.
- [ ] **Schema Setup:** Define `migrations/01_init.sql`:
    - `transcriptions` table (id, text, model_used, duration_ms, timestamp).
    - `daily_usage` table (date, words_generated, time_transcribed_ms).
- [ ] **Data Insertion:** Modify the `transcribe_audio` command to automatically insert the result and update daily metrics into the SQLite DB upon successful transcription.
- [ ] **Data Fetching:** Create Tauri commands to fetch data for the Usage View (aggregations for 7/30 days) and Transcriptions View (pagination/list).
- [ ] **Frontend Wiring:** Replace the hardcoded mock data in `UsageView.tsx` and `TranscriptionsView.tsx` with real data from SQLite.
