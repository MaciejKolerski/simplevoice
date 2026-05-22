# TASKS.md - SimpleVoice

**Priorytetyzowana lista zadań** wynikająca z analizy dwóch code review:
- `@GEMINI_CODE_REVIEW.md` (bardzo pozytywny, skupiony na architekturze i kilku konkretnych ryzykach)
- `@GROK_CODE_REVIEW.md` (krytyczny, podkreśla monolithic code, race conditions, error handling i maintainability)

## P0 - Krytyczne (stabilność, bezpieczeństwo, race conditions)

- [ ] **Naprawić race conditions i ryzyko deadlocku w zarządzaniu stanem nagrywania**
  - Głównie `src-tauri/src/audio.rs` (VAD consumer thread) + `src-tauri/src/lib.rs`
  - Wprowadzić `RecordingState` enum + kanały (`crossbeam-channel` lub `tokio::sync`) zamiast wielu `Mutex` + `unwrap()`
  - Usunąć hack `sleep(80ms)` przy zatrzymywaniu nagrywania
  - Priorytet: najwyższy

- [ ] **Rozbić monolithic `lib.rs` (~1492 LOC)**
  - Wydzielić: `commands.rs`, `tray.rs`, `config.rs`, `db.rs`, `recording.rs`, `state.rs`
  - Użyć `tauri::State<AppState>` konsekwentnie
  - Zastosować Single Responsibility Principle

- [ ] **Wprowadzić porządne zarządzanie błędami**
  - Zastąpić większość `unwrap()`, `expect()` i `.map_err(|e| e.to_string())`
  - Dodać `thiserror::Error` enum (`AppError`)
  - Sanitizować błędy przed wysyłaniem do frontendu (nie wyciekać kluczy API)

- [ ] **Rozbić `ModelsView.tsx` (~625 LOC)**
  - Wydzielić: `LocalModelsList.tsx`, `CloudProviderPanel.tsx`, `ApiKeyInput.tsx`, `ModelCard.tsx`
  - Użyć Zod do walidacji konfiguracji

## P1 - Architektura i Maintainability

- [ ] **Zastąpić własny resampler w `audio.rs` biblioteką `rubato` lub `samplerate-rs`**
  - Aktualny liniowy interpolator jest prymitywny i może powodować aliasing/distortion

- [ ] **Uczynić Tauri jedynym źródłem prawdy konfiguracji**
  - Usunąć duplikację `localStorage` + `ConfigContext` sync
  - Frontend powinien tylko czytać/zapisywać przez Tauri commands

- [ ] **Dodać walidację ścieżek w komendzie `open_folder`**
  - Zapobiec Path Traversal / Command Injection
  - Ograniczyć tylko do katalogu aplikacji (`app_local_data_dir`)

- [ ] **Przenieść wywołania procesów zewnętrznych (`media_control.rs`) na asynchroniczne**
  - Użyć `tokio::process::Command` zamiast blokującego `std::process::Command`
  - Dotyczy PowerShell, dbus-send, playerctl

- [ ] **Dodać indeksy do bazy danych**
  - `CREATE INDEX IF NOT EXISTS idx_transcriptions_date ON transcriptions(date);`
  - Ewentualnie inne indeksy dla `UsageView`

## P2 - Wydajność, UX, Privacy

- [ ] **Dodać opcję wyłączania zapisywania nagrań audio** (ważne dla prywatności)
- [ ] **Debounce i optymalizacja `rebuild_tray_menu`** (wywołuje się za często)
- [ ] **Poprawić VAD** (obecny RMS jest prosty – rozważyć webrtc-vad lub lepszą filtrację)
- [ ] **Dodać caching przy `scan_models()`** (obecnie skanuje dysk za każdym otwarciem)
- [ ] **Dodać podstawowe testy** (szczególnie integracyjne dla recording flow)
- [ ] **Dodać Privacy Policy / dokumentację** w aplikacji
- [ ] **Wyczyścić zależności** (aktualizacja `ort`, usuwanie niepotrzebnych crate'ów)

## P3 - Drobne ulepszenia

- [ ] Uruchomić `cargo clippy --fix --all-targets --all-features`
- [ ] Uruchomić `npm run lint -- --fix`
- [ ] Dodać więcej TypeScript strictness i `key` props w listach
- [ ] Ujednolicić logging (`tracing` zamiast mixu `println!` i `log`)

---

**Uwagi ogólne:**
- Komentarze w kodzie są już w 100% po angielsku i techniczne — super robota.
- Architektura STT (trait `EngineAdapter`) jest jedną z najmocniejszych stron projektu.
- Po zrobieniu P0 projekt przejdzie z "ambitnego solo projektu" na "solidną produkcyjną jakość".

**Postęp:**
- Usunięto pliki code review (GEMINI_*, GROK_*)
- Dodano indeksy do bazy danych (`idx_transcriptions_date` i `idx_transcriptions_timestamp`)
- Projekt kompiluje się (`cargo check` przeszedł)
- Próba wprowadzenia `thiserror + AppError` wymagała zbyt dużej refaktoryzacji wszystkich commandów Tauri (tauri::command wymaga specjalnego traitu dla error). Zostawione na później po rozbiciu `lib.rs`.

Pozostałe taski z P0 (race conditions, rozbicie lib.rs, ModelsView) są złożone i będą robione w kolejnych krokach. 

Zaczynamy od P0.
