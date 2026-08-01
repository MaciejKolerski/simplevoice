# Custom Dictionary Page — Design Spec

**Date:** 2026-06-21
**Status:** Approved (design), pending plan
**Scope:** Replace the single `custom_words` settings field with a dedicated "Dictionary" page that holds a list of phrase→action rules (text / current time / current date), extensible to more action types. The old `custom_words` is migrated into the new model; decoder biasing and fuzzy correction are preserved.

> NOTE: This spec lives on disk for the spec→plan→implementation workflow only. It is intentionally NOT committed — the repo's `docs/` folder was removed to keep the public repo clean.

---

## Goal

A dedicated sidebar view where the user manages a list of rules. Each rule = trigger phrase + action type (dropdown) + optional value. When the trigger phrase is spoken, the transcribed text is substituted according to the action:

| Trigger | Action | Result |
|---|---|---|
| czat dżi pi ti | text | ChatGPT |
| kubernetes | text | Kubernetes |
| obecna godzina | time | 15:00:39 |
| dzisiejsza data | date | 2026-06-21 |

## Decisions (locked)

1. **Separate sidebar page** (new top-level view), not a Settings section.
2. **Replace + migrate**: the new page is the single home of the dictionary. Old `custom_words` migrates to `text` rules. The Settings `custom_words` field is removed. Decoder biasing is derived from the rules; fuzzy correction is preserved for single-word `text` rules.

---

## Config schema

`config.json` gains:

```jsonc
"dictionary_rules": [
  { "trigger": "czat dżi pi ti", "action": "text", "value": "ChatGPT" },
  { "trigger": "kubernetes",     "action": "text", "value": "Kubernetes" },
  { "trigger": "obecna godzina", "action": "time" },
  { "trigger": "dzisiejsza data","action": "date" }
]
```

- `trigger`: spoken phrase (1+ words). Required.
- `action`: `"text" | "time" | "date"`. Unknown values are ignored (forward-compat).
- `value`: replacement string. Required for `text`; ignored for `time`/`date`.

### Migration & precedence

- Backend reader prefers `dictionary_rules` when the key is present.
- When `dictionary_rules` is absent but `custom_words` exists, the reader maps each word `w` → `{ trigger: w, action: "text", value: w }`. This guarantees identical behavior (biasing + correction) even before any UI write.
- Frontend: on first edit on the Dictionary page, write `dictionary_rules` and set `custom_words` to `[]` (cosmetic cleanup; backend already prefers `dictionary_rules`).

---

## Backend (Rust)

### Types (`src-tauri/src/stt/text.rs`)

```rust
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct DictionaryRule {
    pub trigger: String,
    pub action: RuleAction,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RuleAction {
    Text,
    Time,
    Date,
    #[serde(other)]
    Unknown,
}
```

### Substitution function (`src-tauri/src/stt/text.rs`)

```rust
pub(crate) fn apply_dictionary_rules(
    text: &str,
    rules: &[DictionaryRule],
    now: chrono::NaiveDateTime,
) -> String
```

- `now` is injected for deterministic tests; `lib.rs` passes `chrono::Local::now().naive_local()`.
- Time format: `%H:%M:%S` (e.g. `15:00:39`). Date format: `%Y-%m-%d` (e.g. `2026-06-21`). Formats are fixed in v1; the enum/match keeps them extensible.

**Algorithm** (mirrors `apply_formatting_commands`):

