# Zadanie 3.1: Wyciek połączeń bazy danych (SQLite Connection Pool Leak)

## Opis problemu
W pliku `src-tauri/src/lib.rs` przy każdym wywołaniu komend bazodanowych (`save_transcription_data`, `clear_history_cmd`, `delete_transcription_cmd`) backend tworzy nowy pool połączeń za pomocą `SqlitePool::connect(&db_url).await`. Powoduje to powtarzające się alokowanie połączeń, ryzyko wyczerpania zasobów oraz błędy `database is locked`.

## Pliki do modyfikacji
- [`src-tauri/src/lib.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/lib.rs)

## Zalecane rozwiązanie
1. Zainicjalizuj `SqlitePool` jednorazowo w funkcji `run()` na poziomie rejestracji aplikacji Tauri.
2. Zarejestruj pool jako stan globalny Tauri za pomocą `.manage(pool)`.
3. Wstrzyknij stan bazy danych do poszczególnych komend Tauri przy użyciu parametru typu `State<'_, SqlitePool>`.

## Lista kroków do wykonania
- [ ] Zlokalizowanie funkcji inicjalizującej aplikację `run()` w `lib.rs`.
- [ ] Utworzenie i konfiguracja `SqlitePool` w tej funkcji oraz wywołanie `.manage(pool)`.
- [ ] Refaktoryzacja sygnatur komend Tauri `save_transcription_data`, `clear_history_cmd` i `delete_transcription_cmd` w celu pobierania stanu poola.
- [ ] Usunięcie lokalnego tworzenia połączeń `SqlitePool::connect` wewnątrz komend.
- [ ] Testy zapisu i odczytu historii transkrypcji.
