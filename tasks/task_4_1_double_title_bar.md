# Zadanie 4.1: Podwójny pasek tytułu (Title Bar) na systemach Windows i Linux

## Opis problemu
W pliku `src-tauri/tauri.conf.json` ustawiona jest konfiguracja `"decorations": true` oraz `"titleBarStyle": "Overlay"`. Opcja `Overlay` jest obsługiwana wyłącznie na macOS, natomiast na systemach Windows i Linux Tauri wyświetla domyślną systemową ramkę okna. Jednocześnie w React renderowany jest autorski komponent `TitleBar.tsx`, co sprawia, że użytkownicy tych systemów widzą dwa paski tytułowe.

## Pliki do modyfikacji
- [`src-tauri/tauri.conf.json`](file:///home/woro/Dokumenty/simplevoice/tauri.conf.json)
- [`src/components/layout/TitleBar.tsx`](file:///home/woro/Dokumenty/simplevoice/src/components/layout/TitleBar.tsx)

## Zalecane rozwiązanie
1. Opcja A: Wyłącz dekoracje systemowe (`"decorations": false`) na platformach Windows i Linux w ustawieniach okna Tauri, aby korzystać wyłącznie z niestandardowego paska tytułowego.
2. Opcja B (rekomendowana): Ukrywaj komponent `<TitleBar />` na systemach Windows i Linux, jeśli systemy te zachowują dekoracje natywne, lub warunkowo steruj dekoracjami Tauri w zależności od platformy.

## Lista kroków do wykonania
- [ ] Analiza konfiguracji okna w `tauri.conf.json`.
- [ ] Zmiana ustawień dekoracji okna lub konfiguracji wyświetlania paska tytułu.
- [ ] Refaktoryzacja komponentu `TitleBar.tsx`, aby zachowywał odpowiedni styl na systemach macOS oraz Windows/Linux (np. użycie API Tauri do sprawdzenia aktualnej platformy).
- [ ] Weryfikacja wizualna na systemach Linux i Windows pod kątem występowania podwójnego paska.
