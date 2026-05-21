# Zadanie 1.2: Synchroniczne uruchamianie procesów PowerShell w pętli na systemie Windows

## Opis problemu
W pliku `src-tauri/src/media_control.rs` funkcja `windows_is_media_playing()` w pętli synchronicznie wywołuje polecenie powłoki PowerShell (`Command::new("powershell")`) dla każdego z 13 procesów. Trwa to 3-10 sekund, blokując wątek główny aplikacji i interfejs UI na start nagrywania.

## Pliki do modyfikacji
- [`src-tauri/src/media_control.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/media_control.rs)

## Zalecane rozwiązanie
1. Unikaj wywoływania PowerShella w pętli.
2. Zamiast tego użyj lekkiej biblioteki systemowej w Rust (np. `sysinfo`) do pobrania uruchomionych procesów.
3. Alternatywnie wywołaj jeden skrypt PowerShell, który pobierze status wszystkich procesów za jednym razem.

## Lista kroków do wykonania
- [ ] Analiza kodu wykrywania procesów w `media_control.rs`.
- [ ] Dodanie biblioteki `sysinfo` do `Cargo.toml` lub refaktoryzacja skryptu PowerShell.
- [ ] Zastąpienie pętli synchronicznych wywołań PowerShella jedną operacją.
- [ ] Testy wydajnościowe na systemie Windows pod kątem responsywności UI przy rozpoczynaniu nagrywania.
