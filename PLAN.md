# SimpleVoice — Modular ASR Architecture Plan

**Cel:** Przekształcić SimpleVoice w otwartą platformę STT obsługującą większość popularnych
modeli ASR z Hugging Face Hub, przy zachowaniu działania istniejących backendów.

---

## Kontekst i filozofia

SimpleVoice jest aplikacją open source — to oznacza, że użytkownicy:

- Mogą nie mieć GPU i muszą pracować na CPU
- Mogą nie znać Pythona — nie możemy wymagać ręcznej konwersji modeli
- Chcą po prostu wrzucić folder z HF Hub i mieć działający STT

**Nadrzędna zasada:** Użytkownik pobiera model z HuggingFace, wskazuje folder —
aplikacja sama wykrywa architekturę i uruchamia właściwy backend.

---

## 1. Krajobraz modeli HuggingFace ASR (2025)

Na HuggingFace dominują cztery rodziny architektur ASR:

| Rodzina | Przykładowe modele HF | Architektura | Format wag |
|---|---|---|---|
| **Whisper** | `openai/whisper-*`, `distil-whisper/*`, `nyrahealth/*` | Encoder-Decoder Transformer | `.safetensors` / `pytorch_model.bin` |
| **Wav2Vec2 / MMS** | `facebook/wav2vec2-*`, `facebook/mms-*`, `facebook/hubert-*` | Encoder + CTC head | `.safetensors` / `pytorch_model.bin` |
| **FastConformer / Parakeet** | `nvidia/parakeet-*`, `nvidia/canary-*` | FastConformer + Transducer/CTC | `.safetensors` |
| **Inne** | `speechbrain/*`, `microsoft/wavlm-*`, `facebook/seamless-*` | Różne | `.safetensors` / `.onnx` |

Dodatkowo istnieją skompilowane/skwantyzowane warianty:

| Format | Backend | Skąd pochodzi |
|---|---|---|
| `.bin` (GGML) | whisper-rs | Whisper.cpp konwersje |
| `.gguf` | whisper-rs >= 0.17 | Whisper.cpp / llama.cpp |
| `.onnx` | `ort` crate | Eksport przez `optimum-cli` |
| `.nemo` | sidecar Python | NVIDIA NeMo natywny format |

---

## 2. Strategia techniczna — trzy backendy

Zamiast implementować każdą architekturę od zera, SimpleVoice używa **trzech
specjalizowanych backendów**, z których każdy pokrywa inną klasę modeli:

```
┌─────────────────────────────────────────────────────────────┐
│                        SttController                        │
│                   Arc<dyn AsrEngine>                        │
└───────────┬─────────────────┬───────────────────────────────┘
            │                 │                               │
    ┌───────▼──────┐  ┌───────▼──────┐              ┌────────▼──────┐
    │  whisper-rs  │  │    Candle    │              │  ONNX Runtime │
    │  (ggml/gguf) │  │ (safetensors)│              │  (ort crate)  │
    └──────────────┘  └──────┬───────┘              └───────────────┘
                             │
                    ┌────────┴──────────┐
                    │                  │
             ┌──────▼─────┐   ┌────────▼──────┐
             │   Whisper  │   │   Wav2Vec2    │
             │(enc-dec HF)│   │  MMS / HuBERT │
             └────────────┘   │  (CTC decode) │
                              └───────────────┘
```

### Dlaczego ONNX jako trzeci backend?

Modele takie jak FastConformer, SpeechBrain, SeamlessM4T, WavLM i setki
fine-tunowanych modeli społecznościowych **nie mają gotowej implementacji
w `candle-transformers`**. Zamiast pisać je od zera w Rust, oferujemy
**wbudowany konwerter HF → ONNX** (Hugging Face Optimum, sidecar Python),
który pozwala uruchomić *dowolny* model ASR z HuggingFace — wystarczy jedna
komenda w UI aplikacji.

---

