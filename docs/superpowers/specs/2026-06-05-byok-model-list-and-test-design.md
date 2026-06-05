# BYOK: auto-fetch model list + connection test — design

## Goal

Improve the Cloud (BYOK) tab on the Models page so that:

1. The **model dropdown is populated automatically from the provider's API**
   instead of a hardcoded per-provider list.
2. A **"Test" button** verifies that the configured key/connection works and
   reports success or a clear error.

## Key insight

The user chose **"connection/key only"** for the Test button (not a real
transcription round-trip). Validating the key is done by calling the provider's
**list-models endpoint** — which is the *same* call used to populate the
dropdown. So both features are served by **one backend command**; the Test
button just reports whether that call succeeded (and how many models came back).

## Current state

- Frontend `src/views/ModelsView.tsx`, `openai-cloud` tab: provider `Select`
  (OpenAI / OpenRouter / Gemini / Custom), API-key `Input` (saved to OS keyring
  via `set_secure_api_key`, masked as `••••`), **hardcoded** model `Select`
  items per provider, Base URL `Input`. Selections persist in `localStorage`
  (`asr_provider`, `asr_model`, `asr_custom_model`, `asr_base_url`).
- Backend `src-tauri/src/stt/cloud.rs`: `transcribe_cloud(...)` with per-provider
  branches (Gemini `generateContent`, OpenRouter JSON `audio/transcriptions`,
  OpenAI/custom multipart). Keys live in keyring under `api_key_<provider>`.
- No model-listing and no connection-test capability exist today.
- Anthropic is intentionally absent from the provider dropdown (cannot transcribe).

## Backend design (`cloud.rs` + command in `lib.rs`)

### New command

```
list_cloud_models(provider: String, base_url: Option<String>)
    -> Result<Vec<String>, String>
```

(Registered in the `generate_handler!` block in `lib.rs`, like the other cloud
commands.)

Flow:

1. Read the key from keyring via the existing `get_secure_api_key(provider)`.
   Empty/missing → `Err` with a "set your API key" message. **The secret never
   crosses the JS boundary.** This is consistent because the key field already
   persists to keyring on every change (`saveProviderKey` on `onChange`), so the
   keyring reflects the just-typed key by the time Test/fetch runs.
2. Resolve the endpoint and auth per provider (using the provider default base
   URL when `base_url` is empty, exactly as `transcribe_cloud` does):
   - **OpenAI** (`https://api.openai.com/v1`), **OpenRouter**
     (`https://openrouter.ai/api/v1`), **Custom**: `GET {base}/models` with
     `Authorization: Bearer <key>`; response shape `{ "data": [ { "id": ... } ] }`.
   - **Gemini** (`https://generativelanguage.googleapis.com/v1beta`):
     `GET {base}/models` with header `x-goog-api-key: <key>`; response shape
     `{ "models": [ { "name": "models/<id>", "supportedGenerationMethods": [...] } ] }`.
3. Parse, apply the **ASR filter**, sort, dedupe, return `Vec<String>` of model
   ids.
4. On non-2xx, return a descriptive error including the status and (truncated)
   body so the Test button can show e.g. "401 — invalid key".

### ASR filtering heuristic

- **OpenAI / OpenRouter / Custom**: keep ids whose lowercase form contains
  `whisper`, `transcribe`, or `asr`.
- **Gemini**: keep models whose `supportedGenerationMethods` contains
  `generateContent` (transcription path uses `generateContent`); strip the
  `models/` prefix from `name`.
- **Empty-result safety net (all providers)**: if the filter removes everything,
  return the **full unfiltered list** — protects unusual Custom/self-hosted
  servers whose model ids don't match the heuristic.

### Pure, unit-testable helpers (no network)

- `fn asr_model_filter(id: &str) -> bool`
- `fn parse_openai_models(json: &serde_json::Value) -> Vec<String>`
- `fn parse_gemini_models(json: &serde_json::Value) -> Vec<String>`

These get `#[cfg(test)]` unit tests covering: filter hits/misses, OpenAI `data`
parsing, Gemini `name` prefix-stripping + `generateContent` gating, and the
empty-result fallback.

## Frontend design (`ModelsView.tsx`, `openai-cloud` tab)

### State

- `cloudModels: string[]` — fetched + filtered ids (replaces hardcoded items).
- `modelsLoading: boolean`, `modelsFetchError: string | null`.
- `testing: boolean`, `testResult: { ok: boolean; message: string } | null`.

### Behavior

- **Auto-fetch** `cloudModels` when the Cloud tab is active and a key is present,
  triggered on: tab open, provider change, and key change (**debounced** ~600 ms
  so typing a key doesn't spam requests). Reuse the existing
  `api-keys-changed` / `asr-engine-changed` events already wired in the view.
- **Model `Select`** is populated from `cloudModels`. **Fallback**: when
  `cloudModels` is empty (no key, fetch error, or empty result), fall back to the
  current curated per-provider lists (kept as a `FALLBACK_CLOUD_MODELS` constant).
  The "Custom… (type below)" option and the manual `asr_custom_model` input are
  preserved unchanged.
- **Refresh button** (`RefreshCw` icon) next to the model `Select` → manual
  re-fetch; spins while `modelsLoading`.
- **Test button** next to the API-key field → calls `list_cloud_models` and sets
  `testResult`: ✅ `models.testOk` ("Connection OK — {{count}} models") or ❌
  `models.testFailed` with the backend error. Shows inline next to the button
  (consistent with the existing inline alerts in this view).

### Selection robustness

If the previously selected `asr_model` is not in the freshly fetched list, keep
it selected (it may still be valid / a custom id) — do not silently overwrite the
user's choice. The dropdown shows it even if not in `cloudModels`.

## i18n

Add parallel keys to `en` / `pl` / `de` under `models` (the `check:i18n` script
enforces parity):

- `test`, `testing`, `testOk` (`{{count}}`), `testFailed`
- `refreshModels`, `fetchingModels`, `modelsFetchFailed`
- `usingFallbackModels` (note shown when the curated fallback is in use)

## Error handling

- Missing/empty key → "Set your API key first."
- 401/403 → surfaced verbatim from the provider (status + body) so the user sees
  "invalid key" vs "no access".
- Network/timeout → reqwest error string.
- Non-Test auto-fetch failures are silent except for setting `modelsFetchError`
  and falling back to curated models; the Test button is the explicit-feedback
  path.

## Files touched

- `src-tauri/src/stt/cloud.rs` — add `list_cloud_models` orchestration + pure
  helpers + unit tests.
- `src-tauri/src/lib.rs` — register `list_cloud_models` command (reads keyring).
- `src/views/ModelsView.tsx` — state, auto-fetch, dropdown sourcing + fallback,
  refresh button, Test button.
- `src/i18n/locales/{en,pl,de}.json` — new keys.

## Verification

- `cargo check` + new Rust unit tests (`cargo test` for the cloud helpers).
- `pnpm lint` (tsc strict).
- `node scripts/check-i18n-keys.mjs` (key parity).
- Manual via `pnpm tauri dev`: enter a real key → dropdown auto-fills with ASR
  models; Refresh works; Test shows ✅ with count; a bad key shows ❌ 401; with no
  key the curated fallback list is used.

## Out of scope (YAGNI)

- Real transcription round-trip in the Test button (user chose key/connection only).
- A "show all models" toggle (user chose filtered-only).
- Anthropic transcription (unsupported).
- Caching fetched lists across app restarts.
