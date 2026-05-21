# Zadanie 3.5: Sztywno ustawiona liczba wątków CPU dla wnioskowania

## Opis problemu
W plikach `src-tauri/src/stt/mod.rs` (dla Whisper) oraz `src-tauri/src/stt/sherpa.rs` (dla Sherpa-ONNX) liczba wątków CPU używanych do obliczeń jest na sztywno wpisana jako odpowiednio `8` i `4`. Może to spowalniać silniejsze procesory lub dławić starsze układy o mniejszej liczbie rdzeni.

## Pliki do modyfikacji
- [`src-tauri/src/stt/mod.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/stt/mod.rs)
- [`src-tauri/src/stt/sherpa.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/stt/sherpa.rs)

## Zalecane rozwiązanie
1. Użyj zewnętrznej biblioteki (np. `num_cpus`) do automatycznego wykrywania liczby fizycznych lub logicznych rdzeni procesora użytkownika w czasie rzeczywistym.
2. Alternatywnie, dodaj w ustawieniach aplikacji (React UI + konfiguracja na dysku) opcję ręcznego wyboru liczby wątków przeznaczonych na transkrypcję.
3. Przekazuj dynamicznie wykrytą lub skonfigurowaną wartość do parametrów inicjalizacyjnych Whisper i Sherpa.

## Lista kroków do wykonania
- [ ] Analiza definicji wątków w `stt/mod.rs` i `sherpa.rs`.
- [ ] Dodanie biblioteki `num_cpus` do zależności w `Cargo.toml` (jeśli nie jest jeszcze obecna).
- [ ] Zastąpienie wartości `8` i `4` dynamiczną kalkulacją (np. połowa dostępnych rdzeni lub `num_cpus::get()`).
- [ ] Opcjonalnie: Dodanie suwaka wyboru wątków w widoku ustawień React.
- [ ] Weryfikacja stabilności i obciążenia procesora podczas transkrypcji.
