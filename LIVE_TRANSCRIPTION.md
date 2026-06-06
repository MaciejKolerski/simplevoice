# LIVE_TRANSCRIPTION.md

Plan dzialania i wdrozenia trybu transkrypcji mowy na tekst **na zywo** (live / streaming) dla Simplevoice.

- Status: spec zatwierdzony, gotowy do implementacji fazowej
- Data: 2026-06-05
- Autor planu: na podstawie zweryfikowanej mapy kodu (`audio.rs`, `lib.rs`, `stt/`, `cloud.rs`, migracje, frontend) oraz syntezy state-of-the-art streaming ASR
- Powiazane: `SIMPLEVOICE.md` (architektura zywa), quality bar tamze obowiazuje w calosci

---

## 1. Cel i zakres

### 1.1 Cel

Dodac tryb, w ktorym tekst pojawia sie **w trakcie mowienia** (a nie dopiero po zakonczeniu wypowiedzi), w kilkusekundowych przyrostach, i jest na biezaco wpisywany do aktywnej aplikacji (dyktowanie na zywo) - przy **twardej gwarancji, ze zadne slowo nie zostanie przeciete na granicy fragmentu**.

### 1.2 Zasada nadrzedna (rdzen calego projektu)

> **Tniemy audio wylacznie w ciszy lub na potwierdzonej granicy slowa/zdania. Utrzymujemy niezmienna strefe "committed" i mutowalna strefe "tentative". Auto-paste wpisuje wylacznie delte committed.**

Z tej jednej zasady wynika rozwiazanie problemu "pol slowa": slowo trafia do strefy committed (i do docelowej aplikacji) dopiero, gdy jest **stabilne** - potwierdzone przez dodatkowy kontekst. Dopoki nie jest stabilne, zyje tylko w nakladce jako tekst tentative i moze sie jeszcze zmienic.

### 1.3 Decyzje produktowe (zatwierdzone)

| Decyzja | Wybor |
|---|---|
| Wyjscie tekstu | Wpisywanie na zywo (auto-paste **tylko zatwierdzonych** slow; append-only, bez cofania w docelowej aplikacji) |
| Silnik live | Pelna abstrakcja streamingu dla **wszystkich** sciezek, wybor strategii w ustawieniach |
| Latencja | Zbalansowana ~1-2 s (zatwierdzanie stabilnych slow/fraz) |
| UI | Rozszerzenie istniejacego `RecordingWindowView` |

### 1.4 Decyzje domyslne (przyjete, mozliwe do zmiany)

- Nowy, **opcjonalny** trait `TimestampedAsr` zamiast lamania istniejacego `AsrEngine::transcribe()`.
- Knoby VAD (`vad_threshold`, `vad_silence_duration_ms`) przeniesione z pamieci do `config.json` i wystawione w ustawieniach.
- Segmenty czastkowe sa **ephemeralne**; do SQLite trafia tylko finalny tekst sesji (jak dzis). Opcjonalna tabela historii live - dopiero Faza 5, jesli bedzie potrzebna.
- Rozpoczecie nowego nagrania w trakcie aktywnej sesji live - najpierw **finalizujemy** biezaca, potem startujemy nowa.

### 1.5 Poza zakresem (non-goals)

- Tlumaczenie miedzyjezykowe (to jest transkrypcja, nie translacja).
- Diaryzacja (kto mowi).
- Live na urzadzeniach mobilnych.
- Zmiana istniejacego trybu batch - tryb live jest **addytywny** i wlaczany przelacznikiem; dotychczasowy flow "nagraj -> VAD stop -> transkrybuj calosc" pozostaje domyslny i nietkniety.

---

## 2. Stan obecny (zweryfikowane fakty)

Caly dzisiejszy flow jest **batch / single-shot**; nie istnieje zadna sciezka streamingowa.

### 2.1 Audio (`src-tauri/src/audio.rs`)

- `AudioController::start_recording(app_handle, pause_audio)` otwiera strumien CPAL (cpal 0.15) z `device.default_input_config()` (natywne kanaly + `sample_rate.0`).
- Trzy galezie callbacku (F32/I16/U16) -> wszystkie do `f32`; `downmix()` (srednia kanalow -> mono) -> wlasny `Resampler` (interpolacja liniowa) normalizuje do **16 kHz mono f32** -> `producer.push_slice(&resampled)` do `SharedRb::<Heap<f32>>` o pojemnosci `dst_rate * 10 = 160_000` probek (10 s @ 16 kHz, ringbuf 0.4).
- Watek consumer (`audio.rs:175-287`) drenuje `consumer.pop_slice(&mut local_buf)` po **1024 probki** (poll 50 ms), dopisuje do `AudioState.buffer: Vec<f32>`, liczy `RMS = sqrt(sum_sq/read)` i emituje event `audio-amplitude`. **To jest naturalny punkt podpiecia strumienia chunkow** - chunki sa juz tu dostepne, dzis uzywane tylko do metra.
- VAD: tylko RMS, bez zewnetrznej biblioteki. `rms >= vad_threshold` (default **0.008**) ustawia `has_spoken=true`; inaczej narasta `silence_samples`; gdy `silence_samples >= vad_silence_duration_ms/1000 * 16000` (default **1500 ms**) -> auto-stop: `is_recording=false`, `is_saving=true`, pauza strumienia, watek zapisu, emit `recording-stopped`.
- Knoby VAD sa **hardcoded** w `AudioController::new` (`audio.rs:79-81`) i nieeksponowane do frontendu. `set_vad_enabled(enabled)` zmienia tylko `vad_enabled` (w pamieci, bez persystencji).
- Na stop pelny `buffer` kopiowany do `AudioState.last_samples`, kodowany do WAV (`save_wav_file()`, hound 3.5, f32->i16, 16-bit, 1 ch, 16000 Hz), emit `recording-stopped` z sciezka WAV.

### 2.2 STT (`src-tauri/src/stt/`)

- Trait (`stt/traits.rs`):
  ```rust
  pub trait AsrEngine: Send + Sync {
      fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<String, AppError>; // PCM 16 kHz mono
      fn display_name(&self) -> &str;
      fn model_format(&self) -> ModelFormat;
      fn supports_language_hint(&self) -> bool { true }
      fn gpu_accelerated(&self) -> bool { false }
  }
  ```
  Wyjscie: **goly `String`, bez timestampow, bez partiali, bez callbackow.**