## 3. Nowa struktura plików

```
src-tauri/src/
├── lib.rs                          ← scan_models, load_model (rozbudowane)
├── error.rs                        ← dodać warianty dla nowych backendów
│
└── stt/
    ├── mod.rs                      ← [MODIFY] SttState → Arc<dyn AsrEngine>
    ├── traits.rs                   ← [NEW] AsrEngine trait, ModelInfo, ModelFormat
    ├── factory.rs                  ← [NEW] AsrFactory — detekcja formatu + konstrukcja
    ├── cloud.rs                    ← bez zmian
    │
    ├── ggml_whisper.rs             ← [NEW] przeniesiony WhisperEngine (whisper-rs)
    ├── gguf_whisper.rs             ← [NEW] GGUF support (whisper-rs >=0.17)
    │
    ├── candle/
    │   ├── mod.rs                  ← [NEW] wspólne utils: device, mel, tokenizer
    │   ├── whisper.rs              ← [NEW] Whisper enc-dec via candle-transformers
    │   └── wav2vec.rs              ← [NEW] Wav2Vec2/MMS/HuBERT CTC via candle
    │
    ├── onnx/
    │   ├── mod.rs                  ← [NEW] OnnxEngine (ort crate)
    │   ├── encoder_decoder.rs      ← [NEW] Whisper-style enc-dec ONNX
    │   └── ctc.rs                  ← [NEW] CTC-style ONNX (wav2vec2, conformer)
    │
    └── converter.rs                ← [NEW] HF→ONNX konwerter (Python sidecar)

src/
└── views/
    ├── ModelsView.tsx              ← [MODIFY] ikony formatów, filtrowanie, konwerter UI
    └── SettingsView.tsx            ← [MODIFY] opcja wyboru backend preference
```

---

## 4. Trait `AsrEngine` — plik `traits.rs`

```rust
use std::path::Path;

/// Wspólny interfejs dla wszystkich lokalnych backendów ASR.
/// Arc<dyn AsrEngine> trzymany w SttState.
pub trait AsrEngine: Send + Sync {
    fn transcribe(
        &self,
        samples: &[f32],          // PCM 16 kHz mono
        language: Option<&str>,   // None = auto-detect
    ) -> Result<String, crate::error::AppError>;

    fn display_name(&self) -> &str;
    fn model_format(&self) -> ModelFormat;
    fn supports_language_hint(&self) -> bool { true }
    fn gpu_accelerated(&self) -> bool { false }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ModelFormat {
    GgmlBin,        // whisper.cpp / whisper-rs, plik *.bin
    Gguf,           // whisper.cpp / whisper-rs >=0.17, plik *.gguf
    HfSafetensors,  // Hugging Face folder z model.safetensors
    HfPytorch,      // Hugging Face folder z pytorch_model.bin
    Onnx,           // folder z *.onnx (wyeksportowany przez optimum)
    Nemo,           // NVIDIA NeMo, plik *.nemo (experimental)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelInfo {
    pub path: String,
    pub format: ModelFormat,
    pub architecture: Option<String>, // "Whisper", "Wav2Vec2CTC", "FastConformer", ...
    pub hf_model_id: Option<String>,  // z config.json → _name_or_path
    pub display_name: String,
    pub size_bytes: u64,
    pub size_formatted: String,
    pub quality_score: u8,   // 0-100, do sortowania w UI
    pub speed_score: u8,     // 0-100
    pub is_active: bool,
    pub needs_conversion: bool, // True = HF safetensors bez ONNX → pokaż przycisk "Convert"
}
```

---

## 5. Factory — plik `factory.rs`

