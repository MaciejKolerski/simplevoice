# Pełny Code Review – SimpleVoice (Speech to Text)

Niniejszy dokument zawiera szczegółową analizę kodu źródłowego aplikacji desktopowej **SimpleVoice** stworzonej w oparciu o framework **Tauri (React + TypeScript + Rust)**. 

Kod aplikacji jest ogólnie dobrze ustrukturyzowany, jednak wykryto w nim **kilka poważnych błędów wydajnościowych, architektonicznych oraz logicznych**, które negatywnie wpływają na stabilność, szybkość działania oraz bezpieczeństwo aplikacji.

Poniżej przedstawiono szczegółowy podział uwag wraz z rekomendacjami zmian. **Zgodnie z życzeniem, do kodu nie wprowadzono żadnych modyfikacji.**

---

## 1. Krytyczne błędy wydajnościowe i blokowanie wątku głównego (Critical Performance)

### 1.1. Inicjalizacja sesji ONNX w silniku Parakeet przy każdej transkrypcji
* **Plik:** [`src-tauri/src/stt/parakeet.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/stt/parakeet.rs#L3-L9)
* **Opis problemu:** W funkcji `transcribe_parakeet` sesja ONNX (`Session::builder().commit_from_file(model_path)`) jest tworzona i kompilowana **od nowa przy każdym uruchomieniu transkrypcji**. Modele ASR (takie jak Nvidia Parakeet) mają zazwyczaj od kilkuset megabajtów do gigabajtów. Ich ciągłe wczytywanie z dysku i alokowanie w pamięci RAM/VRAM przy każdym nagraniu powoduje gigantyczne opóźnienia (nawet do kilkunastu sekund) przed faktyczną analizą dźwięku.
* **Rekomendacja:** Sesja ONNX powinna być inicjalizowana jednorazowo w strukturze `ParakeetEngine` (np. w metodzie `initialize` lub podczas tworzenia silnika) i przechowywana w stanie struktury jako pole, analogicznie do tego, jak context jest przechowywany w `WhisperEngine`.

### 1.2. Synchroniczne uruchamianie procesów PowerShell w pętli na systemie Windows
* **Plik:** [`src-tauri/src/media_control.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/media_control.rs#L92-L175)
* **Opis problemu:** Funkcja `windows_is_media_playing()` sprawdza stan odtwarzaczy multimedialnych na Windowsie. Jeśli pierwsza próba przez WinRT nie powiedzie się lub zwróci brak odtwarzania, funkcja iteruje po liście 13 procesów (`known_procs`) i dla **każdego z nich** synchronicznie wywołuje polecenie powłoki PowerShell:
  `std::process::Command::new("powershell").args(...).output()`
  Uruchomienie procesu PowerShell na systemie Windows jest operacją bardzo ciężką i trwa zazwyczaj od 200 ms do 1 sekundy. Wykonanie tego w pętli do 13 razy może zablokować wątek główny aplikacji (a w konsekwencji cały interfejs UI) na **3 do 10 sekund** przy próbie rozpoczęcia nagrywania!
* **Rekomendacja:** Należy unikać uruchamiania PowerShella w pętli. Listę uruchomionych procesów można pobrać jednorazowo jednym zapytaniem do systemu (np. za pomocą lekkiej biblioteki systemowej w Rust, takiej jak `sysinfo` lub jednego skryptu PowerShell listującego wszystkie procesy na raz).

---

## 2. Błędy logiczne i integracyjne (Logical & Integration Bugs)

### 2.1. Niedziałające szablony dostawców BYOK (Anthropic, Gemini, OpenRouter)
* **Pliki:** [`src/views/ModelsView.tsx`](file:///home/woro/Dokumenty/simplevoice/src/views/ModelsView.tsx#L126-L162) oraz [`src-tauri/src/stt/cloud.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/stt/cloud.rs#L28-L97)
* **Opis problemu:** W zakładce chmurowej BYOK użytkownik ma do wyboru predefiniowanych dostawców: OpenAI, OpenRouter, Anthropic Claude oraz Google Gemini. Jednak backend w pliku `cloud.rs` (funkcja `transcribe_cloud`) przesyła plik dźwiękowy za pomocą standardowego zapytania multipart typu OpenAI (wysyłając dane na endpoint `/audio/transcriptions`).
  - **Anthropic Claude** nie posiada w ogóle endpointu transkrypcji dźwiękowej (wymaga przesyłania tekstu/obrazów na `/v1/messages`).
  - **OpenRouter** nie wspiera standardowej metody transkrypcji audio w ten sposób dla wszystkich modeli.
  - **Google Gemini** wymaga użycia Gemini File API, a nie standardowego formatu OpenAI na endpoint `/audio/transcriptions`.
  Próba użycia dostawców takich jak Anthropic lub Gemini spowoduje natychmiastowy błąd 404/400 z serwera.
* **Rekomendacja:** Należy usunąć te opcje z UI jako bezpośrednich dostawców transkrypcji audio, bądź zaimplementować dla każdego z nich dedykowaną obsługę API w pliku `cloud.rs`.

### 2.2. Całkowity brak UI do konfiguracji skrótów klawiszowych w ustawieniach
* **Plik:** [`src/views/SettingsView.tsx`](file:///home/woro/Dokumenty/simplevoice/src/views/SettingsView.tsx)
* **Opis problemu:** W kodzie komponentu `SettingsView.tsx` zaimplementowano pełną logikę nagrywania skrótów klawiszowych: nasłuchiwanie zdarzeń `keydown`/`keyup`, nakładka (overlay) do wciskania klawiszy oraz komendy IPC (`register_shortcut` i `register_copy_shortcut`). Jednak w zwracanym kodzie JSX **brakuje sekcji/przycisków do wywołania tej funkcji**! Zmienna `isRecordingShortcut` nigdy nie może zostać ustawiona na `true` przez interakcję użytkownika, przez co zmiana skrótów w ustawieniach jest dla użytkownika niemożliwa.
* **Rekomendacja:** Należy dodać w JSX w sekcji preferencji elementy interfejsu (np. przyciski "Zmień skrót" i wyświetlanie aktualnego skrótu), które ustawiają stan `setIsRecordingShortcut(true)` oraz odpowiedni `shortcutTarget`.

### 2.3. Wyciek próbek dźwięku przy zatrzymaniu nagrywania (VAD oraz Manual Stop)
* **Plik:** [`src-tauri/src/audio.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/audio.rs#L378-L422)
* **Opis problemu:** W metodzie `stop_recording` blokowana jest flaga `s.is_recording = false` i czyszczony jest bufor `let samples = std::mem::take(&mut s.buffer)`. Wątek konsumenta (ring buffer consumer thread) działa jednak asynchronicznie i co 50 ms sprawdza stan flagi. Kiedy zauważy `!is_recording`, wchodzi do bloku czyszczącego:
  ```rust
  while !consumer.is_empty() {
      let read = consumer.pop_slice(&mut local_buf);
      s.buffer.extend_from_slice(&local_buf[..read]);
  }
  ```
  Problem polega na tym, że te próbki (które były jeszcze w buforze kołowym w momencie kliknięcia stop) trafiają do wyczyszczonego już bufora `s.buffer`, ale **nie są zapisywane** do pliku WAV! Zostają w nim i będą doklejone na początku kolejnego nagrania, dopóki kolejna sesja nie wyczyści bufora na start. Powoduje to utratę końcówki nagrania oraz "zaśmiecenie" początku kolejnego nagrania.
* **Rekomendacja:** Wątek konsumenta powinien zrzucić wszystkie pozostałe próbki do bufora *przed* tym, jak wątek główny skopiuje próbki i zapisze plik WAV. Wymaga to odpowiedniej synchronizacji (np. użycia zmiennej warunkowej lub powiadomienia wątku konsumenta).

---

## 3. Architektura i Dobre Praktyki (Architecture & Best Practices)

### 3.1. Wyciek połączeń bazy danych (SQLite Connection Pool Leak)
* **Plik:** [`src-tauri/src/lib.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/lib.rs#L1009-L1011)
* **Opis problemu:** Przy każdym wywołaniu komend Tauri: `save_transcription_data`, `clear_history_cmd` oraz `delete_transcription_cmd` backend tworzy od zera **nowy** pool połączeń do bazy SQLite:
  `let pool = sqlx::SqlitePool::connect(&db_url).await...`
  Tworzenie połączeń przy każdej operacji zapisu/usuwania jest bardzo nieoptymalne i może prowadzić do blokowania pliku bazy danych (błędy `database is locked`) lub wyczerpania zasobów.
* **Rekomendacja:** Pool połączeń SQL powinien być utworzony raz w funkcji `run()` na poziomie inicjalizacji aplikacji Tauri i zarejestrowany w stanie aplikacji za pomocą `.manage(pool)`. Następnie komendy powinny go wstrzykiwać za pomocą parametru `pool: State<'_, SqlitePool>`.

### 3.2. Zdwojenie logiki bazodanowej (Direct SQL w React)
* **Pliki:** [`src/views/TranscriptionsView.tsx`](file:///home/woro/Dokumenty/simplevoice/src/views/TranscriptionsView.tsx#L24-L28) oraz [`src/views/UsageView.tsx`](file:///home/woro/Dokumenty/simplevoice/src/views/UsageView.tsx#L76-L80)
* **Opis problemu:** Aplikacja używa wtyczki `@tauri-apps/plugin-sql` w kodzie Reacta do bezpośredniego ładowania bazy danych (`Database.load("sqlite:simplevoice.db")`) i wykonywania zapytań SQL bezpośrednio w komponentach widoków. Równocześnie w pliku `lib.rs` backend wykonuje zapytania bezpośrednio przy użyciu `sqlx`. Powoduje to rozproszenie logiki biznesowej, utrudnia utrzymanie spójności schematu bazy danych oraz zwiększa ryzyko jednoczesnego zapisu z dwóch różnych sterowników SQL.
* **Rekomendacja:** Cała interakcja z bazą danych powinna odbywać się po stronie Rust (backendu). React powinien wywoływać dedykowane komendy Tauri (np. `get_transcriptions`, `delete_transcription`), a nie pisać bezpośrednio zapytania `SELECT` / `DELETE` w kodzie komponentów.

### 3.3. Nadpisywanie globalnego obiektu `localStorage`
* **Plik:** [`src/main.tsx`](file:///home/woro/Dokumenty/simplevoice/src/main.tsx#L8-L46)
* **Opis problemu:** W pliku wejściowym Reacta nadpisywane (monkey-patched) są metody `localStorage.setItem` oraz `localStorage.removeItem`. Robione jest to po to, aby przy każdej zmianie stanu zsynchronizować plik konfiguracyjny JSON na dysku za pomocą komendy `save_config`. Nadpisywanie natywnych obiektów przeglądarki jest uważane za antywzorzec (może powodować konflikty z zewnętrznymi bibliotekami). Ponadto pętla synchronicznie odczytująca cały `localStorage` przy każdej drobnej modyfikacji spowalnia aplikację.
* **Rekomendacja:** Lepiej stworzyć dedykowany Context w React (np. `ConfigProvider`) lub niestandardowy hook (np. `usePersistedConfig`), który w bezpieczny i jawny sposób zarządza stanem konfiguracji i komunikuje się z backendem Tauri.

### 3.4. Ograniczenie Whisper do pojedynczego segmentu (Single Segment Cutoff)
* **Plik:** [`src-tauri/src/stt/mod.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/stt/mod.rs#L60)
* **Opis problemu:** W parametrach uruchomienia silnika lokalnego Whisper ustawione jest:
  `params.set_single_segment(true);`
  To ustawienie zmusza silnik Whisper do transkrypcji **wyłącznie pierwszego zdania/segmentu wypowiedzi**. Jeżeli użytkownik zrobi dłuższą notatkę głosową i chwilę milczy, wszystko, co powie po pierwszej pauzie, zostanie odcięte i pominięte w transkrypcji.
* **Rekomendacja:** Usunięcie tego ustawienia (lub ustawienie go na `false`), aby Whisper przetwarzał całe nagranie niezależnie od liczby wygenerowanych segmentów.

### 3.5. Sztywno ustawiona liczba wątków CPU dla wnioskowania
* **Pliki:** [`src-tauri/src/stt/mod.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/stt/mod.rs#L52) oraz [`src-tauri/src/stt/sherpa.rs`](file:///home/woro/Dokumenty/simplevoice/src-tauri/src/stt/sherpa.rs#L16)
* **Opis problemu:** W silnikach Whisper i Sherpa-ONNX liczba wątków CPU używanych do wnioskowania (inference) jest wpisana na sztywno odpowiednio jako `8` i `4`. Może to przeciążyć starsze komputery (posiadające np. 2 lub 4 rdzenie) lub spowolnić mocniejsze maszyny, które mogłyby wykorzystać więcej wątków.
* **Rekomendacja:** Liczba wątków powinna być wykrywana automatycznie na podstawie konfiguracji sprzętowej użytkownika (np. przy użyciu biblioteki `num_cpus`) lub być konfigurowalna w preferencjach aplikacji.

---

## 4. Uwagi dotyczące UX i Konfiguracji Okna (UI/UX & Window Config)

### 4.1. Podwójny pasek tytułu (Title Bar) na systemach Windows i Linux
* **Pliki:** [`src-tauri/tauri.conf.json`](file:///home/woro/Dokumenty/simplevoice/src-tauri/tauri.conf.json#L25-L27) oraz [`src/components/layout/TitleBar.tsx`](file:///home/woro/Dokumenty/simplevoice/src/components/layout/TitleBar.tsx)
* **Opis problemu:** Aplikacja ma włączoną opcję `"decorations": true` oraz `"titleBarStyle": "Overlay"` w konfiguracji Tauri. Styl `Overlay` jest obsługiwany **wyłącznie na macOS** (pozwala na rysowanie zawartości pod przyciskami zamknij/minimalizuj). Na systemach Windows i Linux ta właściwość jest ignorowana, co powoduje wyświetlenie standardowej systemowej ramki okna z paskiem tytułu. Równolegle aplikacja renderuje własny, niestandardowy komponent `TitleBar` w React. W rezultacie użytkownicy systemów Windows i Linux widzą **dwa paski tytułu jeden pod drugim**, co wygląda mało estetycznie.
* **Rekomendacja:** W konfiguracji okna dla platform Windows/Linux należy wyłączyć dekoracje (`"decorations": false`), jeśli chcemy używać w pełni niestandardowego paska tytułu, lub ukrywać element `<TitleBar />` na systemach innych niż macOS przy zachowaniu dekoracji systemowych.