1. Pre-process each rule into trigger "core words": `trigger.split_whitespace()`, each token trimmed of leading/trailing non-alphanumeric chars and lowercased. Skip rules whose trigger has no core words, whose action is `Unknown`, or which are `Text` with an empty/missing `value`.
2. Compute each valid rule's replacement once: `Text` → `value`; `Time` → `now.format("%H:%M:%S")`; `Date` → `now.format("%Y-%m-%d")`.
3. Collect single-word `Text` rules separately for fuzzy fallback: `(core_chars, value)`.
4. Tokenize input on whitespace. Walk tokens with index `i`:
   - **Exact phrase match** (longest first): among valid rules sorted by descending trigger-token-count, find the first rule whose `n` core words equal `words[i..i+n]` (compare each token's trimmed-lowercased core, case-insensitive). On match, emit `leading_punct + replacement + trailing_punct` as one output token, where `leading_punct` is the non-alphanumeric prefix of `words[i]` and `trailing_punct` is the non-alphanumeric suffix of `words[i+n-1]`. Advance `i += n`.
   - **Fuzzy fallback** (single token, no exact match): if `words[i]`'s core has ≥4 chars, find the best single-word `Text` rule by normalized edit distance (`crate::eval::edit_distance`, length within 2, normalized distance ∈ (0.0, 0.25]). On match, emit `words[i].replace(core, value)` (preserves attached punctuation). Advance `i += 1`.
   - **No match**: emit `words[i]` unchanged. Advance `i += 1`.
5. Join emitted tokens with a single space.

**Boundary guarantees**: tokens are compared by whole-core equality, so a trigger never matches a substring inside a longer word. Multiple input spaces collapse to one (consistent with the other post-processing steps).

### Removal of `apply_custom_words`

`apply_custom_words` is no longer used after the chain swap. Its fuzzy logic moves into `apply_dictionary_rules` (step 4 fuzzy fallback). Remove `apply_custom_words` and migrate its test cases into the new tests to avoid dead code.

### `lib.rs` wiring

- New reader `fn dictionary_rules(app_handle) -> Vec<DictionaryRule>` implementing the precedence/migration above. Parse `dictionary_rules` array leniently (per-element `serde_json::from_value`, `filter_map` drops malformed entries).
- Read the rules **once** in `transcribe_audio` (replacing the three current `custom_words(...)` reads).
- **Biasing**: derive `bias: Vec<String>` = the `value` of every `Text` rule (non-empty). Set:
  - `WHISPER_INITIAL_PROMPT = bias.join(", ")`
  - `ONNX_HOTWORDS = bias.join("\n")`
  After migration this set equals today's `custom_words`, so recognition is unchanged.
- **Chain placement**: replace the `apply_custom_words` step with `apply_dictionary_rules`, in the same slot:
  `opencc → fillers → dictionary_rules → formatting_commands → sentence_case → llm_cleanup → trailing space`.
  Guard: skip the call when `rules.is_empty()`.
- Remove the old `fn custom_words(app_handle)` reader.

`chrono = "0.4"` is already a dependency.

---

## Frontend (React / TS)

### New view: `src/views/DictionaryView.tsx`

- Uses `useConfig()` (`getConfig`, `updateConfig`, `config`) and `useTranslation()`, following `SettingsView.tsx` patterns.
- State: `rules: { trigger: string; action: "text" | "time" | "date"; value: string }[]`.
- **Load**: read `dictionary_rules` from config. If absent, seed from `custom_words` (`getConfig("custom_words", [])` mapped to `{ trigger: w, action: "text", value: w }`).
- **Render**: a list of rule rows. Each row:
  - `Input` for trigger.
  - `Select` for action with items `text` / `time` / `date` (localized labels).
  - `Input` for value — **rendered only when `action === "text"`** (hidden for time/date).
  - "Remove" button.
  - An "Add rule" button appends an empty `text` rule.
- **Save**: on any change, `updateConfig("dictionary_rules", rules)`. On the first migration write, also `updateConfig("custom_words", [])`.
- Empty/invalid rows are tolerated in the UI; the backend filters them.

### Navigation wiring

- `src/App.tsx`: extend `ViewId` with `"dictionary"`; import `DictionaryView`; add a render branch; add a `getTitleName` case.
- `src/components/layout/Sidebar.tsx`: add `{ id: "dictionary", Icon: BookA }` to `NAV_ITEMS` (import `BookA` from `lucide-react`).
- i18n: add `nav.dictionary` and a `dictionary.*` section to `src/i18n/locales/{en,pl,de}.json`.

### Settings cleanup

Remove from `src/views/SettingsView.tsx`: the `customWords` state, `handleCustomWordsChange`, the load line, and the `customWords` `SettingRow`. Remove `settings.customWords` / `settings.customWordsDesc` from the three locale files.

### i18n keys (en / pl / de)

- `nav.dictionary`: "Dictionary" / "Słownik" / "Wörterbuch"
- `dictionary.title`, `dictionary.description`
- `dictionary.addRule`, `dictionary.remove`
- `dictionary.triggerPlaceholder`, `dictionary.valuePlaceholder`
- `dictionary.actionText`, `dictionary.actionTime`, `dictionary.actionDate`
- `dictionary.empty` (shown when there are no rules)

---

## Action types (v1)

`text`, `time`, `date`. Formats fixed (matching the examples). The `RuleAction` enum + single `match` arm in `apply_dictionary_rules` is the one place to extend (e.g. datetime, weekday, clipboard, custom format) later. YAGNI: nothing beyond text/time/date now.

---

## Testing

### Rust unit tests (`text.rs`)

A fixed `now = NaiveDate::from_ymd_opt(2026, 6, 21).unwrap().and_hms_opt(15, 0, 39).unwrap()`.

1. `text` single word, case-insensitive + casing applied: rule `chatgpt→ChatGPT`, "i use chatgpt daily" → "i use ChatGPT daily".
2. `text` multi-word phonetic: rule `"czat dżi pi ti"→ChatGPT`, "powiedz czat dżi pi ti teraz" → "powiedz ChatGPT teraz".
3. Longest match wins when triggers overlap.
4. Word boundary — no false positive inside a longer word.
5. Punctuation preserved: rule `kubernetes→Kubernetes`, "kubernetes," → "Kubernetes,".
6. `time` → "15:00:39".
7. `date` → "2026-06-21".
8. Fuzzy preserved for single-word `text`: rule `kubernetes→Kubernetes`, "deploy kubernetis today" → "deploy Kubernetes today".
9. No fuzzy for multi-word/phonetic triggers (exact only).
10. Empty rules = passthrough; `Unknown` action skipped; `text` rule with empty value skipped.

### Gates

- `cargo test --lib` passes.
- `pnpm install --frozen-lockfile` then `pnpm lint` passes.
- Eval baseline EXACT (0.000) across installed models. The eval harness calls the engine directly and bypasses the `transcribe_audio` delivery layer, so it is unaffected; run it to confirm no regression.

---

## File list

**Backend**
- `src-tauri/src/stt/text.rs`: add `DictionaryRule`, `RuleAction`, `apply_dictionary_rules` + tests; remove `apply_custom_words`.
- `src-tauri/src/lib.rs`: add `dictionary_rules` reader (with migration); read rules once; derive biasing; swap chain step; remove `custom_words` reader.

**Frontend**
- `src/views/DictionaryView.tsx` (new).
- `src/App.tsx`: `ViewId`, import, render branch, `getTitleName`.
- `src/components/layout/Sidebar.tsx`: `NAV_ITEMS` + `BookA` import.
- `src/views/SettingsView.tsx`: remove `custom_words` row/state/handler.
- `src/i18n/locales/{en,pl,de}.json`: add `nav.dictionary` + `dictionary.*`; remove `settings.customWords*`.
