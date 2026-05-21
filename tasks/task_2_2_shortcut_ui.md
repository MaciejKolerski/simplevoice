# Zadanie 2.2: Brak interfejsu konfiguracji skrótów klawiszowych w Ustawieniach

## Opis problemu
W pliku `src/views/SettingsView.tsx` zaimplementowano pełną logikę nagrywania skrótów (np. `isRecordingShortcut`, `register_shortcut`), ale w kodzie JSX brakuje elementów interfejsu (przycisków/kontrolek), które wywołują rozpoczęcie nagrywania nowego skrótu. W efekcie użytkownik nie ma możliwości ich konfiguracji z poziomu UI.

## Pliki do modyfikacji
- [`src/views/SettingsView.tsx`](file:///home/woro/Dokumenty/simplevoice/src/views/SettingsView.tsx)

## Zalecane rozwiązanie
1. Dodaj w widoku ustawień sekcję dedykowaną skrótom klawiszowym.
2. Zaimplementuj przyciski (np. "Zmień skrót") pozwalające na wywołanie logiki `setIsRecordingShortcut(true)` oraz ustawiające cel skrótu (np. nagrywanie, kopiowanie).
3. Wyświetl aktualnie skonfigurowany skrót klawiszowy w czytelnej formie.

## Lista kroków do wykonania
- [ ] Przegląd stanu i funkcji obsługi skrótów w `SettingsView.tsx`.
- [ ] Dodanie sekcji w JSX do renderowania przycisków do zmiany skrótów.
- [ ] Spięcie przycisków z istniejącymi funkcjami nasłuchiwania i nagrywania skrótów.
- [ ] Weryfikacja działania nagrywania skrótów i ich zapisu.
