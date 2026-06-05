# BYOK: auto-fetched model list + connection test — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Populate the Cloud (BYOK) model dropdown from the provider's list-models API and add a "Test" button that validates the key/connection via the same call.

**Architecture:** One backend command `list_cloud_models(provider, base_url)` reads the key from keyring, calls the provider's `/models` endpoint, parses + ASR-filters the ids, and returns them. The frontend auto-fetches this list (debounced), falls back to a curated list on error/no-key, and the Test button reports whether the call succeeded. Parsing/filtering live in pure, unit-tested Rust helpers.

**Tech Stack:** Rust (Tauri 2, reqwest 0.12 with `json`, serde_json), React 19 + TypeScript, i18next (en/pl/de), keyring.

**Spec:** `docs/superpowers/specs/2026-06-05-byok-model-list-and-test-design.md`

---

## File Structure

- `src-tauri/src/stt/cloud.rs` — add pure helpers (`asr_model_filter`, `apply_asr_filter`, `sort_dedup`, `parse_openai_models`, `parse_gemini_models`, `truncate`), the async `list_models`, and a `#[cfg(test)]` module. Responsibility: all cloud HTTP + parsing.
- `src-tauri/src/lib.rs` — add the `list_cloud_models` command (keyring read) and register it. Responsibility: command surface + secrets.
- `src/views/ModelsView.tsx` — fetch state/logic, dropdown sourcing + fallback, Refresh + Test buttons. Responsibility: BYOK UI.
- `src/i18n/locales/{en,pl,de}.json` — new `models.*` keys.

---

## Task 1: Backend pure helpers + unit tests (TDD)

**Files:**
- Modify: `src-tauri/src/stt/cloud.rs` (add helpers near the top, after the `use` lines; add a test module at the end)

- [ ] **Step 1: Write the failing tests**

Append to the end of `src-tauri/src/stt/cloud.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn asr_filter_matches_keywords() {
        assert!(asr_model_filter("whisper-1"));
        assert!(asr_model_filter("gpt-4o-transcribe"));
        assert!(asr_model_filter("openai/whisper-large-v3"));
        assert!(asr_model_filter("some-ASR-model"));
        assert!(!asr_model_filter("gpt-4o"));
        assert!(!asr_model_filter("text-embedding-3-small"));
    }

    #[test]
    fn parses_openai_data_ids() {
        let j = json!({"data":[{"id":"whisper-1"},{"id":"gpt-4o"}]});
        assert_eq!(parse_openai_models(&j), vec!["whisper-1", "gpt-4o"]);
    }

    #[test]
    fn openai_missing_data_is_empty() {
        let j = json!({"object":"list"});
        assert!(parse_openai_models(&j).is_empty());
    }

    #[test]
    fn gemini_keeps_generatecontent_and_strips_prefix() {
        let j = json!({"models":[
            {"name":"models/gemini-1.5-flash","supportedGenerationMethods":["generateContent","countTokens"]},
            {"name":"models/embedding-001","supportedGenerationMethods":["embedContent"]}
        ]});
        assert_eq!(parse_gemini_models(&j), vec!["gemini-1.5-flash"]);
    }

    #[test]
    fn asr_filter_empty_fallback_returns_all() {
        let all = vec!["model-a".to_string(), "model-b".to_string()];
        assert_eq!(apply_asr_filter(all.clone()), all);
    }

    #[test]
    fn asr_filter_keeps_only_matches_when_present() {
        let all = vec!["whisper-1".to_string(), "gpt-4o".to_string()];
        assert_eq!(apply_asr_filter(all), vec!["whisper-1".to_string()]);
    }

    #[test]
    fn sort_dedup_orders_and_unifies() {
        let v = vec!["b".to_string(), "a".to_string(), "a".to_string()];
        assert_eq!(sort_dedup(v), vec!["a".to_string(), "b".to_string()]);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail (compile error: helpers undefined)**

Run: `cd src-tauri && cargo test --lib cloud::tests 2>&1 | tail -20`
Expected: FAIL — `cannot find function 'asr_model_filter'` etc.

- [ ] **Step 3: Implement the helpers**

In `src-tauri/src/stt/cloud.rs`, immediately after the existing `use ...;` lines at the top of the file, insert:

```rust
/// Keep model ids that look like speech-to-text models.
fn asr_model_filter(id: &str) -> bool {
    let lower = id.to_lowercase();
    ["whisper", "transcribe", "asr"]
        .iter()
        .any(|kw| lower.contains(kw))
}