```rust
pub struct AsrFactory;

impl AsrFactory {
    /// Wykrywa format i tworzy odpowiedni engine.
    pub fn load(path: &Path, use_gpu: bool) -> Result<Box<dyn AsrEngine>, AppError> {
        let info = Self::detect(path)?;
        match info.format {
            ModelFormat::GgmlBin      => ggml_whisper::load(path, use_gpu),
            ModelFormat::Gguf         => gguf_whisper::load(path, use_gpu),
            ModelFormat::HfSafetensors | ModelFormat::HfPytorch => {
                match info.architecture.as_deref() {
                    Some("Whisper") => candle::whisper::load(path, use_gpu),
                    Some(a) if is_ctc_arch(a) => candle::wav2vec::load(path, use_gpu),
                    _ => Err(AppError::UnsupportedArchitecture(
                        info.architecture.unwrap_or_default()
                    )),
                }
            },
            ModelFormat::Onnx  => onnx::load(path),
            ModelFormat::Nemo  => Err(AppError::UseOnnxConversion),
        }
    }

    /// Odczytuje metadane modelu bez ładowania wag.
    pub fn detect(path: &Path) -> Result<ModelInfo, AppError> { ... }

    /// Odczytuje "architectures" z config.json w folderze HF.
    fn read_hf_architecture(dir: &Path) -> Option<String> {
        let config = std::fs::read_to_string(dir.join("config.json")).ok()?;
        let json: serde_json::Value = serde_json::from_str(&config).ok()?;
        json["architectures"][0].as_str().map(|s| s.to_string())
    }
}

fn is_ctc_arch(arch: &str) -> bool {
    matches!(arch,
        "Wav2Vec2ForCTC" | "HubertForCTC" | "UniSpeechSatForCTC" |
        "WavLMForCTC"    | "MCTCTForCTC"  | "SEWForCTC"
    )
}
```

### Logika detekcji formatu

```
path.is_file():
  *.gguf  → Gguf
  *.onnx  → Onnx (plik pojedynczy)
  *.nemo  → Nemo
  *.bin   → GgmlBin (zakładamy GGML; jeśli okaże się HF, błąd z radą)

path.is_dir():
  zawiera model.safetensors lub model.safetensors.index.json
    → HfSafetensors
    → odczytaj config.json → architecture
  zawiera pytorch_model.bin
    → HfPytorch
    → odczytaj config.json → architecture
  zawiera *.onnx + tokenizer.json
    → Onnx (folder wyeksportowany przez optimum)
  zawiera tokens.txt + *.onnx
    → Onnx (styl sherpa-onnx / parakeet-rs)
```

---

## 6. Backendy — szczegóły implementacji

### 6.1 Backend 1: `ggml_whisper.rs` + `gguf_whisper.rs`

**Zakres:** Istniejący `WhisperEngine` (przeniesiony bez zmian). GGUF przez whisper-rs >=0.17.

**Formaty:** `*.bin` (GGML), `*.gguf`

**Pokrycie modeli HF:** Modele z repozytorium `ggerganov/whisper.cpp`, `TheBloke/*`,
`openai/whisper-*` w skwantyzowanej formie.

**Zależności:**
```toml
# istniejące, bez zmian
whisper-rs = { version = "0.16.0", default-features = false }
```

---

### 6.2 Backend 2: `candle/` — Whisper i Wav2Vec2/MMS

**Zakres:** Natywne ładowanie modeli Hugging Face w formacie `.safetensors`
bez konwersji — bezpośrednio z folderu HF.

#### 6.2.1 `candle/whisper.rs`

**Formaty:** Folder HF z `model.safetensors` + `config.json` + `tokenizer.json`

**Pokrycie modeli (przykłady):**
- `openai/whisper-tiny`, `whisper-base`, `whisper-small`, `whisper-medium`, `whisper-large-v3`
- `distil-whisper/distil-large-v3`, `distil-whisper/distil-small.en`
- `nyrahealth/CrisperWhisper`
- Setki fine-tunów na HF (jezyk_COUNTRY/whisper-*)

