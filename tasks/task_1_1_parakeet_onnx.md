# Zadanie 1.1: Inicjalizacja sesji ONNX w silniku Parakeet przy każdej transkrypcji

## Opis problemu
W pliku `src-tauri/src/stt/parakeet.rs` sesja ONNX (`Session::builder().commit_from_file(model_path)`) jest tworzona i kompilowana od nowa przy każdym uruchomieniu transkrypcji. Powoduje to ogromne opóźnienia przy każdym nagraniu (nawet do kilkunastu sekund).

## Pliki do modyfikacji
- [`src-tauri/src/stt/parakeet.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/stt/parakeet.rs)

## Zalecane rozwiązanie
1. Zmodyfikuj strukturę silnika Parakeet, aby przechowywała zainicjalizowaną sesję ONNX (lub środowisko) jako pole.
2. Zainicjalizuj sesję ONNX jednorazowo przy starcie silnika.
3. W metodzie transkrypcji korzystaj z już istniejącej instancji sesji.

## Lista kroków do wykonania
- [ ] Analiza struktury silnika w `parakeet.rs`.
- [ ] Przeniesienie tworzenia sesji ONNX do metody inicjalizacyjnej/konstruktora.
- [ ] Refaktoryzacja funkcji transkrypcji do używania przechowywanej sesji.
- [ ] Testy wydajnościowe czasu startu transkrypcji.
