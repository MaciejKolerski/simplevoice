# Zadanie 3.2: Zdwojenie logiki bazodanowej (Direct SQL w React)

## Opis problemu
Aplikacja pobiera dane z historii transkrypcji bezpośrednio w React za pomocą wtyczki `@tauri-apps/plugin-sql` (pliki `TranscriptionsView.tsx` i `UsageView.tsx`), jednocześnie zapisując je po stronie backendu Rust za pomocą `sqlx`. Rozprasza to logikę bazodanową i grozi niespójnością danych.

## Pliki do modyfikacji
- [`src/views/TranscriptionsView.tsx`](file:///home/woro/Dokumenty/simplevoice/src/views/TranscriptionsView.tsx)
- [`src/views/UsageView.tsx`](file:///home/woro/Dokumenty/simplevoice/src/views/UsageView.tsx)
- [`src-tauri/src/lib.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/lib.rs)

## Zalecane rozwiązanie
1. Przenieś pobieranie historii transkrypcji oraz statystyk użycia do nowych komend Tauri w języku Rust (np. `get_transcriptions` i `get_usage_stats`).
2. W kodzie React zastąp bezpośrednie zapytania SQL (`Database.load(...)` oraz `.select(...)`) wywołaniami IPC: `invoke("get_transcriptions")` i `invoke("get_usage_stats")`.
3. Usuń wtyczkę `@tauri-apps/plugin-sql` z frontendu, jeśli nie jest już potrzebna do innych celów.

## Lista kroków do wykonania
- [ ] Zdefiniowanie struktur DTO dla historii transkrypcji po stronie Rust.
- [ ] Implementacja komend Tauri do odczytu danych w `lib.rs` przy użyciu wstrzykniętego `SqlitePool`.
- [ ] Zmiana logiki pobierania danych w widoku `TranscriptionsView.tsx` na wywołanie nowej komendy Tauri.
- [ ] Zmiana logiki pobierania statystyk w widoku `UsageView.tsx` na wywołanie nowej komendy Tauri.
- [ ] Testy integralności i poprawności ładowania danych w widokach historii i statystyk.
