# Live Transcription — Faza 0c: Frontend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Make live transcription visible and usable end-to-end: a Settings toggle that flips `live_transcription_enabled` (read by the backend), live committed/tentative text rendered in the existing floating overlay (`recording_window`), and `App.tsx` skipping the batch transcribe when live is on (final text arrives via `transcription-final`, then pasted/saved).

**Architecture:** The overlay window (200x60 transparent NSPanel) renders the existing waveform pill; when live text arrives it grows via `setSize` and shows a committed/tentative text block below the pill, then shrinks back on the next recording. Everything is behind the flag — with it off, zero behavior change. Backend already emits `transcription-partial|committed|final|error` (Faza 0b).

**Tech Stack:** React 19, `@tauri-apps/api` (`listen`, `getCurrentWindow`, `LogicalSize`), `useConfig` (writes `config.json`), `react-i18next`. Verified by `pnpm lint` + `pnpm check:i18n`; the overlay layout needs a visual pass in `pnpm tauri dev` (GUI, not automatable here).

**Visual-verification caveat:** The overlay is a delicate transparent NSPanel. This plan uses conservative defaults (keep width 200, grow height only, keep waveform-only look identical when no live text). The exact pixel sizing (`EXPANDED_H`, text box max-height) is expected to be eyeballed/tuned by the user in `pnpm tauri dev`.

---

### Task 1: i18n keys

**Files:** `src/i18n/locales/{en,de,pl}.json`

- [ ] Add after the `vadDesc` line (line 61) in each file:
  - en: `"liveTranscription": "Live transcription",` / `"liveTranscriptionDesc": "Show text in the recording overlay as you speak, instead of only after you stop.",`
  - pl: `"liveTranscription": "Transkrypcja na żywo",` / `"liveTranscriptionDesc": "Pokazuj tekst w nakładce nagrywania w trakcie mówienia, a nie dopiero po zakończeniu.",`
  - de: `"liveTranscription": "Live-Transkription",` / `"liveTranscriptionDesc": "Text während des Sprechens im Aufnahme-Overlay anzeigen, statt erst nach dem Stopp.",`
- [ ] Run `pnpm check:i18n`; expected: parity OK.

### Task 2: SettingsView toggle

**Files:** `src/views/SettingsView.tsx`

- [ ] Add state `const [liveEnabled, setLiveEnabled] = useState(false);` alongside the other toggles.
- [ ] In the mount effect, mirror the sound pattern: read `localStorage.getItem("live_transcription_enabled") === "true"`, `setLiveEnabled(...)`, `updateConfig("live_transcription_enabled", ...)`.
- [ ] Add handler `handleLiveToggle(checked)`: setState + `localStorage.setItem` + `updateConfig("live_transcription_enabled", checked)`.
- [ ] Add a `<Switch>` row (after the VAD row) using `t("settings.liveTranscription")` / `...Desc`.

### Task 3: RecordingWindowView live text + resize

**Files:** `src/views/RecordingWindowView.tsx`

- [ ] Add `committed`/`tentative` `useState`. Listen: `transcription-committed` -> setCommitted(full), `transcription-partial` -> setTentative(text), `transcription-final` -> setCommitted(text)+setTentative(""), `recording-started` -> clear both.
- [ ] Wrap pill in a `flex-col` group; conditionally render the text block when `committed || tentative` (committed solid, tentative dimmed/italic). Keep the pill markup unchanged so waveform-only is identical.
- [ ] Effect on `hasText = !!(committed||tentative)`: `getCurrentWindow().setSize(new LogicalSize(200, hasText ? EXPANDED_H : 60))`.

### Task 4: App.tsx live handling

**Files:** `src/App.tsx`

- [ ] In `handleStopped`, compute `const live = localStorage.getItem("live_transcription_enabled")==="true" && (localStorage.getItem("asr_engine")||"local")==="local";`. If `live`: stash `wavPath`+`modelName` in refs, set `isRecording(false)`, and return early (no `transcribe_audio`).
- [ ] Add a `transcription-final` listener: if `live`, take `payload.text`, paste it (`paste_text`), save (`save_transcription_data` with stashed wavPath+model), `set_last_transcription`, play done sound, dispatch `transcription-added`, clear transcribing state.

### Task 5: Verify

- [ ] `pnpm lint` (tsc strict) -> PASS.
- [ ] `pnpm check:i18n` -> PASS.
- [ ] Manual (user): set live toggle on, load a local Whisper model, `pnpm tauri dev`, dictate -> overlay grows and shows committed/tentative text; on stop the text is pasted + saved to history. Tune `EXPANDED_H` if needed.

## Self-Review
- Spec coverage: LIVE_TRANSCRIPTION.md §3.11 (overlay committed/tentative, settings toggle), §3.10 (frontend listens to the 4 events), end-to-end live path. Incremental auto-paste of committed deltas remains Faza 2 (0c pastes the final text once, like batch).
- Behind the flag: default off => identical batch behavior.