**Implementacja:**
1. Device: `Device::new_metal(0)` (macOS), `Device::Cpu` (Linux/Windows)
2. Wagi: `VarBuilder::from_mmaped_safetensors(&[path/model.safetensors], DType::F32, &device)`
3. Config: `serde_json::from_str::<candle_transformers::models::whisper::Config>`
4. Tokenizer: `tokenizers::Tokenizer::from_file("tokenizer.json")`
5. Mel: load `mel_filters.safetensors` lub compute via candle
6. Inference: Encoder forward → greedy autoregressive decode (SOT → EOT)

#### 6.2.2 `candle/wav2vec.rs`

**Formaty:** Folder HF z `.safetensors` + `config.json` + `tokenizer.json`

**Pokrycie modeli:**
- `facebook/wav2vec2-base-960h`, `facebook/wav2vec2-large-960h-lv60-self`
- `facebook/mms-300m`, `facebook/mms-1b-all` (100+ jezyków)
- `facebook/hubert-large-ls960-ft`
- `microsoft/wavlm-large`, `microsoft/unispeech-sat-large-100h`
- Setki fine-tunów: `jonatasgrosman/wav2vec2-large-xlsr-*`

**Implementacja:**
1. Encoder forward pass (brak dekodera — jedna siec)
2. Logits → argmax po osi czasu (greedy CTC decode)
3. CTC blank token filtering + collapse repeats
4. Tokenizer do tekstu (vocab BPE lub character-level)

> **Uwaga:** `candle-transformers` 0.8 ma czesciowa implementacje Wav2Vec2.
> Moze wymagac dopisania brakujacych warstw (feature extractor conv stack).

**Zaleznosci:**
```toml
[features]
candle = [
  "dep:candle-core",
  "dep:candle-nn",
  "dep:candle-transformers",
  "dep:tokenizers",
]

[dependencies]
candle-core         = { version = "0.8", optional = true }
candle-nn           = { version = "0.8", optional = true }
candle-transformers = { version = "0.8", optional = true }
tokenizers          = { version = "0.21", optional = true,
                        default-features = false, features = ["onig"] }

[target.'cfg(target_os = "macos")'.dependencies]
candle-core = { version = "0.8", optional = true, features = ["metal"] }
```

---

### 6.3 Backend 3: `onnx/` — ONNX Runtime (model uniwersalny)

**Zakres:** Wszystkie modele wyeksportowane do ONNX przez `optimum-cli`.
To jest **klucz do obslugi 100% modeli HF** — kazdy model z `transformers`
moze byc wyeksportowany do ONNX, niezaleznie od architektury.

**Formaty:** Folder z plikami `.onnx` (Whisper = encoder + decoder osobno,
Wav2Vec2 = jeden plik, FastConformer = encoder + decoder)

**Pokrycie modeli po konwersji:**
- Cala rodzina Whisper i Distil-Whisper (przez `optimum-cli`)
- Wav2Vec2, MMS, HuBERT (przez `optimum-cli`)
- NVIDIA Parakeet, Canary (oficjalne eksporty NVIDIA lub przez `optimum`)
- SpeechBrain, WavLM, SeamlessM4T — cokolwiek co `optimum` potrafi wyeksportowac

#### 6.3.1 `onnx/encoder_decoder.rs` — Whisper-style

```
Pliki w folderze:
  encoder_model.onnx      ← AudioEncoder
  decoder_model.onnx      ← TextDecoder (bez KV cache)
  decoder_with_past.onnx  ← TextDecoder (z KV cache, szybsze)
  tokenizer.json, config.json, vocab.json
```

Implementacja:
1. Wczytaj `encoder_model.onnx` → `Session`
2. Wczytaj `decoder_with_past.onnx` → `Session`
3. Mel spectrogram (wlasna implementacja lub `ndarray`)
4. Encoder forward → cross-attention key-value
5. Autoregressive decode (greedy) z KV cache reuse

#### 6.3.2 `onnx/ctc.rs` — Wav2Vec2-style