- `GgmlWhisperEngine` (`stt/ggml_whisper.rs`): jedno wywolanie `state.full(params, samples)`, dzis z `params.set_no_timestamps(true)`, zbiera segmenty przez `full_n_segments()` + `get_segment(i).to_str()`.
- `OnnxEngine` (`stt/onnx_engine.rs`): sherpa-onnx `OfflineRecognizer` (Parakeet TDT 0.6b, Moonshine) - **offline/batch**.
- Candle Whisper (`stt/candle/whisper.rs`): mel ciety na segmenty 3000-ramkowe, reczna petla generacji tokenow - nie emituje partiali.
- NeMo (`stt/nemo_engine.rs`): subprocess python3 - batch.
- `SttController::transcribe()` (`stt/mod.rs`) owijane w `spawn_blocking`.

### 2.3 Orkiestracja (`src-tauri/src/lib.rs`) i cloud (`src-tauri/src/stt/cloud.rs`)

- Frontend `App.tsx` `handleStopped` lapie `recording-stopped` i wola `transcribe_audio(samples: Option<Vec<f32>>, ...)` (`None` => `last_samples`); routing do `SttController::transcribe()` (local) lub `crate::stt::cloud::transcribe_cloud()` (cloud).
- Wynik: auto-copy (arboard) -> `paste_text()` (enigo / Wayland) -> `save_transcription_data()` (SQLite) -> `set_last_transcription()` (`LastTranscription: Mutex<Option<String>>`).
- Cloud: zwykly `reqwest` POST calego WAV. OpenAI multipart `/audio/transcriptions` (`whisper-1`, `gpt-4o-transcribe`), Gemini JSON base64, OpenRouter JSON base64. **Brak WebSocket / tungstenite / mpsc.** Klucze w keyring `simplevoice / api_key_{provider}`.
- Eventy dzis: `recording-started`, `recording-stopped`, `audio-amplitude`, `transcribing-status`, `copy-last-success`, `recording-failed-to-start`, `model-status-changed`. **Brak eventu z tekstem czastkowym.**
- Globalne skroty (`tauri-plugin-global-shortcut`) -> `toggle_recording()` / `copy_last_transcription()`.

### 2.4 Konfiguracja i persystencja

- `ActiveConfig { engine: String="local", provider: String="openai", gpu_enabled: bool }` w pamieci; `config.json` w `app_local_data_dir` przez `save_config`/`load_config` (`lib.rs:1576-1639`, **merge zachowujacy nieznane klucze**, serializacja przez `CONFIG_FILE_LOCK`).
- Klucze `config.json`: `sound_feedback_enabled`, `pause_audio_on_record`, `recording_window_mode`, `recording_window_locked`, `recording_window_x/y`, `recording_window_has_custom_pos`, `gpu_enabled`.
- Frontend: `ConfigContext.tsx` (`Config = Record<string, any>`), per-feature toggles takze w `localStorage` (`vad_enabled`, `asr_engine`, `asr_provider`, `asr_model`, `asr_base_url`, `asr_language`).
- SQLite (`migrations/01_init.sql`): `transcriptions(id, timestamp, date, text, model, wav_path, duration_sec)`, `daily_usage(date, words_generated, time_transcribed_sec)`.
- Feature flags Cargo: `default = ["candle", "onnx"]`.

### 2.5 Macierz zdolnosci silnikow (do strategii live)