/// Apply the ASR keyword filter, but if it removes everything, return the full
/// list (protects unusual custom/self-hosted servers whose ids don't match).
fn apply_asr_filter(all: Vec<String>) -> Vec<String> {
    let filtered: Vec<String> = all.iter().filter(|id| asr_model_filter(id)).cloned().collect();
    if filtered.is_empty() {
        all
    } else {
        filtered
    }
}

fn sort_dedup(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v.dedup();
    v
}

/// Parse model ids from an OpenAI-style `{ "data": [ { "id": ... } ] }` body.
fn parse_openai_models(json: &serde_json::Value) -> Vec<String> {
    json.get("data")
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("id").and_then(|v| v.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Parse Gemini models, keeping only those that support `generateContent`
/// (the method the transcription path uses) and stripping the `models/` prefix.
fn parse_gemini_models(json: &serde_json::Value) -> Vec<String> {
    json.get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|m| {
                    m.get("supportedGenerationMethods")
                        .and_then(|v| v.as_array())
                        .map(|methods| {
                            methods.iter().any(|x| x.as_str() == Some("generateContent"))
                        })
                        .unwrap_or(false)
                })
                .filter_map(|m| m.get("name").and_then(|v| v.as_str()))
                .map(|name| name.strip_prefix("models/").unwrap_or(name).to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Trim and cap an error body so it is safe to surface in the UI.
fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() > max {
        s.chars().take(max).collect::<String>() + "…"
    } else {
        s.to_string()
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib cloud::tests 2>&1 | tail -20`
Expected: PASS — 7 passed.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/stt/cloud.rs
git commit -m "feat(byok): cloud model-list parsing + ASR filter helpers with tests"
```

---

## Task 2: Backend `list_models` + `list_cloud_models` command

**Files:**
- Modify: `src-tauri/src/stt/cloud.rs` (add `pub async fn list_models`)
- Modify: `src-tauri/src/lib.rs` (add `list_cloud_models` command + register in `generate_handler!`)

- [ ] **Step 1: Add `list_models` to `cloud.rs`**

Append (above the `#[cfg(test)]` module) in `src-tauri/src/stt/cloud.rs`:

```rust
/// List available model ids for a BYOK provider by calling its `/models`
/// endpoint. Returns ASR-relevant ids (filtered, sorted, deduped). Errors carry
/// the HTTP status + a truncated body so the UI can show e.g. "401 — ...".
pub async fn list_models(
    provider: &str,
    base_url: Option<&str>,
    api_key: &str,
) -> Result<Vec<String>, String> {
    let provider_str = provider.trim().to_lowercase();
    let base_trimmed = base_url.unwrap_or("").trim();
    let client = reqwest::Client::new();

    if provider_str == "gemini" {
        let base = if base_trimmed.is_empty() {
            "https://generativelanguage.googleapis.com/v1beta"
        } else {
            base_trimmed
        };
        let endpoint = format!("{}/models", base.trim_end_matches('/'));
        let response = client
            .get(&endpoint)
            .header("x-goog-api-key", api_key)
            .send()
            .await
            .map_err(|e| format!("Failed to reach Gemini: {}", e))?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("{} — {}", status, truncate(&body, 300)));
        }
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Gemini models: {}", e))?;
        return Ok(sort_dedup(parse_gemini_models(&json)));
    }

    // OpenAI / OpenRouter / custom (OpenAI-compatible)
    let base = if base_trimmed.is_empty() {
        match provider_str.as_str() {
            "openrouter" => "https://openrouter.ai/api/v1",
            _ => "https://api.openai.com/v1",
        }
    } else {
        base_trimmed
    };
    let endpoint = format!("{}/models", base.trim_end_matches('/'));
    let response = client
        .get(&endpoint)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|e| format!("Failed to reach provider: {}", e))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("{} — {}", status, truncate(&body, 300)));
    }
    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse models: {}", e))?;
    Ok(sort_dedup(apply_asr_filter(parse_openai_models(&json))))
}
```

- [ ] **Step 2: Add the `list_cloud_models` command to `lib.rs`**

In `src-tauri/src/lib.rs`, immediately after the `has_secure_api_key` command (ends near line 1429, the closing `}` of that fn), insert:

```rust
#[tauri::command]
async fn list_cloud_models(
    provider: String,
    base_url: Option<String>,
) -> Result<Vec<String>, String> {
    let key = get_secure_api_key(provider.clone())?;
    if key.trim().is_empty() {
        return Err(format!(
            "API key for {} is missing. Set it above first.",
            provider
        ));
    }
    crate::stt::cloud::list_models(&provider, base_url.as_deref(), &key).await
}
```

- [ ] **Step 3: Register the command**

In `src-tauri/src/lib.rs`, find the `generate_handler!` block and the line `has_secure_api_key,`. Add `list_cloud_models,` right after it:

```rust
            has_secure_api_key,
            list_cloud_models,
            minimize_window,
```

(Anchor: `has_secure_api_key,` is immediately followed by `minimize_window,` today.)

- [ ] **Step 4: Verify it compiles**

Run: `cd src-tauri && cargo check --lib 2>&1 | tail -8`
Expected: `Finished` with no errors.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/stt/cloud.rs src-tauri/src/lib.rs
git commit -m "feat(byok): list_cloud_models command (keyring-backed)"
```

---

## Task 3: i18n keys (en/pl/de)

**Files:**
- Modify: `src/i18n/locales/en.json`, `src/i18n/locales/pl.json`, `src/i18n/locales/de.json`

- [ ] **Step 1: Add keys to `en.json`**

In `src/i18n/locales/en.json`, find the `"models": {` block line `"unknownError": "Unknown error occurred",` and insert directly above it:

```json
    "test": "Test",
    "testing": "Testing…",
    "testOk": "Connection OK — {{count}} models",
    "testFailed": "Test failed",
    "refreshModels": "Refresh models",
    "fetchingModels": "Fetching models…",
    "modelsFetchFailed": "Could not fetch models",
    "usingFallbackModels": "Showing suggested models (no live list).",
```

- [ ] **Step 2: Add keys to `pl.json`**

In `src/i18n/locales/pl.json`, find `"unknownError": "Wystąpił nieznany błąd",` and insert directly above it:

```json
    "test": "Testuj",
    "testing": "Testowanie…",
    "testOk": "Połączenie OK — modeli: {{count}}",
    "testFailed": "Test nie powiódł się",
    "refreshModels": "Odśwież modele",
    "fetchingModels": "Pobieranie modeli…",
    "modelsFetchFailed": "Nie udało się pobrać modeli",
    "usingFallbackModels": "Pokazuję sugerowane modele (brak listy z API).",
```

- [ ] **Step 3: Add keys to `de.json`**

In `src/i18n/locales/de.json`, find `"unknownError": "Ein unbekannter Fehler ist aufgetreten",` and insert directly above it:

```json
    "test": "Testen",
    "testing": "Wird getestet…",
    "testOk": "Verbindung OK — {{count}} Modelle",
    "testFailed": "Test fehlgeschlagen",
    "refreshModels": "Modelle aktualisieren",
    "fetchingModels": "Modelle werden geladen…",
    "modelsFetchFailed": "Modelle konnten nicht geladen werden",
    "usingFallbackModels": "Vorgeschlagene Modelle (keine Live-Liste).",
```

- [ ] **Step 4: Verify key parity**

Run: `node scripts/check-i18n-keys.mjs`
Expected: `i18n key parity OK (237 keys per locale)`

- [ ] **Step 5: Commit**

```bash
git add src/i18n/locales/en.json src/i18n/locales/pl.json src/i18n/locales/de.json
git commit -m "i18n(byok): keys for model fetch + connection test"
```

---

## Task 4: Frontend — fetch state, dropdown sourcing, Test/Refresh UI

**Files:**
- Modify: `src/views/ModelsView.tsx`

- [ ] **Step 1: Add icon imports**

In `src/views/ModelsView.tsx`, the lucide import block currently ends with `Pause,` and `Play,`. Add three icons:

```tsx
  Pause,
  Play,
  PlugZap,
  CircleCheck,
  CircleX,
```

- [ ] **Step 2: Add the fallback-models constant**

Directly after the `modelKey` arrow function (the block that ends with `` `${model.repo_id}::${model.files.join("|")}`; ``), insert:

```tsx
// Curated fallback shown when the provider's live model list is unavailable
// (no key yet, fetch error, or an empty response).
const FALLBACK_CLOUD_MODELS: Record<string, string[]> = {
  openai: ["whisper-1", "gpt-4o-transcribe", "gpt-4o-mini-transcribe"],
  openrouter: ["openai/whisper-large-v3"],
  gemini: ["gemini-1.5-flash", "gemini-1.5-pro", "gemini-2.0-flash-exp"],
  custom: [],
};
```

- [ ] **Step 3: Add fetch/test state**

After the BYOK state block (the line `const [asrBaseUrl, setAsrBaseUrl] = useState<string>(` ... `);`), insert:

```tsx
  // Cloud model-list + connection-test state
  const [cloudModels, setCloudModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState<boolean>(false);
  const [modelsFetchError, setModelsFetchError] = useState<string | null>(null);
  const [testing, setTesting] = useState<boolean>(false);
  const [testResult, setTestResult] = useState<{ ok: boolean; message: string } | null>(null);
```

- [ ] **Step 4: Add fetch + test handlers**

Directly after the `handleSelectEngine` function (ends with its closing `};`), insert:

```tsx
  const fetchCloudModels = async (): Promise<string[]> => {
    setModelsLoading(true);
    setModelsFetchError(null);
    try {
      const list = await invoke<string[]>("list_cloud_models", {
        provider: asrProvider,
        baseUrl: asrBaseUrl,
      });
      setCloudModels(list);
      return list;
    } catch (err: any) {
      setCloudModels([]);
      setModelsFetchError(err?.toString() || t("models.modelsFetchFailed"));
      throw err;
    } finally {
      setModelsLoading(false);
    }
  };

  const handleTestConnection = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      const list = await fetchCloudModels();
      setTestResult({ ok: true, message: t("models.testOk", { count: list.length }) });
    } catch (err: any) {
      setTestResult({ ok: false, message: err?.toString() || t("models.testFailed") });
    } finally {
      setTesting(false);
    }
  };
```

- [ ] **Step 5: Add debounced auto-fetch effect**

Directly after the first `useEffect(() => { ... }, []);` block (the large mount effect that returns the cleanup removing listeners), insert a new effect:

```tsx
  // Auto-fetch the live model list when on the Cloud tab and a key is present.
  // Debounced so typing a key doesn't fire a request per keystroke.
  useEffect(() => {
    if (asrEngine !== "openai-cloud") return;
    if (!providerKey) return; // no key -> keep the curated fallback
    const handle = setTimeout(() => {
      fetchCloudModels().catch(() => {});
    }, 600);
    return () => clearTimeout(handle);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asrEngine, asrProvider, asrBaseUrl, providerKey]);
```

- [ ] **Step 6: Replace the `isCustomModel` definition with model-options-aware logic**

Find:

```tsx
  const isCustomModel = asrModel === "custom" || !KNOWN_MODELS.has(asrModel);
```

Replace with:

```tsx
  const modelOptions =
    cloudModels.length > 0 ? cloudModels : FALLBACK_CLOUD_MODELS[asrProvider] || [];
  const isCustomModel =
    asrModel === "custom" ||
    (!modelOptions.includes(asrModel) && !KNOWN_MODELS.has(asrModel));
```

- [ ] **Step 7: Replace the API-key field's right column with key + Test button**

Find this block (the API-key row's right column):

```tsx
              <div className="flex gap-2 w-72 shrink-0">
                <Input
                  type={showApiKey ? "text" : "password"}
                  value={providerKey}
                  onChange={(e) => {
                    setProviderKey(e.target.value);
                    saveProviderKey(asrProvider, e.target.value);
                  }}
                  placeholder={
                    providerKey === "••••••••••••••••"
                      ? ""
                      : t("models.apiKeyPlaceholder", {
                          provider: asrProvider.toUpperCase(),
                        })
                  }
                  className="flex-1 bg-black font-mono"
                />
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  onClick={() => setShowApiKey(!showApiKey)}
                  title={showApiKey ? t("models.hideKey") : t("models.showKey")}
                >
                  {showApiKey ? <EyeOff size={15} /> : <Eye size={15} />}
                </Button>
              </div>
```

Replace with:

```tsx
              <div className="flex flex-col gap-2 w-72 shrink-0">
                <div className="flex gap-2">
                  <Input
                    type={showApiKey ? "text" : "password"}
                    value={providerKey}
                    onChange={(e) => {
                      setProviderKey(e.target.value);
                      saveProviderKey(asrProvider, e.target.value);
                    }}
                    placeholder={
                      providerKey === "••••••••••••••••"
                        ? ""
                        : t("models.apiKeyPlaceholder", {
                            provider: asrProvider.toUpperCase(),
                          })
                    }
                    className="flex-1 bg-black font-mono"
                  />
                  <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    onClick={() => setShowApiKey(!showApiKey)}
                    title={showApiKey ? t("models.hideKey") : t("models.showKey")}
                  >
                    {showApiKey ? <EyeOff size={15} /> : <Eye size={15} />}
                  </Button>
                </div>
                <div className="flex items-center gap-2">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={handleTestConnection}
                    disabled={testing || !providerKey}
                  >
                    {testing ? (
                      <Loader2 size={13} className="animate-spin" />
                    ) : (
                      <PlugZap size={13} />
                    )}
                    {testing ? t("models.testing") : t("models.test")}
                  </Button>
                  {testResult && (
                    <span
                      className={`flex items-center gap-1 text-[11px] min-w-0 ${
                        testResult.ok ? "text-success" : "text-danger"
                      }`}
                    >
                      {testResult.ok ? (
                        <CircleCheck size={13} className="shrink-0" />
                      ) : (
                        <CircleX size={13} className="shrink-0" />
                      )}
                      <span className="truncate">{testResult.message}</span>
                    </span>
                  )}
                </div>
              </div>
```

- [ ] **Step 8: Replace the model `Select` block with dynamic options + Refresh**

Find this block (the model row's right column — the `<Select value={asrModel} ...>` through its closing `</Select>`):

```tsx
              <Select
                value={asrModel}
                onValueChange={(v) => handleModelChange(v as string)}
              >
                <SelectTrigger className="w-72 bg-black shrink-0">
                  <SelectValue>
                    {(v: string) => (v === "custom" ? t("models.customModel") : v)}
                  </SelectValue>
                </SelectTrigger>
                <SelectContent>
                  {asrProvider === "openai" && (
                    <>
                      <SelectItem value="whisper-1">whisper-1</SelectItem>
                      <SelectItem value="gpt-4o-mini">gpt-4o-mini</SelectItem>
                      <SelectItem value="gpt-4o">gpt-4o</SelectItem>
                    </>
                  )}
                  {asrProvider === "openrouter" && (
                    <>
                      <SelectItem value="openai/whisper-large-v3">
                        openai/whisper-large-v3
                      </SelectItem>
                      <SelectItem value="meta-llama/llama-3.2-11b-vision-instruct:free">
                        meta-llama/llama-3.2-11b-vision-instruct:free
                      </SelectItem>
                      <SelectItem value="deepseek/deepseek-chat">
                        deepseek/deepseek-chat
                      </SelectItem>
                      <SelectItem value="google/gemini-2.0-flash-exp:free">
                        google/gemini-2.0-flash-exp:free
                      </SelectItem>
                    </>
                  )}
                  {asrProvider === "gemini" && (
                    <>
                      <SelectItem value="gemini-1.5-flash">gemini-1.5-flash</SelectItem>
                      <SelectItem value="gemini-1.5-pro">gemini-1.5-pro</SelectItem>
                      <SelectItem value="gemini-2.0-flash-exp">
                        gemini-2.0-flash-exp
                      </SelectItem>
                    </>
                  )}
                  <SelectItem value="custom">{t("models.customTypeBelow")}</SelectItem>
                </SelectContent>
              </Select>
```

Replace with:

```tsx
              <div className="flex flex-col gap-1 w-72 shrink-0">
                <div className="flex items-center gap-2">
                  <Select
                    value={asrModel}
                    onValueChange={(v) => handleModelChange(v as string)}
                  >
                    <SelectTrigger className="flex-1 bg-black">
                      <SelectValue>
                        {(v: string) =>
                          v === "custom" ? t("models.customModel") : v
                        }
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      {modelOptions.map((m) => (
                        <SelectItem key={m} value={m}>
                          {m}
                        </SelectItem>
                      ))}
                      {asrModel &&
                        asrModel !== "custom" &&
                        !modelOptions.includes(asrModel) && (
                          <SelectItem key={asrModel} value={asrModel}>
                            {asrModel}
                          </SelectItem>
                        )}
                      <SelectItem value="custom">
                        {t("models.customTypeBelow")}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                  <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    onClick={() => fetchCloudModels().catch(() => {})}
                    disabled={modelsLoading || !providerKey}
                    title={t("models.refreshModels")}
                  >
                    <RefreshCw
                      size={14}
                      className={modelsLoading ? "animate-spin" : ""}
                    />
                  </Button>
                </div>
                {modelsLoading && (
                  <span className="text-[11px] text-muted">
                    {t("models.fetchingModels")}
                  </span>
                )}
                {!modelsLoading && cloudModels.length === 0 && providerKey && (
                  <span className="text-[11px] text-muted">
                    {t("models.usingFallbackModels")}
                  </span>
                )}
              </div>
```

- [ ] **Step 9: Verify the frontend typechecks**

Run: `pnpm lint 2>&1 | tail -10`
Expected: no output after the `> tsc --noEmit --strict` banner (clean).

- [ ] **Step 10: Commit**

```bash
git add src/views/ModelsView.tsx
git commit -m "feat(byok): auto-fetch model list + connection test in Cloud tab"
```

---

## Task 5: Full verification + manual checklist

**Files:** none (verification only)

- [ ] **Step 1: Rust tests + check**

Run: `cd src-tauri && cargo test --lib cloud::tests 2>&1 | tail -8 && cargo check --lib 2>&1 | tail -4`
Expected: tests PASS; `cargo check` `Finished`.

- [ ] **Step 2: Frontend typecheck + i18n parity**

Run: `pnpm lint && node scripts/check-i18n-keys.mjs`
Expected: tsc clean; `i18n key parity OK`.

- [ ] **Step 3: Manual smoke test**

Run: `pnpm tauri dev`
Then in the app, Models → Cloud tab:
- Enter a valid OpenAI key → model dropdown auto-fills with ASR models (whisper/transcribe); the "Showing suggested models" hint disappears.
- Click **Test** → ✅ "Connection OK — N models".
- Click the **Refresh** icon → list reloads (spinner shows).
- Enter a wrong key → **Test** → ❌ shows "401 — ...".
- Clear the key → dropdown shows the curated fallback list; the hint reappears.
- Switch provider to Gemini with a valid key → dropdown shows `generateContent` models.
- Pick "Custom… (type below)" → the custom model id input still appears and persists.

- [ ] **Step 4: Final commit (only if Step 3 surfaced fixes)**

```bash
git add -A
git commit -m "fix(byok): address manual-test findings"
```

---

## Self-Review notes

- **Spec coverage:** model fetch (Tasks 2/4), ASR filter + empty fallback (Task 1), per-provider endpoints/auth (Task 2), Test button = same call (Task 4 Step 4/7), curated fallback (Task 4 Steps 2/6/8), debounced auto-fetch + refresh (Task 4 Steps 5/8), selection robustness (Task 4 Step 8 extra `SelectItem`), i18n parity (Task 3), keyring-only secret (Task 2 Step 2), unit tests (Task 1) — all covered.
- **Type consistency:** `list_cloud_models(provider, base_url)` ↔ frontend `invoke("list_cloud_models", { provider, baseUrl })` (Tauri snake↔camel); `list_models(provider, base_url, api_key)`; helpers return `Vec<String>` ↔ `string[]`.
- **i18n count:** `testOk` uses `{{count}}`. Parity count (237) assumes 8 new keys added to each of the 3 locales over the current 229.
