# Zadanie 2.3: Wyciek próbek dźwięku przy zatrzymaniu nagrywania (VAD i Stop)

## Opis problemu
W pliku `src-tauri/src/audio.rs` wątek konsumenta w pętli asynchronicznie czyści bufor kołowy po zatrzymaniu nagrywania (`!is_recording`). Próbki, które znajdowały się jeszcze w locie, trafiają z bufora kołowego z powrotem do wyczyszczonego już bufora `s.buffer`, ale dzieje się to po tym, jak plik WAV został zapisany. W rezultacie te próbki są tracone w bieżącej transkrypcji i doklejane na początku kolejnego nagrania.

## Pliki do modyfikacji
- [`src-tauri/src/audio.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/audio.rs)

## Zalecane rozwiązanie
1. Zsynchronizuj wątek konsumenta z wątkiem głównym podczas wywoływania procedury stop.
2. Upewnij się, że przed pobraniem próbek i zapisaniem pliku WAV, wątek konsumenta zdąży zrzucić wszystkie zalegające próbki z bufora kołowego do `s.buffer`.

## Lista kroków do wykonania
- [ ] Analiza synchronizacji wątku nagrywania w `audio.rs`.
- [ ] Dodanie mechanizmu blokowania/synchronizacji (np. zmienna warunkowa Condvar lub prosty sygnał zakończenia), aby wątek konsumenta dokończył przepisywanie próbek.
- [ ] Upewnienie się, że `s.buffer` jest całkowicie czyszczony po zapisie pliku, bez możliwości dopisania starych danych.
- [ ] Testy nagrań pod kątem utraty ostatnich sekund słów oraz czystości początku nowego nagrania.