| Silnik | Plik | Batch/streaming | Word-timestamps | Strategia live |
|---|---|---|---|---|
| Whisper GGML/GGUF | `ggml_whisper.rs` | batch | **TAK** (zweryfikowane: `set_token_timestamps`+`set_max_len(1)`+`set_split_on_word`, `WhisperSegment::start/end_timestamp`, `WhisperTokenData{t0,t1}`) | **LocalAgreement-2** (sciezka referencyjna) |
| Candle Whisper | `candle/whisper.rs` | batch | mozliwe, ale wymaga alignacji DTW (spike) | LocalAgreement-2 po spike; do tego czasu VAD-segmented |
| sherpa Parakeet / Moonshine | `onnx_engine.rs` | offline/batch | brak word-timestampow dla LA-2 (modele offline; por. k2-fsa/sherpa-onnx#2918) | **VAD-segmented** (swiadomy, potwierdzony fallback - nie otwarte pytanie) |
| streaming Zipformer (NOWY) | `OnlineRecognizer` | natywny online | strumien tokenow, monotoniczny | **Native online + endpointing** |
| NeMo | `nemo_engine.rs` | batch (IPC) | nie | VAD-segmented |
| Cloud (OpenAI) | `cloud.rs` | dzis blokujacy POST | n/d | **Cloud realtime** (WebSocket) |
| Cloud (Gemini / OpenRouter) | `cloud.rs` | blokujacy POST | n/d | brak realtime -> VAD-segmented chunked POST |

---

## 3. Architektura docelowa

### 3.1 Przeglad warstw

```
                       +-----------------------------+
   audio chunki        |     StreamingController     |   eventy Tauri
   (16 kHz f32) -----> |  (lifecycle, wspolbieznosc) | -----> transcription-partial
        ^              |   posiada StreamWorker      |        transcription-committed
        |              +--------------+--------------+        transcription-final
   audio.rs                           |
   consumer thread                    v
   (fan-out tap)            +---------------------+
                           |  StreamingStrategy   |  (trait; wymienne)
                           +---------------------+
                            |  LocalAgreementStrategy   -> Stabilizer (LCP-of-2)
                            |  VadSegmentedStrategy      -> Silero VAD -> batch decode
                            |  NativeOnlineStrategy      -> sherpa OnlineRecognizer
                            |  CloudRealtimeStrategy     -> OpenAI Realtime WS
                            +---------------------+
                                       |
                            uzywa AsrEngine / TimestampedAsr / OnlineRecognizer / WS
```

Tryb batch (dzisiejszy) pozostaje rownolegla, nietknieta sciezka. Live jest wlaczany flaga `live_transcription_enabled`.

### 3.2 Nowy modul `src-tauri/src/stt/streaming/`

```
stt/streaming/
  mod.rs            // StreamEvent, StreamSink, StreamingStrategy trait, StreamingController, fasada
  stabilizer.rs     // LocalAgreement-2: committed/tentative, LCP slow, trimming bufora
  local_agreement.rs// LocalAgreementStrategy (uzywa TimestampedAsr)
  vad_segmented.rs  // VadSegmentedStrategy (uzywa AsrEngine batch) + brama VAD
  native_online.rs  // NativeOnlineStrategy (sherpa OnlineRecognizer)  [Faza 3, za cfg(feature="onnx")]
  cloud_realtime.rs // CloudRealtimeStrategy (OpenAI Realtime WS)       [Faza 4]
  vad.rs            // adapter VAD (Silero przez sherpa-onnx / dedykowany crate / fallback RMS)
  words.rs          // typ Word + normalizacja do porownan LCP
```

### 3.3 Typy zdarzen i trait strategii

```rust
// stt/streaming/mod.rs
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Tentatywny "ogon" - re-renderowany w calosci, NIGDY nie wpisywany do aplikacji.
    Partial { text: String },
    /// Nowo ustabilizowane slowa - append-only, bezpieczne do auto-paste.
    Committed { delta: String, full_committed: String },
    /// Finalizacja wypowiedzi/sesji (endpoint VAD albo stop uzytkownika).
    Final { text: String },
    /// Blad strategii (model error, OOM, timeout cloud). recoverable=true => strumien trwa dalej.
    Error { reason: String, recoverable: bool },
}

/// Kanal emisji (uzywamy crossbeam-channel, juz w zaleznosciach).
pub type StreamSink = crossbeam_channel::Sender<StreamEvent>;

/// Wspolny interfejs strategii live. Implementacje wolane z dedykowanego watku roboczego.
pub trait StreamingStrategy: Send {
    /// Dokarm mono 16 kHz f32 (dowolna dlugosc). Nieblokujace dla pipeline audio:
    /// ciezka praca offload do wlasnego watku/spawn_blocking wewnatrz strategii.
    fn push_audio(&mut self, samples: &[f32], sink: &StreamSink) -> Result<(), AppError>;
    /// Koniec mowy / stop: dopchnij reszte tentative do committed i wyemituj Final.
    fn finish(&mut self, sink: &StreamSink) -> Result<(), AppError>;
    /// Reset po finalizacji wypowiedzi (sesja moze obejmowac wiele wypowiedzi).
    fn reset(&mut self);
}
```

Uzasadnienie crossbeam zamiast bezposredniego `AppHandle::emit`: trzymamy strategie czyste i testowalne bez Tauri; `StreamingController` mapuje `StreamEvent` na konkretne eventy Tauri w jednym miejscu.

### 3.4 Word-timestamps: trait `TimestampedAsr`

```rust
// stt/streaming/words.rs
#[derive(Clone, Debug)]
pub struct Word {
    pub text: String, // z wiodaca spacja jak zwraca whisper, trim do porownan
    pub t0_ms: u32,   // poczatek wzgledem poczatku podanego bufora
    pub t1_ms: u32,   // koniec
    pub p: f32,       // prawdopodobienstwo (do progu pewnosci/diagnostyki)
}

// stt/traits.rs (dodatek; NIE zmieniamy istniejacego AsrEngine)
pub trait TimestampedAsr: AsrEngine {
    /// Dekoduj z timestampami na poziomie slowa. Default: niewspierane.
    fn transcribe_words(&self, samples: &[f32], language: Option<&str>)
        -> Result<Vec<Word>, AppError> {
        Err(AppError::Model("word timestamps unsupported".into()))
    }
    fn supports_word_timestamps(&self) -> bool { false }
}
```

Konkretna implementacja dla `GgmlWhisperEngine` (zweryfikowane API whisper-rs 0.16.0):

```rust
impl TimestampedAsr for GgmlWhisperEngine {
    fn supports_word_timestamps(&self) -> bool { true }

    fn transcribe_words(&self, samples: &[f32], language: Option<&str>)
        -> Result<Vec<Word>, AppError> {
        let mut guard = self.state.lock()/* ... */;
        let state = &mut *guard;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_temperature(0.0);
        params.set_n_threads(/* jak w transcribe() */);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_suppress_blank(true);
        // KLUCZ dla word-timestamps:
        params.set_no_timestamps(false);
        params.set_token_timestamps(true);
        params.set_max_len(1);        // jeden "segment" ~= jedno slowo
        params.set_split_on_word(true);
        match language {
            Some(l) if !l.trim().is_empty() && l != "auto" => params.set_language(Some(l)),
            _ => params.set_language(None),
        }
        state.full(params, samples)?;

        let mut words = Vec::new();
        for i in 0..state.full_n_segments() {
            if let Some(seg) = state.get_segment(i) {
                let text = seg.to_str().unwrap_or("").to_string();
                if text.trim().is_empty() { continue; }
                // whisper-rs: start/end_timestamp() -> i64 w centisekundach (10 ms);
                // *10 -> milisekundy. Clamp przy zwezaniu do u32 (overflow dopiero >~596 h).
                let t0 = (seg.start_timestamp().max(0).min(u32::MAX as i64) as u32) * 10;
                let t1 = (seg.end_timestamp().max(0).min(u32::MAX as i64) as u32) * 10;
                // Srednie prawdopodobienstwo slowa po tokenach. API zweryfikowane w 0.16.0:
                // WhisperToken::token_probability() -> f32; WhisperSegment::{n_tokens, get_token}.
                let n = seg.n_tokens();
                let p = (0..n).filter_map(|i| seg.get_token(i).map(|t| t.token_probability()))
                    .sum::<f32>() / (n.max(1) as f32);
                words.push(Word { text, t0_ms: t0, t1_ms: t1, p: p.clamp(0.0, 1.0) });
            }
        }
        Ok(words)
    }
}
```

Uwaga wydajnosciowa: tryb word-timestamps (`max_len=1`) jest nieco wolniejszy niz zwykly `full()`; akceptowalne, bo i tak dekodujemy maly bufor + gatujemy VAD-em. Pole `p` jest **diagnostyczne** - Stabilizer w MVP NIE uzywa go do decyzji o commitcie (decyduje wylacznie zgoda LocalAgreement-2).

### 3.5 Stabilizer (LocalAgreement-2) - rdzen gwarancji "bez pol slowa"

Algorytm (na podstawie Machacek et al., arXiv:2307.14743; `ufal/whisper_streaming`):

```
Stan:
  committed: Vec<Word>          // niezmienne, juz wyemitowane jako Committed
  prev_hyp:  Vec<Word>          // poprzednia hipoteza dla regionu niepotwierdzonego
  audio_buf: Vec<f32>           // od poczatku biezacego (jeszcze niezamknietego) zdania
  buf_start_ms: u32             // offset czasowy poczatku audio_buf

push_audio(new_samples):
  audio_buf.extend(new_samples)
  jesli (nowego audio < min_chunk_size, np. 1000 ms)  -> return   // self-adaptive
  hyp = engine.transcribe_words(audio_buf, lang)                 // PELNE re-dekodowanie bufora
  // odetnij prefiks juz potwierdzony (po liczbie slow committed w tym buforze)
  tail = hyp[liczba_slow_committed_w_buforze ..]
  agreed = longest_common_word_prefix(tail, prev_hyp)            // LocalAgreement-2 (zgoda 2x)
  jesli agreed niepuste:
     committed.extend(agreed)
     emit Committed { delta = join(agreed), full_committed = join(committed) }
  prev_hyp = tail
  emit Partial { text = join(tail[len(agreed)..]) }              // mutowalny ogon
  // przycinanie bufora na potwierdzonej granicy (cisza/koniec slowa/zdania):
  jesli (ostatnie committed konczy zdanie .?!) lub (audio_buf >= live_buffer_cap_s):
     cut_ms = t1_ms ostatniego potwierdzonego slowa
     // ciecie zawsze na granicy slowa; arytmetyka bezpieczna (bez panic/underflow):
     offset_ms = cut_ms.saturating_sub(buf_start_ms)
     off = (offset_ms as usize) * 16            // 16 probek / ms @ 16 kHz
     jesli (off == 0) lub (off > audio_buf.len()):
        pomin trim w tej iteracji                // straz przed panic/underflow
     inaczej jesli (cut_ms <= min(t0_ms slow tentative)):  // nie tnij w tentative
        audio_buf.drain(..off); buf_start_ms = cut_ms; prev_hyp.clear()
     inaczej:
        odloz trim do nastepnego dekodu

finish():
  // dopchnij wszystko, co zostalo w ogonie tentative
  hyp = engine.transcribe_words(audio_buf, lang)
  reszta = hyp[liczba_slow_committed_w_buforze ..]
  committed.extend(reszta)
  emit Final { text = join(committed_calej_sesji) }
  reset()
```

Dlaczego to eliminuje "pol slowa":
- Slowo trafia do `committed` (a wiec do auto-paste) dopiero, gdy **dwa kolejne pelne re-dekodowania** zgodza sie co do niego jako calego slowa (LCP po slowach, nie po znakach).
- Re-dekodowanie zawsze obejmuje pelny lewy kontekst (rosnacy bufor), wiec slowa przy krawedzi sa rozpoznawane z maksymalnym kontekstem, a nie z urwanego fragmentu.
- Przyciecie bufora ladowa **dokladnie na `t1_ms` potwierdzonego slowa** - nigdy w srodku slowa.

`longest_common_word_prefix` porownuje slowa po **znormalizowanej** formie - patrz `words.rs`:

```
normalize(w) = w.lower().trim().trim_edge_punct(".,!?;:\"')(")    // tylko interpunkcja brzegowa
longest_common_word_prefix(a, b):
   i = 0
   dopoki i < len(a) i i < len(b) i normalize(a[i]) == normalize(b[i]): i += 1
   zwroc a[..i]                 // ZAWSZE cale slowa, nigdy podlancuch
// edge-case'y (testy 6.1): "Mr." vs "mr", "don't", liczby, znaki nie-ASCII (NFC), puste segmenty pomijane
```

**Re-dekod: serializacja i cap bufora.** Re-dekodowanie biegnie w `spawn_blocking`, **FIFO, jeden naraz** (kolejny dekod nie startuje, dopoki poprzedni nie wroci; nowe audio jest buforowane, nie re-entrant). Dlugie `state.full()` trzyma `Mutex<WhisperState>` - dlatego biegnie **poza** watkiem audio/UI (nie blokuje callbacku CPAL ani `rebuild_tray_menu`). Gdy `audio_buf >= live_buffer_cap_s`, **wymuszamy trim** do ostatniej potwierdzonej granicy slowa przed dopisaniem kolejnego audio; jesli brak potwierdzonej granicy, odrzucamy najstarsze audio tentative i emitujemy `Error{recoverable:true, reason:"buffer_cap"}`. Hint jezyka **zablokowany raz na sesje** (`language_lock_strategy: auto|manual`) - bez per-chunk re-detekcji, inaczej jezyk migocze miedzy dekodami.

**Strazniki wiarygodnosci timestampow.** `max_len(1)`+`split_on_word` to nie kontraktowa gwarancja API, wiec: pomijamy segmenty puste/biale; wymagamy `t1_ms` niezerowego i monotonicznego `>=` poprzedniego; trim tylko gdy `cut_ms <= min(t0_ms tentative)`. **Fallback:** gdy word-timestamps sa niedostepne/niewiarygodne, tniemy na granicy **segmentu** Whispera (w ciszy) zamiast slowa - nadal nigdy w srodku slowa.

### 3.6 Strategie

**LocalAgreementStrategy** (`local_agreement.rs`) - sciezka referencyjna jakosci.
- Wymaga `TimestampedAsr` (MVP: Whisper GGML). Owija `Stabilizer`. `push_audio` co `min_chunk_ms` odpala re-dekodowanie w `spawn_blocking`, serializowane (jeden dekod naraz; nowe audio buforowane).
- Brama VAD (Silero) z `vad.rs`: cisza nie wyzwala re-dekodowania (oszczednosc CPU) i wyznacza punkty trim.

**VadSegmentedStrategy** (`vad_segmented.rs`) - najprostsza, dziala z **kazdym** silnikiem batch (`AsrEngine`).
- Silero VAD zamyka segment na pauzie konca mowy (`min_silence_duration_ms`), po czym caly segment idzie do `transcribe()` w `spawn_blocking`. Ciecie zawsze w ciszy => brak przecietych slow. Przeplyw: podczas mowy akumuluj + emituj `Partial{text:""}` (lub `"..."`) dla zywego feedbacku; po `min_silence_duration_ms` ciszy wyslij segment i po zwrocie emituj `Committed`/`Final`. **Uczciwie:** to daje tekst **per-wypowiedz** (brak partiali wewnatrz segmentu), nie slowo-po-slowie - dla plynnego live sluzy LocalAgreement-2 (Faza 1) albo natywny streaming (Faza 3).
- To uogolnienie dzisiejszej logiki RMS: zamiast "stop na ciszy" robimy "zamknij segment na ciszy i jedz dalej".

**NativeOnlineStrategy** (`native_online.rs`, Faza 3) - najnizsza latencja.
- sherpa-onnx `OnlineRecognizer` (streaming Zipformer transducer) + endpointing. `push_audio` -> `stream.accept_waveform` + `decode`; partiale = biezacy `result.text`; `is_endpoint`/`is_final` => `Final` + reset streamu. Slowa nie tniete "z definicji" (cache enkodera miedzy chunkami).
- Wymaga **nowego typu silnika** i modelu streaming (osobny od obecnych offline ONNX).

**CloudRealtimeStrategy** (`cloud_realtime.rs`, Faza 4) - BYOK.
- WebSocket (`tokio-tungstenite`, zaleznosc tylko tej fazy) do OpenAI Realtime transcription; audio jako base64 **PCM16 mono 24 kHz** przez `input_audio_buffer.append`. **Resampling 16->24 kHz dzieje sie wewnatrz tej strategii** (tap audio dostarcza wszystkim strategiom jednolicie 16 kHz; wspolny `stt/streaming/resampler.rs` rozszerza liniowy `Resampler` z `audio.rs`). Eventy: `...transcription.delta` => `Partial`, `...transcription.completed` => `Committed`/`Final`. Server VAD `silence_duration_ms ~500`.
- Gemini/OpenRouter: brak potwierdzonego realtime -> fallback do VAD-segmented chunked POST (segment z Silero VAD -> dotychczasowy `transcribe_cloud`). **To degraduje latencje** do ~1-3 s/pauze (nie ~1-2 s ciagle) - selektor strategii pokazuje wtedy tooltip o trybie segmentowym.

### 3.7 Audio tap (fan-out z consumer thread)

Minimalna zmiana w `audio.rs`, bez ruszania sciezki WAV:

```rust
// AudioState (dodatki)
pub stream_tx: Option<crossbeam_channel::Sender<Vec<f32>>>, // Some, gdy sesja live aktywna
pub live_mode_active: bool,                                 // true => VAD nie konczy nagrania (3.9)

// w petli consumer (audio.rs ~203-214), po appendzie do buffer i policzeniu RMS.
// Klonujemy Sender pod blokada, ale wysylamy JUZ PO jej zwolnieniu (consumer nigdy
// nie trzyma locka AudioState podczas send -> brak race/kontencji):
let tx = state.stream_tx.clone();        // tani klon (Arc wewnatrz crossbeam)
// ... drop(state_guard) ...
if let Some(tx) = tx {
    match tx.try_send(local_buf[..read].to_vec()) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => emit("transcription-buffering", ()), // dekoder nie nadaza
        Err(TrySendError::Disconnected(_)) => { /* worker zniknal: sesja konczona */ }
    }
}
```

- Pelny `AudioState.buffer` dalej akumuluje do WAV (zachowujemy mozliwosc finalnej re-transkrypcji / historii / zapisu pliku).
- **Kanal ograniczony (bounded)**: `crossbeam_channel::bounded(~10)` (~10 chunkow ~= ~640 ms audio). Na `Full` emitujemy `transcription-buffering` (zamiast cichego gubienia); strategia moze zareagowac wiekszym `min_chunk_ms`. Brak nieograniczonego wzrostu pamieci.
- **Synchronizacja `stream_tx`**: `StreamingController` ustawia `Some(tx)` w `start_live_session`, `None` w `stop_live_session`; consumer wysyla poza blokada `AudioState`. Kolejnosc teardownu w `finish()`: `stop=true` -> `stream_tx=None` -> drain kanalu -> `join` workera (timeout 5 s, 3.9).
- **VAD w trybie live** (`live_mode_active=true`): galaz VAD w `audio.rs` **nie** ustawia `is_recording=false` i **nie** emituje `recording-stopped`. Pauze konca-mowy przekazuje jako sygnal granicy do strategii (marker w kanale / `Partial{is_vad_pause}`), ktora zamyka wypowiedz (`Committed` + `reset()`). Koniec **sesji** to reczny stop/skrot albo (gdy `live_session_end_ms>0`) timeout zarzadzany przez `StreamingController`, nie watek audio. W trybie batch (live OFF) VAD auto-stop dziala jak dzis.

### 3.8 VAD (`vad.rs`)

- Preferencja: **Silero VAD przez sherpa-onnx** (juz zalezne pod feature `onnx`, domyslnie wlaczone; sherpa udostepnia `VoiceActivityDetector` Silero). Decyzja: tryb live zaklada `onnx` ON dla Silero. **Gdy `onnx` wylaczone -> fallback do istniejacego progu RMS** (zero nowych zaleznosci; granice mniej dokladne, ale ciecia nadal w ciszy). Nie wprowadzamy niezweryfikowanego crate'a - jesli kiedys zechcemy czystego-Rust Silero bez onnx, dodamy go warunkowo z dokladna wersja po weryfikacji.
- Parametry (Silero `VADIterator`): `threshold=0.5` (offset `-0.15` histereza), `min_silence_duration_ms=500` (300-700), `speech_pad_ms=100`, okna 512 probek (32 ms @ 16 kHz).

### 3.9 StreamingController (lifecycle + wspolbieznosc)

```rust
// stt/streaming/mod.rs
pub struct StreamingController {
    worker: Mutex<Option<StreamWorker>>, // None = brak aktywnej sesji
    committed_so_far: Mutex<String>,     // JEDYNE zrodlo prawdy dla wpisanego tekstu (3.12)
    session_id: Mutex<Option<String>>,   // UUID na sesje (wiersz historii)
}

struct StreamWorker {
    audio_tx: crossbeam_channel::Sender<Vec<f32>>, // do strategii
    handle: std::thread::JoinHandle<()>,
    stop: Arc<AtomicBool>,
}
```

- `start(app, engine_or_cloud, strategy_kind, language)` - **wewnetrzne**, nigdy nie jako bezposrednia komenda frontendu. **Strazniki jak `is_recording_allowed`**: locka `SttState.engine`; jesli `loading_model_path.is_some()` a `engine.is_none()` -> `Err("model_loading_in_progress")` (klucz i18n); dla cloud sprawdza obecnosc klucza w keyring. Po przejsciu: generuje `session_id`, czysci `committed_so_far`, ustawia `AudioState.{stream_tx, live_mode_active}`, spawnuje workera (czyta audio -> `strategy.push_audio(.., &sink)`) i forwarder (`StreamEvent` -> eventy Tauri, z throttlingiem paste `live_paste_throttle_ms`).
- `finish()` - **sekwencja graceful shutdown**: `stop=true` -> `strategy.finish(&sink)` (dopchnij tentative) -> `stream_tx=None` + `live_mode_active=false` -> drain kanalu -> `join` workera z **timeoutem ~5 s** (po timeoutcie: log + detach, "cleanup incomplete"); worker sprawdza `stop` po kazdym zwroconym dekodzie. Na koniec: **jeden** wiersz do `transcriptions` (cala sesja, 3.14), `set_last_transcription`, opcjonalny finalny `paste_text`.
- Wspolbieznosc: `start()` gdy worker `Some` -> najpierw `finish()` poprzedniej, potem nowa (decyzja 1.4). `is_recording` pozostaje jedna flaga.
- Integracja z `toggle_recording()`: helper `live_mode_enabled(app)` (domyslnie false) routuje toggle do `start/stop_live_session` zamiast batch; batch zachowuje dotychczasowa sciezke i guard.

### 3.10 Eventy i komendy Tauri (nowe)

Eventy (**nowe**, emit z forwardera; payloady jako interfejsy TS na frontendzie):
- `transcription-partial` -> `{ text: string }` (tentatywny ogon, nadpisywany w calosci)
- `transcription-committed` -> `{ delta: string, full: string }` (`full` = pelny committed, zrodlo prawdy)
- `transcription-final` -> `{ text: string }`
- `transcription-error` -> `{ reason: string, recoverable: boolean }`
- `transcription-buffering` -> `{}` (dekoder nie nadaza; UI moze pokazac wskaznik)

Wspolistnienie ze starymi eventami: tryb **batch** dalej emituje `recording-stopped` + `transcribing-status` (bez zmian). Tryb **live** NIE emituje `recording-stopped`; strumien konczy `transcription-final`. `RecordingWindowView` w trybie live nasluchuje 4 nowych eventow zamiast polegac na `transcribing-status`.

Komendy (`lib.rs`):
- `set_live_transcription_enabled(enabled: bool)` -> zapis do `config.json`.
- `set_live_strategy(strategy: String)` -> `"auto" | "local_agreement" | "vad_segmented" | "native_online" | "cloud_realtime"`.
- `set_live_tuning(min_chunk_ms: Option<u32>, vad_threshold: Option<f32>, vad_silence_duration_ms: Option<u32>, commit_punctuation: Option<bool>, autopaste: Option<bool>)`.
- `start_live_session` / `stop_live_session` (uzywane przez toggle + skroty + UI).
- `"auto"` mapuje na strategie wg silnika: Whisper GGML -> local_agreement; ONNX/NeMo -> vad_segmented; streaming engine -> native_online; cloud OpenAI -> cloud_realtime; cloud inny -> vad_segmented.

### 3.11 Frontend

- `RecordingWindowView.tsx`: dodac strefe tekstu pod fala. Stan w `useState` (NIE refy): `committed: string`, `tentative: string`.
  - committed: pelny kontrast, ustawiany z `transcription-committed.full` (zrodlo prawdy z backendu).
  - tentative: przygaszony/kursywa, **nadpisywany w calosci** z `transcription-partial.text`.
  - Layout: `.live-text-zone { height: ~3-4 wiersze; white-space: pre-wrap; overflow-y: auto; }`, auto-scroll do dolu (uwaga na transparentny NSPanel + DPI/resize). Wskaznik na `transcription-buffering`, toast na `transcription-error`.
  - Reset obu pol na `recording-started`; na `transcription-final` zamroz committed + wyczysc tentative. Nowy klucz `recording_window_show_live_text` (domyslnie true, gdy live ON). Nasluch 4 nowych eventow obok istniejacych (`audio-amplitude` itd.).
- `App.tsx`: gdy live ON, lapie `transcription-committed` -> wpisuje **delte** przez `paste_text` (jesli `live_autopaste`) i akumuluje; na `transcription-final` -> `save_transcription_data` + `set_last_transcription`. Tryb batch (dzisiejszy `handleStopped`) pozostaje, gdy live OFF.
- Ustawienia (`SettingsView.tsx`): sekcja "Live transcription" - przelacznik, selektor strategii (auto/...), selektor latencji (mapowany na `min_chunk_ms` / `min_silence_duration_ms`), przelacznik auto-paste live, oraz knoby VAD (prog, cisza). Zapis przez nowe komendy + lustro w `localStorage` jak istniejace ustawienia.

### 3.12 Auto-paste committed delta (sedno UX)

- **Jedyne zrodlo prawdy: backend.** `StreamingController` posiada `committed_so_far: String`; `Committed{delta, full_committed}` niesie `full` jako prawde. Frontend wpisuje **tylko** `delta` przez `paste_text()` (enigo / Wayland zwp_virtual_keyboard / accessibility) i NIE liczy delty samodzielnie ani nie re-emituje.
- Tentative **nigdy** nie trafia do docelowej aplikacji - tylko do nakladki.
- Wpis jest append-only: poniewaz committed jest niezmienne, nigdy nie cofamy ani nie nadpisujemy znakow w obcej aplikacji.
- Na `Final` opcjonalnie dopisujemy spacje/interpunkcje konczaca.
- **Throttling**: `live_paste_throttle_ms` (domyslnie ~200 ms) w forwarderze - delta nie czesciej niz co X ms (laczy male delty), by uniknac walki o fokus i nadmiaru zdarzen klawiatury.
- **Bledy paste**: gdy `paste_text` zawiedzie (zmiana fokusu, IME, lag Wayland) -> emit `transcription-error{recoverable:true}`, toast w UI, strumien trwa dalej (committed zostaje w nakladce do recznego skopiowania). Zachowac dotychczasowa logike auto-paste per platforma (macOS accessibility / Wayland / Windows).

### 3.13 Konfiguracja (klucze `config.json`)

```jsonc
{
  "live_transcription_enabled": false,
  "live_strategy": "auto",            // auto|local_agreement|vad_segmented|native_online|cloud_realtime
  "live_min_chunk_ms": 1000,          // okno aktualizacji LocalAgreement-2
  "live_commit_punctuation": true,    // trim bufora na granicy zdania
  "live_autopaste": true,             // wpisywanie committed delty na zywo
  "live_session_end_ms": 0,           // 0 = koniec sesji tylko recznie; >0 = auto po dlugiej ciszy
  "live_buffer_cap_s": 20,            // twardy limit dlugosci bufora LA-2 (wymus trim)
  "live_paste_throttle_ms": 200,      // min odstep miedzy wpisami delty
  "language_lock_strategy": "auto",   // auto = wykryj raz na sesje | manual = z ustawien
  "recording_window_show_live_text": true,
  "vad_threshold": 0.008,             // PRZENIESIONE z audio.rs do persystencji
  "vad_silence_duration_ms": 1500     // PRZENIESIONE z audio.rs do persystencji
}
```

- Helper `load_vad_config(app) -> (f32, u32)` wolany w `start_recording` **przed** spawnem consumera (per-nagranie - zmiany dzialaja od nastepnego nagrania); cichy + logowany fallback do `(0.008, 1500)` przy braku/uszkodzeniu pliku. Suwaki w `SettingsView`: `vad_threshold [0.001, 0.05] krok 0.001`, `vad_silence_duration_ms [300, 3000] krok 100`.

### 3.14 Persystencja

- MVP: cala sesja live -> **jeden** wiersz w istniejacej tabeli `transcriptions`. Przy wielu wypowiedziach (oddzielonych pauzami VAD) `StreamingController` akumuluje teksty `Final` poszczegolnych wypowiedzi w jeden string sesji i zapisuje **raz** w `stop_live_session` (bez duplikatow per-wypowiedz). `model` bierzemy z aktywnego `SttController` i przekazujemy w `transcription-final`; `word_count`/`duration` liczone jak dzis. Opcjonalnie `session_id` (UUID) w wierszu. Brak zmian schematu.
- Opcjonalnie (Faza 5, tylko jesli potrzebna historia live): migracja `02_live_segments.sql`:
  ```sql
  CREATE TABLE IF NOT EXISTS transcription_segments (
      id TEXT PRIMARY KEY,
      session_id TEXT NOT NULL,
      seg_index INTEGER NOT NULL,
      text TEXT NOT NULL,
      t0_ms INTEGER, t1_ms INTEGER,
      committed INTEGER NOT NULL DEFAULT 1,
      created_at TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS idx_segments_session ON transcription_segments(session_id);
  ```

### 3.15 i18n

Dodac klucze w `src/i18n/locales/{en,de,pl}.json` (przejdzie `pnpm check:i18n`):

```
settings.live.title
settings.live.enable
settings.live.strategy            (+ opcje: auto, localAgreement, vadSegmented, nativeOnline, cloudRealtime)
settings.live.latency             (+ opcje: low, balanced, accurate)
settings.live.autopaste
settings.live.vadThreshold
settings.live.vadSilenceMs
recording.live.committedLabel
recording.live.tentativeLabel
recording.live.buffering
errors.modelLoading
errors.livePasteFailed
```

---

## 4. Roadmap fazowy

Kazda faza jest samodzielnie wartosciowa, przechodzi `pnpm lint`, weryfikowana `pnpm tauri dev`, i nie psuje trybu batch.

### Faza 0 - Szkielet + pierwszy dzialajacy tryb (VAD-segmented)

Cel: dziala live z **dowolnym** obecnym silnikiem (tekst pojawia sie per-pauza, ciecia w ciszy).

Kroki:
1. Nowy modul `stt/streaming/` z `StreamEvent`, `StreamSink`, `StreamingStrategy`, `StreamingController` (3.3, 3.9).
2. `vad.rs` - adapter Silero (sherpa-onnx pod feature `onnx`; fallback RMS) (3.8).
3. `VadSegmentedStrategy` (3.6).
4. Audio tap w `audio.rs` (`stream_tx`, fan-out) (3.7); knoby VAD z `config.json` (3.13).
5. Eventy + komendy Tauri (3.10); integracja w `toggle_recording` za flaga `live_transcription_enabled`.
6. Frontend: strefa tekstu w `RecordingWindowView`, nasluch eventow (committed/tentative), reset (3.11). Bez auto-paste jeszcze (lub tylko `Final`).
7. Ustawienia: przelacznik live + selektor strategii (na razie `auto`/`vad_segmented`) (3.11); i18n (3.15).

DoD: w trybie live, mowiac z pauzami, widze tekst segment po segmencie w oknie; zadne slowo nie przeciete; tryb batch nietkniety.

### Faza 1 - Rdzen stabilizacji (LocalAgreement-2 dla Whisper GGML)

Cel: plynny live ~1-2 s z twarda gwarancja calych slow i korekta tylko w strefie tentative.

Kroki:
1. `words.rs` (typ `Word`, normalizacja, `longest_common_word_prefix`) + testy jednostkowe (czysta logika).
2. `TimestampedAsr` w `traits.rs`; implementacja dla `GgmlWhisperEngine` (3.4) - wlaczenie `token_timestamps`+`max_len(1)`+`split_on_word`; testy na probce audio.
3. `stabilizer.rs` (LocalAgreement-2, 3.5) + testy jednostkowe (sekwencje hipotez -> oczekiwane committed/tentative/trim).
4. `LocalAgreementStrategy` (3.6): brama VAD, re-dekod co `min_chunk_ms` w `spawn_blocking`, emisja Partial/Committed.
5. `"auto"` mapuje Whisper GGML -> `local_agreement`.

DoD: ciagla mowa (bez pauz) daje plynny strumien committed slow; mierzona latencja "wypowiedziane -> committed" ~1-2 s na malym modelu; brak przeciec slow w testach.

### Faza 2 - Live dictation (auto-paste committed delta)

Kroki:
1. `live_autopaste` w config + UI.
2. Wpisywanie tylko `transcription-committed.delta` przez `paste_text`; akumulacja; opcjonalny throttling (3.12).
3. Obsluga jezykow bez spacji (commit per token/znak) - flaga w strategii.

DoD: dyktujac na zywo do edytora widze tekst dopisywany na biezaco, bez cofania znakow, bez przeciec slow.

### Faza 3 - Natywny streaming (sherpa OnlineRecognizer)

Kroki:
1. Nowy typ silnika streaming (`OnlineRecognizer`) za `cfg(feature="onnx")`; rozpoznanie modelu streaming w `factory.rs`/`detect`.
2. `NativeOnlineStrategy` (3.6): `accept_waveform`/`decode`/`is_endpoint`.
3. Model manager: pozycja do pobrania streaming Zipformer (osobny od offline Parakeet).
4. `"auto"` mapuje silnik streaming -> `native_online`.

DoD: na modelu streaming latencja ~0,6 s, partiale token po tokenie, finalizacja na endpoincie; CPU nizsze niz LocalAgreement.

### Faza 4 - Cloud realtime (OpenAI)

Kroki:
1. Zaleznosci: `tokio-tungstenite` (WS) - tylko ta faza.
2. Wspolny resampler `stt/streaming/resampler.rs` (16->24 kHz, PCM16) uzyty wewnatrz strategii; tap audio pozostaje 16 kHz dla wszystkich.
3. `CloudRealtimeStrategy` (3.6): WS, `input_audio_buffer.append`, mapowanie `delta`/`completed`; klucz z keyring.
4. Fallback Gemini/OpenRouter -> VAD-segmented chunked POST.

DoD: z kluczem OpenAI live dziala przez chmure (delta/completed), reszta providerow fallbackuje sensownie.

### Faza 5 - Polish, testy, dokumentacja

Kroki:
1. Strojenie progow per strategia (sekcja 5), presety latencji low/balanced/accurate.
2. Pelna macierz testow manualnych (sekcja 6); edge-case'y VAD i wspolbieznosci.
3. (Opcjonalnie) tabela `transcription_segments` + historia live.
4. Aktualizacja `SIMPLEVOICE.md` (nowy modul `stt/streaming/`, eventy, config), README, i18n komplet (en/de/pl), screeny SVG jesli UI sie zmienia.

DoD: production-grade wg quality bar; `pnpm lint` czyste; przeglad adversarialny bez krytycznych uwag.

---

## 5. Parametry i strojenie

| Strategia | Parametr | Default | Zakres | Efekt |
|---|---|---|---|---|
| LocalAgreement-2 | `min_chunk_ms` | 1000 | 500-2000 | mniejszy = szybsze tentative, wiecej CPU |
| LocalAgreement-2 | `buffer_trimming` | segment | segment/sentence | gdzie przycinac bufor (zawsze na granicy slowa) |
| LocalAgreement-2 | `buffer_cap_s` | 15-30 | 10-30 | twardy limit dlugosci bufora (koszt re-dekodu) |
| LocalAgreement-2 | zgoda | 2x (LCP) | - | ile kolejnych dekodow musi zgodzic slowo |
| VAD-segmented | `min_silence_duration_ms` | 500 | 300-700 | dluzej = bezpieczniejsze granice, wieksze opoznienie |
| VAD-segmented | `speech_pad_ms` | 100 | 50-200 | margines, by nie ucinac poczatkow/koncow |
| Native online | `decode_chunk_len` | 32 ramki (~320 ms) | 16-64 | mniejszy = nizsza latencja, mniejszy prawy kontekst |
| Native online | `rule2_min_trailing_silence` | 1.2 s | 0.6-1.2 | proba finalizacji na pauzie |
| Cloud realtime | server VAD `silence_duration_ms` | 500 | 300-800 | dlugosc ciszy konczaca ture |

Presety latencji (UI -> parametry): **low** (`min_chunk_ms=500` / chunk 16 / silence 300), **balanced** (1000 / 32 / 500), **accurate** (VAD-segmented, silence 700).

Zrodla parametrow - sekcja 8.

---

## 6. Strategia testow

### 6.1 Jednostkowe (Rust, bez modelu - najwazniejsze)

- `words::longest_common_word_prefix`: prefiksy, rozne dlugosci, normalizacja (wielkosc liter, interpunkcja).
- `stabilizer`: sekwencje hipotez -> oczekiwane (committed, tentative, punkty trim). Wlasnosc krytyczna: **kazde committed slowo jest calym slowem z hipotezy** (nigdy podlancuch przecinajacy slowo). Test regresji na "pol slowa".
- `stabilizer::trim`: ms->probki (16/ms), `saturating_sub`, `off > len()` -> trim pominiety (bez panic); ciecie laduje dokladnie na granicy slowa.
- `TimestampedAsr` (GGML): `transcribe_words` zwraca niepuste slowa z rozsadnym `p` (>0, <=1) i monotonicznymi `t1_ms`.
- delta/committed: 5-10 kumulatywnych `Committed` -> suma `delta` == finalne `full`, bez duplikatow, w kolejnosci.
- VAD adapter: sekwencje RMS/prob -> oczekiwane granice segmentow.

### 6.2 Integracyjne

- Maly model Whisper (`tiny`/`base`) + przygotowany plik audio z mowa ciagla i z pauzami; sprawdz: brak przeciec, monotoniczne narastanie committed, finalny tekst ~= batch.
- Wspolbieznosc: start nowej sesji w trakcie aktywnej -> poprzednia sfinalizowana, brak panic/deadlock.

### 6.3 Manualne (macierz)

Silniki {Whisper GGML, Parakeet, Moonshine, Candle, [streaming Zipformer], [cloud OpenAI]} x strategie {local_agreement, vad_segmented, native_online, cloud_realtime} x platformy {macOS, Linux Wayland, Windows} - dyktowanie na zywo do edytora; weryfikacja: brak przeciec slow, poprawny auto-paste delty, brak cofania znakow, reset miedzy sesjami, zachowanie przy zmianie fokusu.

### 6.4 Wydajnosc

- Pomiar latencji "wypowiedziane -> committed" i obciazenia CPU/GPU per strategia/model; weryfikacja samo-adaptacji (pod obciazeniem wieksze, rzadsze chunki).

---

## 7. Ryzyka i mitygacje

| Ryzyko | Mitygacja |
|---|---|
| Koszt CPU re-dekodu (rosnacy bufor) | maly model, brama VAD (nie dekoduj ciszy), agresywny trim, `min_chunk_ms`, samo-adaptacja |
| Word-timestamps w Candle wymagaja DTW | spike w Fazie 1; do tego czasu Candle -> VAD-segmented |
| Brak WebSocket w drzewie | `tokio-tungstenite` tylko w Fazie 4, izolowane |
| Realtime tylko OpenAI | Gemini/OpenRouter -> fallback VAD-segmented chunked POST |
| Walka o fokus przy auto-paste | throttling wpisow, wpis tylko committed delty, zachowac istniejaca logike platform |
| Jezyki bez spacji (zh/ja) | commit per token/znak, flaga w strategii |
| Latencja na slabym CPU | presety latencji, domyslnie mniejsze modele dla live, info w UI |
| Blad strategii w trakcie (model/cloud/OOM/timeout) | `StreamEvent::Error{recoverable}`, toast, strategia kontynuuje (skip chunka / nasluchuj dalej) |
| Worker zawieszony w dlugim dekodzie przy stopie | `finish()` z timeoutem ~5 s -> log+detach; worker sprawdza `stop` po kazdym dekodzie |
| Cichy zanik probek (backpressure) | bounded channel + `transcription-buffering`; brak silent-drop |
| Race/kontencja na `Mutex<WhisperState>` | re-dekod FIFO (jeden naraz) w `spawn_blocking`, poza watkiem audio/UI |
| Regresja trybu batch | live addytywny za flaga; testy batch pozostaja |

---

## 8. Zrodla

- Machacek, Dabre, Bojar, "Turning Whisper into Real-Time Transcription System" - https://arxiv.org/abs/2307.14743 (HTML: https://arxiv.org/html/2307.14743)
- `ufal/whisper_streaming` - https://github.com/ufal/whisper_streaming ; VAD iterator: https://github.com/ufal/whisper_streaming/blob/main/silero_vad_iterator.py
- sherpa-onnx streaming Zipformer transducer - https://k2-fsa.github.io/sherpa/onnx/pretrained_models/online-transducer/zipformer-transducer-models.html
- sherpa endpoint detection - https://k2-fsa.github.io/sherpa/ncnn/endpoint.html
- sherpa-onnx Parakeet streaming nuance (issue #2918) - https://github.com/k2-fsa/sherpa-onnx/issues/2918
- Silero VAD - https://github.com/snakers4/silero-vad
- OpenAI Realtime transcription - https://developers.openai.com/api/docs/guides/realtime-transcription
- OpenAI Realtime VAD - https://developers.openai.com/api/docs/guides/realtime-vad
- OpenAI Realtime server events - https://developers.openai.com/api/reference/resources/realtime/server-events
- Streaming ASR deployment (chunk/latency tradeoffs) - https://apxml.com/courses/speech-recognition-synthesis-asr-tts/chapter-6-optimization-deployment-toolkits/streaming-asr-deployment
- Google, "Quality and Stability of a Streaming On-Device Recognizer" - https://arxiv.org/pdf/2006.01416

---

## 9. Slowniczek

- **committed / tentative** - tekst potwierdzony (niezmienny, wpisywany) vs niepotwierdzony (mutowalny, tylko w nakladce).
- **LocalAgreement-2** - regula commitu: zatwierdz najdluzszy wspolny prefiks slow z 2 kolejnych re-dekodowan.
- **endpointing** - detekcja konca wypowiedzi w silniku streaming (sygnal finalizacji).
- **VAD** - Voice Activity Detection (wykrywanie mowy/ciszy).
- **delta** - nowo zatwierdzone slowa od ostatniej emisji (jedyne, co wpisujemy do aplikacji).