```
Pliki w folderze:
  model.onnx         ← caly model
  tokenizer.json, config.json
```

Implementacja:
1. Wczytaj `model.onnx` → `Session`
2. Input: `[1, T]` float32 waveform
3. Output: CTC logits `[1, T', vocab_size]`
4. Greedy CTC decode → tekst

**Zaleznosci:**
```toml
[features]
onnx = ["dep:ort", "dep:ndarray"]

[dependencies]
ort     = { version = "2.0", optional = true,
            default-features = false, features = ["load-dynamic"] }
ndarray = { version = "0.16", optional = true }
```

> **`load-dynamic`**: ONNX Runtime nie jest linkowany statycznie — uzytkownik
> dostarcza `libonnxruntime.dylib/so/dll` lub aplikacja bundluje go przez Tauri.
> Alternatywa: `ort = { features = ["download-binaries"] }` — automatyczne pobranie
> biblioteki przy pierwszym uruchomieniu.

---

### 6.4 Wbudowany konwerter HF → ONNX (`converter.rs`)

To jest **kluczowy element** demokratyzacji dostepu do modeli.

**Problem:** Uzytkownik ma folder `openai/whisper-small` (safetensors),
ale `candle-transformers` nie obsluguje jego konkretnego wariantu albo
model jest z innej rodziny (np. FastConformer).

**Rozwiazanie:** Przycisk „Convert to ONNX" w UI uruchamia:

```bash
python -m optimum.exporters.onnx \
  --model /path/to/hf-model-folder \
  --task automatic-speech-recognition \
  /path/to/output-onnx-folder
```

**Implementacja w Rust:**

```rust
pub struct OnnxConverter;

impl OnnxConverter {
    /// Sprawdza czy Python + optimum sa dostepne.
    pub fn check_prerequisites() -> ConversionPrereqs { ... }

    /// Instaluje optimum jesli brakuje (pip install optimum[exporters]).
    pub async fn install_optimum() -> Result<(), AppError> { ... }

    /// Konwertuje folder HF do ONNX. Wysyla progress przez Tauri event.
    pub async fn convert(
        src: &Path,
        dst: &Path,
        app_handle: &tauri::AppHandle,
    ) -> Result<(), AppError> { ... }
}
```

**Tauri events podczas konwersji:**
```json
{ "event": "conversion-progress", "payload": { "percent": 42, "message": "Exporting encoder..." } }
{ "event": "conversion-done",     "payload": { "onnx_path": "/.../model-onnx" } }
{ "event": "conversion-error",    "payload": { "message": "optimum not found" } }
```

