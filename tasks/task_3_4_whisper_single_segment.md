# Zadanie 3.4: Ograniczenie Whisper do pojedynczego segmentu (Single Segment Cutoff)

## Opis problemu
W pliku `src-tauri/src/stt/mod.rs` w parametrach inicjalizacyjnych silnika Whisper znajduje się flaga:
`params.set_single_segment(true);`
To powoduje, że Whisper przetwarza i zwraca wyłącznie pierwsze wypowiedziane zdanie, odcinając całą resztę nagrania po wystąpieniu pierwszej pauzy.

## Pliki do modyfikacji
- [`src-tauri/src/stt/mod.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/stt/mod.rs)

## Zalecane rozwiązanie
1. Zmień flagę na `false` lub całkowicie usuń linijkę `params.set_single_segment(true);`.
2. Upewnij się, że silnik przetwarza całe wejściowe nagranie audio i poprawnie łączy wszystkie wygenerowane segmenty tekstowe w jeden wynik.

## Lista kroków do wykonania
- [ ] Zlokalizowanie konfiguracji parametrów Whisper w `stt/mod.rs`.
- [ ] Usunięcie lub zmiana parametru `set_single_segment` na `false`.
- [ ] Dostosowanie ewentualnej pętli zbierania segmentów tekstu z Whisper (zbieranie wszystkich zamiast pierwszego).
- [ ] Przeprowadzenie testu z długim nagraniem (ponad 30 sekund z kilkoma zdaniami i przerwami na oddech) w celu weryfikacji pełnej transkrypcji.