**Frontend UI:**
- Status: `Python ✓ / optimum ✓` (lub przycisk „Install optimum")
- Przycisk „Convert to ONNX" → progress bar → automatyczne zaladowanie po konwersji
- Informacja: „Konwersja jest jednorazowa, wyniki sa cachowane"

---

## 7. Zmiany w `lib.rs`

### 7.1 `scan_models` — nowa logika

```rust
// Stara logika: sprawdza tylko .bin, .gguf, foldery z tokens.txt
// Nowa logika: deleguje do AsrFactory::detect() dla kazdego wpisu

fn scan_models(...) -> Result<Vec<ModelInfo>, String> {
    let models_dir = ...;
    let mut models = Vec::new();
    for entry in read_dir(&models_dir).flatten() {
        if let Ok(info) = AsrFactory::detect(&entry.path()) {
            models.push(info);
        }
    }
    models.sort_by(|a, b| b.quality_score.cmp(&a.quality_score));
    Ok(models)
}
```

`ModelInfo` zastepuje dotychczasowy `LocalModel` — zawiera pole `architecture`
i `needs_conversion`, ktore frontend wykorzystuje do wyswietlenia odpowiednich akcji.

### 7.2 Nowe komendy Tauri

```rust
#[tauri::command]
async fn check_conversion_prereqs() -> ConversionPrereqs { ... }

#[tauri::command]
async fn install_optimum() -> Result<(), String> { ... }

#[tauri::command]
async fn convert_model_to_onnx(src: String, dst: String, ...) -> Result<(), String> { ... }
```

---

## 8. Zmiany frontendowe (`ModelsView.tsx`)

### 8.1 Nowe elementy UI

**Ikona/odznaka formatu** przy kazdym modelu:

| Format | Etykieta | Kolor |
|---|---|---|
| `ggml_bin` | GGML | szary |
| `gguf` | GGUF | szary |
| `hf_safetensors` | HF Native | fioletowy |
| `hf_pytorch` | HF PyTorch | fioletowy |
| `onnx` | ONNX | zielony |
| `nemo` | NeMo ⚠ | zolty |

**Przycisk „Convert to ONNX"** — widoczny gdy:
- `format === "hf_safetensors" || format === "hf_pytorch"`
- `architecture` nie jest w zbiorze obslugiwanym przez candle

**Filtrowanie modeli:**
- Wszystkie / GGML / ONNX / HF Native
- Sortowanie: Jakosc / Szybkosc / Rozmiar

### 8.2 Status backendow

Sekcja „Dostepne backendy" w Settings:

```
✓ GGML Whisper    — zawsze dostepny
✓ Candle HF       — dostepny (skompilowany z feature candle)
○ ONNX Runtime    — wymagany libonnxruntime [Pobierz]
○ Python/Optimum  — wymagany do konwersji modeli [Sprawdz]
```

---

## 9. Plan wykonania — etapy

### Etap 1 — Refaktoryzacja fundamentow `stt/` *(prereq dla wszystkiego)*

Priorytet: **Krytyczny**

- [ ] Utworz `stt/traits.rs` z `AsrEngine`, `ModelInfo`, `ModelFormat`
- [ ] Przeniesc `WhisperEngine` do `stt/ggml_whisper.rs`
- [ ] Zaimplementuj `AsrEngine for GgmlWhisperEngine`
- [ ] Utworz `stt/factory.rs` — podstawowa detekcja (GgmlBin + HfSafetensors)
- [ ] Zaktualizuj `stt/mod.rs`: `engine: Option<Arc<dyn AsrEngine>>`
- [ ] Zaktualizuj `scan_models` w `lib.rs` — uzywaj `AsrFactory::detect()`
- [ ] Zaktualizuj `LocalModel` → `ModelInfo` we froncie
- [ ] `cargo check` + `pnpm lint` — zero bledow

**Weryfikacja:** Istniejace `.bin` modele dzialaja identycznie jak przed.

---

### Etap 2 — Backend Candle: Whisper HF

Priorytet: **Wysoki** — pokrywa ~60% popularnych modeli HF

- [ ] Dodaj feature `candle` do `Cargo.toml`
- [ ] Utworz `stt/candle/mod.rs` — device selection, mel utils
- [ ] Utworz `stt/candle/whisper.rs` — `CandleWhisperEngine`
- [ ] Podlacz w `factory.rs` dla `HfSafetensors + architecture=Whisper*`
- [ ] Test: `openai/whisper-tiny` → nagranie → transkrypcja
- [ ] Test: Metal na macOS (sprawdz uzycie GPU w Activity Monitor)

**Weryfikacja:** Ladowanie `openai/whisper-small` z HF folder bez konwersji.

---

### Etap 3 — Backend Candle: Wav2Vec2 / MMS

Priorytet: **Wysoki** — pokrywa rodzine Wav2Vec2, HuBERT, MMS (100+ jezykow)

- [ ] Utworz `stt/candle/wav2vec.rs` — `Wav2VecEngine` z CTC decode
- [ ] Zaimplementuj `is_ctc_arch()` w factory z pelna lista architektur
- [ ] Test: `facebook/wav2vec2-base-960h`
- [ ] Test: `facebook/mms-300m` (wielojezyczny)

---

### Etap 4 — Backend ONNX Runtime

Priorytet: **Wysoki** — kluczowy dla modeli spoza Candle

- [ ] Dodaj feature `onnx` do `Cargo.toml` z crate `ort`
- [ ] Utworz `stt/onnx/mod.rs` + `encoder_decoder.rs` + `ctc.rs`
- [ ] Detekcja stylu ONNX: encoder-decoder vs CTC (sprawdz pliki w folderze)
- [ ] Test: Whisper wyeksportowany przez `optimum-cli` → `encoder_model.onnx`
- [ ] Test: Parakeet z oficjalnym ONNX exportem NVIDIA
- [ ] Test: Moonshine ONNX (juz czesciowo obslugiwane w `scan_models`)

**Weryfikacja:** Folder z `encoder_model.onnx + decoder_model.onnx` → transkrypcja.

---

### Etap 5 — Wbudowany konwerter HF → ONNX

Priorytet: **Wysoki** — demokratyzuje dostep do dowolnego modelu HF

- [ ] Utworz `stt/converter.rs` z `OnnxConverter`
- [ ] Komenda `check_conversion_prereqs` → zwroc status Python + optimum
- [ ] Komenda `install_optimum` → `pip install optimum[exporters]`
- [ ] Komenda `convert_model_to_onnx` → sidecar z progress events
- [ ] Frontend: przycisk „Convert to ONNX" w `ModelsView.tsx`
- [ ] Frontend: progress bar z `listen("conversion-progress", ...)`

**Weryfikacja:** Klikniecie „Convert" na folderze `facebook/mms-1b-all` →
konwersja do ONNX → automatyczne zaladowanie jako aktywny model.

---

### Etap 6 — GGUF support

Priorytet: **Sredni** — popularne skwantyzowane warianty Whisper

- [ ] Zaktualizuj `whisper-rs` do 0.17+ z feature `gguf` (po stabilizacji)
- [ ] Utworz `stt/gguf_whisper.rs`
- [ ] Detekcja `*.gguf` w factory
- [ ] Test: model `.gguf` z repozytorium `ggerganov/whisper.cpp`

---

### Etap 7 — Rozbudowa frontendu

Priorytet: **Sredni**

- [ ] Odznaki formatow per model
- [ ] Filtrowanie i sortowanie listy modeli
- [ ] Panel statusu backendow w `SettingsView.tsx`
- [ ] Zaktualizowane instrukcje dodawania modeli (dla kazdego formatu)
- [ ] `pnpm lint` — zero bledow TypeScript

---

### Etap 8 — Weryfikacja i dokumentacja

- [ ] Matrix testow: `.bin` / `.gguf` / HF Whisper / HF Wav2Vec2 / ONNX
- [ ] Test platform: macOS (Metal) + Linux (CPU) + Windows (CPU)
- [ ] `cargo check --all-features` bez bledow
- [ ] Zaktualizuj `README.md` — lista obslugiwanych formatow i modeli
- [ ] Zaktualizuj `AGENTS.md` — nowe komendy pnpm, nowe moduly

---

## 10. Macierz platform i backendow

| Backend | macOS Metal | Linux CPU | Linux CUDA | Windows CPU |
|---|:---:|:---:|:---:|:---:|
| GGML Whisper | ✅ | ✅ | ✅ Vulkan | ✅ |
| GGUF Whisper | ✅ | ✅ | ✅ Vulkan | ✅ |
| Candle Whisper | ✅ Metal | ✅ | ⚙ +feature | ✅ |
| Candle Wav2Vec2 | ✅ Metal | ✅ | ⚙ +feature | ✅ |
| ONNX Runtime | ✅ CoreML | ✅ | ✅ CUDA EP | ✅ DirectML |
| HF→ONNX Conv. | ✅ +Python | ✅ +Python | ✅ +Python | ✅ +Python |

---

## 11. Nowe zaleznosci Cargo — pelne zestawienie

```toml
[features]
default = []
candle  = ["dep:candle-core", "dep:candle-nn", "dep:candle-transformers", "dep:tokenizers"]
onnx    = ["dep:ort", "dep:ndarray"]

[dependencies]
# --- istniejace (bez zmian) ---
whisper-rs   = { version = "0.16.0", default-features = false }
serde_json   = "1"   # juz obecne

# --- Candle (feature = "candle") ---
candle-core         = { version = "0.8", optional = true }
candle-nn           = { version = "0.8", optional = true }
candle-transformers = { version = "0.8", optional = true }
tokenizers          = { version = "0.21", optional = true,
                        default-features = false, features = ["onig"] }

# --- ONNX Runtime (feature = "onnx") ---
ort     = { version = "2.0", optional = true,
            default-features = false, features = ["load-dynamic"] }
ndarray = { version = "0.16", optional = true }

[target.'cfg(target_os = "macos")'.dependencies]
whisper-rs  = { version = "0.16.0", features = ["metal"] }
candle-core = { version = "0.8", optional = true, features = ["metal"] }

[target.'cfg(target_os = "linux")'.dependencies]
whisper-rs = { version = "0.16.0", features = ["vulkan"] }

[target.'cfg(target_os = "windows")'.dependencies]
whisper-rs = { version = "0.16.0", default-features = false }
```

---

## 12. Ryzyka i mitigacje

| Ryzyko | Prawdopodobienstwo | Mitigacja |
|---|:---:|---|
| `candle-transformers` brak Wav2Vec2 feature extractor | Srednie | Zaimplementuj conv stack recznie lub uzyj ONNX |
| Czas kompilacji wzrosnie 2-3x przy candle | Wysokie | Feature flags — tylko to co potrzeba w default build |
| `ort` wymaga zewnetrznego `libonnxruntime` | Srednie | Uzyj `features = ["download-binaries"]` lub bundluj przez Tauri |
| Python/optimum niedostepny na maszynie uzytkownika | Wysokie | Konwerter jest opcjonalny; GGML i Candle dzialaja bez Pythona |
| Whisper enc-dec ONNX: 2 osobne pliki sesji | Niskie | Znany wzorzec — Moonshine juz tak dziala w obecnym kodzie |
| `pytorch_model.bin` vs GGML `.bin` — kolizja | Srednie | Factory sprawdza katalog + `config.json` przed rozszerzeniem |
| whisper-rs 0.17 niestabilne API | Niskie | GGUF = Etap 7 (nizszy priorytet, pin do commitu) |
| Duze modele (>7B) przekraczaja RAM uzytkownika | Niskie | Ostrzezenie w UI przy modelach >3 GB |

---

## 13. Pokrycie modeli po implementacji

Po ukonczeniu wszystkich etapow aplikacja bedzie obslugiwac:

**Przez Backend Candle (natywne safetensors, bez konwersji):**
- Wszystkie warianty `openai/whisper-*` i `distil-whisper/*`
- `facebook/wav2vec2-*`, `facebook/mms-*`, `facebook/hubert-*`
- `microsoft/wavlm-*`, `microsoft/unispeech-sat-*`
- Setki fine-tunow na HF (jezyk x architektura)

**Przez Backend ONNX (z konwersja przez optimum lub gotowe eksporty):**
- `nvidia/parakeet-*`, `nvidia/canary-*`
- `speechbrain/*` (po eksporcie)
- `facebook/seamless-m4t-*`
- Dosłownie kazdy model ASR z `transformers` po `optimum-cli export onnx`

**Przez Backend GGML/GGUF (istniejace):**
- `ggerganov/whisper.cpp` — skwantyzowane GGML/GGUF
- Lokalne fine-tuny w formacie whisper.cpp

**Szacowane pokrycie HuggingFace ASR:** ~90% popularnych modeli (wg liczby pobran)
