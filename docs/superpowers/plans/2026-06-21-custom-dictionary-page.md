# Custom Dictionary Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single `custom_words` settings field with a dedicated "Dictionary" sidebar page holding a list of phrase→action rules (text / current time / current date), migrating the old `custom_words` and preserving decoder biasing + fuzzy correction.

**Architecture:** A new `apply_dictionary_rules` post-processing function in `text.rs` (with `DictionaryRule`/`RuleAction` types) does phrase substitution at the delivery layer. `lib.rs` reads the rules once, derives decoder biasing from `text`-rule values, and migrates legacy `custom_words` when `dictionary_rules` is absent. The frontend adds a `DictionaryView` rule editor wired into the sidebar; the old Settings field is removed.

**Tech Stack:** Rust (whisper-rs/sherpa-onnx backend, serde, chrono), React 19 + TypeScript + Tailwind, react-i18next, lucide-react.

## Global Constraints

- Chat in Polish; all code, comments, and identifiers in English (technical comments only).
- This plan and its spec live on disk for workflow only — **do NOT commit** `docs/` (the repo's `docs/` folder was intentionally removed to keep the public repo clean). Commit only source changes.
- Per-task gate: `cargo test --lib` passes for backend tasks; `pnpm install --frozen-lockfile` then `pnpm lint` passes for frontend tasks.
- Final gate before merge: eval baseline EXACT (0.000) across installed models (harness bypasses the delivery layer, so expect no change — run to confirm).
- Action types v1: `text`, `time`, `date` only (YAGNI). Time format `%H:%M:%S`, date format `%Y-%m-%d`.
- `chrono = "0.4"` is already a dependency — do not add new crates.
- Matching: case-insensitive, word-boundary (whole-token core), multi-word (longest trigger wins), computed at transcription time.

---

### Task 1: Backend — `apply_dictionary_rules` + rule types (TDD)

Adds the substitution logic and types to `text.rs`. `apply_custom_words` stays for now (still called by `lib.rs`); it is removed in Task 2. The new function is exercised by its own tests, so `cargo test --lib` is the gate. (`cargo build` will warn the new function is unused until Task 2 wires it — expected, resolved in Task 2.)

**Files:**
- Modify: `src-tauri/src/stt/text.rs`

**Interfaces:**
- Consumes: `crate::eval::edit_distance(&[char], &[char]) -> usize` (existing).
- Produces:
  - `pub(crate) struct DictionaryRule { pub trigger: String, pub action: RuleAction, pub value: Option<String> }` (derives `Debug, Clone, serde::Deserialize`)
  - `pub(crate) enum RuleAction { Text, Time, Date, Unknown }` (derives `Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize`; `#[serde(rename_all = "lowercase")]`, `Unknown` is `#[serde(other)]`)
  - `pub(crate) fn apply_dictionary_rules(text: &str, rules: &[DictionaryRule], now: chrono::NaiveDateTime) -> String`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `src-tauri/src/stt/text.rs`:

```rust
    fn rule(trigger: &str, action: RuleAction, value: Option<&str>) -> DictionaryRule {
        DictionaryRule {
            trigger: trigger.to_string(),
            action,
            value: value.map(|s| s.to_string()),
        }
    }

    fn now_fixed() -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 6, 21)
            .unwrap()
            .and_hms_opt(15, 0, 39)
            .unwrap()
    }

    #[test]
    fn dict_text_single_word_case_insensitive() {
        let r = vec![rule("chatgpt", RuleAction::Text, Some("ChatGPT"))];
        assert_eq!(apply_dictionary_rules("i use chatgpt daily", &r, now_fixed()), "i use ChatGPT daily");
    }

    #[test]
    fn dict_text_multiword_phrase() {
        let r = vec![rule("czat dżi pi ti", RuleAction::Text, Some("ChatGPT"))];
        assert_eq!(apply_dictionary_rules("powiedz czat dżi pi ti teraz", &r, now_fixed()), "powiedz ChatGPT teraz");
    }

    #[test]
    fn dict_longest_trigger_wins() {
        let r = vec![
            rule("new", RuleAction::Text, Some("NEW")),
            rule("new york", RuleAction::Text, Some("NYC")),
        ];
        assert_eq!(apply_dictionary_rules("i love new york today", &r, now_fixed()), "i love NYC today");
        assert_eq!(apply_dictionary_rules("a new day", &r, now_fixed()), "a NEW day");
    }

    #[test]
    fn dict_word_boundary_no_false_positive() {
        let r = vec![rule("cat", RuleAction::Text, Some("CAT"))];
        assert_eq!(apply_dictionary_rules("category cat", &r, now_fixed()), "category CAT");
    }

    #[test]
    fn dict_preserves_attached_punctuation() {
        let r = vec![rule("kubernetes", RuleAction::Text, Some("Kubernetes"))];
        assert_eq!(apply_dictionary_rules("deploy kubernetes, now", &r, now_fixed()), "deploy Kubernetes, now");
    }

    #[test]
    fn dict_time_and_date() {
        let rt = vec![rule("obecna godzina", RuleAction::Time, None)];
        assert_eq!(apply_dictionary_rules("teraz obecna godzina koniec", &rt, now_fixed()), "teraz 15:00:39 koniec");
        let rd = vec![rule("dzisiejsza data", RuleAction::Date, None)];
        assert_eq!(apply_dictionary_rules("dzisiejsza data", &rd, now_fixed()), "2026-06-21");
    }

    #[test]
    fn dict_fuzzy_for_single_word_text_rules() {
        let r = vec![rule("kubernetes", RuleAction::Text, Some("Kubernetes"))];
        assert_eq!(apply_dictionary_rules("deploy kubernetis today", &r, now_fixed()), "deploy Kubernetes today");
    }

    #[test]
    fn dict_no_fuzzy_for_multiword_triggers() {
        let r = vec![rule("new york", RuleAction::Text, Some("NYC"))];
        // A near-miss of a multi-word trigger must NOT match — multi-word triggers
        // are exact-only; only single-word Text rules get fuzzy snapping. The exact
        // phrase still substitutes mid-sentence.
        assert_eq!(apply_dictionary_rules("visit new yor today", &r, now_fixed()), "visit new yor today");
        assert_eq!(apply_dictionary_rules("visit new york today", &r, now_fixed()), "visit NYC today");
    }

    #[test]
    fn dict_no_fuzzy_for_single_word_time_date_rules() {
        let r = vec![rule("godzina", RuleAction::Time, None)];
        // Fuzzy snapping is Text-only; a near-miss of a Time/Date trigger stays put,
        // while the exact trigger still fires.
        assert_eq!(apply_dictionary_rules("powiedz godzin teraz", &r, now_fixed()), "powiedz godzin teraz");
        assert_eq!(apply_dictionary_rules("powiedz godzina teraz", &r, now_fixed()), "powiedz 15:00:39 teraz");
    }

    #[test]
    fn dict_empty_and_invalid_rules_passthrough() {
        assert_eq!(apply_dictionary_rules("nothing here", &[], now_fixed()), "nothing here");
        let r = vec![
            rule("x", RuleAction::Unknown, Some("Y")),
            rule("z", RuleAction::Text, None),
        ];
        assert_eq!(apply_dictionary_rules("x z stays", &r, now_fixed()), "x z stays");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test --lib dict_`
Expected: FAIL to compile — `cannot find type DictionaryRule` / `apply_dictionary_rules`.

- [ ] **Step 3: Implement the types and function**

Add to `src-tauri/src/stt/text.rs` (after `apply_custom_words`, before the test module):

```rust
/// A single user dictionary rule: a spoken trigger phrase mapped to an action.
/// `value` is the replacement for `Text` rules and ignored for `Time`/`Date`.
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct DictionaryRule {
    pub trigger: String,
    pub action: RuleAction,
    #[serde(default)]
    pub value: Option<String>,
}

/// Dictionary action kind. Unknown values from a newer config are tolerated and
/// skipped (forward-compatible) rather than failing the whole config read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RuleAction {
    Text,
    Time,
    Date,
    #[serde(other)]
    Unknown,
}

/// Lowercased alphanumeric core of a whitespace token (attached punctuation stripped).
fn token_core(token: &str) -> String {
    token.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

/// Replaces spoken trigger phrases with their action result: literal text, the
/// current time (`%H:%M:%S`), or the current date (`%Y-%m-%d`). Matching is
/// case-insensitive, on word boundaries, and multi-word (the longest trigger wins).
/// Single-word `Text` rules also snap near-miss typos via the same fuzzy rule the
/// former `apply_custom_words` used. `now` is injected for deterministic tests.
/// Off when `rules` is empty (the caller skips the call).
pub(crate) fn apply_dictionary_rules(
    text: &str,
    rules: &[DictionaryRule],
    now: chrono::NaiveDateTime,
) -> String {
    struct Prepared {
        cores: Vec<String>,
        replacement: String,
        is_text: bool,
    }

    let mut prepared: Vec<Prepared> = Vec::new();
    for r in rules {
        let cores: Vec<String> = r
            .trigger
            .split_whitespace()
            .map(token_core)
            .filter(|c| !c.is_empty())
            .collect();
        if cores.is_empty() {
            continue;
        }
        let replacement = match r.action {
            RuleAction::Text => match &r.value {
                Some(v) if !v.is_empty() => v.clone(),
                _ => continue,
            },
            RuleAction::Time => now.format("%H:%M:%S").to_string(),
            RuleAction::Date => now.format("%Y-%m-%d").to_string(),
            RuleAction::Unknown => continue,
        };
        prepared.push(Prepared { cores, replacement, is_text: matches!(r.action, RuleAction::Text) });
    }
    if prepared.is_empty() {
        return text.to_string();
    }
    // Longest trigger first so multi-word phrases win over their single-word prefixes.
    prepared.sort_by(|a, b| b.cores.len().cmp(&a.cores.len()));

    // Single-word Text rules are eligible for fuzzy typo-snapping (Text only:
    // time/date triggers are exact, never fuzzy-snapped).
    let fuzzy: Vec<(Vec<char>, &str)> = prepared
        .iter()
        .filter(|p| p.cores.len() == 1 && p.is_text)
        .map(|p| (p.cores[0].chars().collect(), p.replacement.as_str()))
        .collect();

    let words: Vec<&str> = text.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut i = 0;
    while i < words.len() {
        if let Some(p) = prepared.iter().find(|p| {
            let n = p.cores.len();
            i + n <= words.len() && (0..n).all(|k| token_core(words[i + k]) == p.cores[k])
        }) {
            let n = p.cores.len();
            let lead: String = words[i].chars().take_while(|c| !c.is_alphanumeric()).collect();
            let trail: String = {
                let rev: String = words[i + n - 1].chars().rev().take_while(|c| !c.is_alphanumeric()).collect();
                rev.chars().rev().collect()
            };
            out.push(format!("{}{}{}", lead, p.replacement, trail));
            i += n;
            continue;
        }

        let core_chars: Vec<char> = token_core(words[i]).chars().collect();
        if core_chars.len() >= 4 {
            let mut best: Option<(usize, f64)> = None;
            for (idx, (fc, _)) in fuzzy.iter().enumerate() {
                if (core_chars.len() as i64 - fc.len() as i64).abs() > 2 {
                    continue;
                }
                let dist = crate::eval::edit_distance(&core_chars, fc);
                let norm = dist as f64 / core_chars.len().max(fc.len()) as f64;
                if best.map_or(true, |(_, b)| norm < b) {
                    best = Some((idx, norm));
                }
            }
            if let Some((idx, norm)) = best {
                if norm > 0.0 && norm <= 0.25 {
                    // Preserve attached punctuation, mirroring the exact-match branch.
                    let lead: String = words[i].chars().take_while(|c| !c.is_alphanumeric()).collect();
                    let trail: String = {
                        let rev: String = words[i].chars().rev().take_while(|c| !c.is_alphanumeric()).collect();
                        rev.chars().rev().collect()
                    };
                    out.push(format!("{}{}{}", lead, fuzzy[idx].1, trail));
                    i += 1;
                    continue;
                }
            }
        }

        out.push(words[i].to_string());
        i += 1;
    }
    out.join(" ")
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --lib dict_`
Expected: PASS — all 10 `dict_*` tests green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/stt/text.rs
git commit -m "feat(dictionary): add apply_dictionary_rules + rule types"
```

---

### Task 2: Backend — wire rules into `lib.rs`, remove `apply_custom_words`

Reads rules once in `transcribe_audio`, derives decoder biasing from `text`-rule values, swaps the chain step, adds the migrating reader, and removes the now-unused `custom_words` reader and `apply_custom_words` (+ its tests).

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/stt/text.rs` (remove `apply_custom_words` and its 3 tests)

**Interfaces:**
- Consumes: `crate::stt::text::{DictionaryRule, RuleAction, apply_dictionary_rules}` (Task 1).
- Produces: `fn dictionary_rules(app_handle: &tauri::AppHandle) -> Vec<crate::stt::text::DictionaryRule>`.

- [ ] **Step 1: Add the migrating reader**

In `src-tauri/src/lib.rs`, replace the entire `fn custom_words(...)` (currently around lines 531–548) with:

```rust
/// Reads `dictionary_rules` from config.json. Falls back to migrating the legacy
/// `custom_words` array (each word -> a `text` rule) when `dictionary_rules` is
/// absent, so existing installs keep their dictionary behavior without a rewrite.
fn dictionary_rules(app_handle: &tauri::AppHandle) -> Vec<crate::stt::text::DictionaryRule> {
    use crate::stt::text::{DictionaryRule, RuleAction};
    let Ok(dir) = app_handle.path().app_local_data_dir() else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(dir.join("config.json")) else {
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    if let Some(arr) = v.get("dictionary_rules").and_then(|a| a.as_array()) {
        return arr
            .iter()
            .filter_map(|item| serde_json::from_value::<DictionaryRule>(item.clone()).ok())
            .collect();
    }
    if let Some(arr) = v.get("custom_words").and_then(|a| a.as_array()) {
        return arr
            .iter()
            .filter_map(|x| x.as_str())
            .map(|s| DictionaryRule {
                trigger: s.to_string(),
                action: RuleAction::Text,
                value: Some(s.to_string()),
            })
            .collect();
    }
    Vec::new()
}
```

- [ ] **Step 2: Read rules once and derive biasing**

In `src-tauri/src/lib.rs`, replace the two biasing assignments (currently around lines 2234–2242):

```rust
    *crate::stt::ggml_whisper::WHISPER_INITIAL_PROMPT.lock().unwrap() =
        custom_words(&app_handle).join(", ");

    *crate::stt::onnx_engine::ONNX_HOTWORDS.lock().unwrap() =
        custom_words(&app_handle).join("\n");
```

with:

```rust
    // Bias decoding toward the custom dictionary (A3/A7). After migration this
    // equals the former custom_words set, so recognition is unchanged. Only
    // `text`-rule values bias; time/date are dynamic and contribute nothing.
    let dict_rules = dictionary_rules(&app_handle);
    let bias: Vec<String> = dict_rules
        .iter()
        .filter(|r| r.action == crate::stt::text::RuleAction::Text)
        .filter_map(|r| r.value.clone())
        .filter(|v| !v.is_empty())
        .collect();
    *crate::stt::ggml_whisper::WHISPER_INITIAL_PROMPT.lock().unwrap() = bias.join(", ");
    *crate::stt::onnx_engine::ONNX_HOTWORDS.lock().unwrap() = bias.join("\n");
```

- [ ] **Step 3: Swap the chain step**

In `src-tauri/src/lib.rs`, replace the `custom_words` delivery-chain block (currently around lines 2369–2374):

```rust
    let custom = custom_words(&app_handle);
    let text = if !custom.is_empty() {
        crate::stt::text::apply_custom_words(&text, &custom)
    } else {
        text
    };
```

with:

```rust
    // Dictionary rules: substitute spoken trigger phrases with text / current
    // time / current date. Reuses the rules read for biasing above.
    let text = if dict_rules.is_empty() {
        text
    } else {
        crate::stt::text::apply_dictionary_rules(
            &text,
            &dict_rules,
            chrono::Local::now().naive_local(),
        )
    };
```

- [ ] **Step 4: Remove `apply_custom_words` and its tests**

In `src-tauri/src/stt/text.rs`, delete the entire `pub(crate) fn apply_custom_words(...)` function and its doc comment, and delete the three tests `custom_words_exact_casing`, `custom_words_fuzzy_snaps_near_misses`, and `custom_words_no_false_positives` (their behavior is now covered by the `dict_*` tests).

- [ ] **Step 5: Build and test**

Run: `cd src-tauri && cargo test --lib`
Expected: PASS, no warnings about unused `apply_dictionary_rules` or `custom_words` (both the old reader and `apply_custom_words` are gone; the new function is now called from `lib.rs`).

Run: `cd src-tauri && cargo build`
Expected: compiles cleanly (no dead-code warnings).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/stt/text.rs
git commit -m "feat(dictionary): wire dictionary_rules into transcribe_audio, migrate custom_words, drop apply_custom_words"
```

---

### Task 3: Frontend — `DictionaryView` page, navigation, i18n keys

Creates the rule editor view, wires it into the sidebar/router, and adds i18n keys. The repo has no frontend unit tests; the gate is `pnpm lint` + the TypeScript compiler.

**Files:**
- Create: `src/views/DictionaryView.tsx`
- Modify: `src/App.tsx`
- Modify: `src/components/layout/Sidebar.tsx`
- Modify: `src/i18n/locales/en.json`, `src/i18n/locales/pl.json`, `src/i18n/locales/de.json`

**Interfaces:**
- Consumes: `useConfig()` → `{ getConfig, updateConfig }` from `../context/ConfigContext`; UI primitives `Input`, `Button`, `Select`/`SelectTrigger`/`SelectValue`/`SelectContent`/`SelectItem`.
- Produces: `export function DictionaryView()`; config key `dictionary_rules`; view id `"dictionary"`.

- [ ] **Step 1: Create the view**

Create `src/views/DictionaryView.tsx`:

```tsx
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Trash2 } from "lucide-react";
import { useConfig } from "../context/ConfigContext";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

type RuleAction = "text" | "time" | "date";

interface DictionaryRule {
  trigger: string;
  action: RuleAction;
  value: string;
}

const ACTIONS: RuleAction[] = ["text", "time", "date"];

export function DictionaryView() {
  const { t } = useTranslation();
  const { getConfig, updateConfig } = useConfig();

  const [rules, setRules] = useState<DictionaryRule[]>(() => {
    const stored = getConfig("dictionary_rules", null) as
      | { trigger?: string; action?: string; value?: string }[]
      | null;
    if (Array.isArray(stored)) {
      return stored.map((r) => ({
        trigger: r.trigger ?? "",
        action: (ACTIONS.includes(r.action as RuleAction)
          ? (r.action as RuleAction)
          : "text"),
        value: r.value ?? "",
      }));
    }
    // Migrate the legacy `custom_words` array into `text` rules.
    const legacy = (getConfig("custom_words", []) as string[]) || [];
    return legacy.map((w) => ({ trigger: w, action: "text" as RuleAction, value: w }));
  });

  const persist = (next: DictionaryRule[]) => {
    setRules(next);
    updateConfig("dictionary_rules", next);
    // Once the new shape is written, the legacy field is no longer used.
    updateConfig("custom_words", []);
  };

  const addRule = () =>
    persist([...rules, { trigger: "", action: "text", value: "" }]);
  const removeRule = (index: number) =>
    persist(rules.filter((_, i) => i !== index));
  const patchRule = (index: number, patch: Partial<DictionaryRule>) =>
    persist(rules.map((r, i) => (i === index ? { ...r, ...patch } : r)));

  const actionLabels: Record<RuleAction, string> = {
    text: t("dictionary.actionText"),
    time: t("dictionary.actionTime"),
    date: t("dictionary.actionDate"),
  };

  return (
    <div className="flex flex-col animate-[fadeIn_0.3s_ease-out]">
      <div className="mb-6">
        <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
          {t("dictionary.title")}
        </h1>
        <p className="mt-1 text-sm text-muted-foreground">
          {t("dictionary.description")}
        </p>
      </div>

      <section className="flex flex-col gap-3 max-w-2xl">
        {rules.length === 0 && (
          <p className="text-sm text-muted-foreground">{t("dictionary.empty")}</p>
        )}

        {rules.map((rule, i) => (
          <div key={i} className="flex items-center gap-2">
            <Input
              value={rule.trigger}
              onChange={(e) => patchRule(i, { trigger: e.target.value })}
              placeholder={t("dictionary.triggerPlaceholder")}
              className="flex-1"
            />

            <Select
              value={rule.action}
              onValueChange={(v) =>
                patchRule(i, { action: (v ?? "text") as RuleAction })
              }
              items={Object.fromEntries(ACTIONS.map((a) => [a, actionLabels[a]]))}
            >
              <SelectTrigger className="w-36 bg-black">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {ACTIONS.map((a) => (
                  <SelectItem key={a} value={a}>
                    {actionLabels[a]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            {rule.action === "text" && (
              <Input
                value={rule.value}
                onChange={(e) => patchRule(i, { value: e.target.value })}
                placeholder={t("dictionary.valuePlaceholder")}
                className="flex-1"
              />
            )}

            <Button
              variant="ghost"
              size="icon"
              onClick={() => removeRule(i)}
              aria-label={t("dictionary.remove")}
            >
              <Trash2 size={16} />
            </Button>
          </div>
        ))}

        <Button variant="outline" onClick={addRule} className="self-start">
          {t("dictionary.addRule")}
        </Button>
      </section>
    </div>
  );
}
```

- [ ] **Step 2: Add the sidebar nav item**

In `src/components/layout/Sidebar.tsx`, add `Languages` to the lucide import and a nav entry:

```tsx
import { Activity, Box, History, Languages, Settings } from "lucide-react";
```

```tsx
const NAV_ITEMS = [
  { id: "usage", Icon: Activity },
  { id: "models", Icon: Box },
  { id: "transcriptions", Icon: History },
  { id: "dictionary", Icon: Languages },
] as const;
```

- [ ] **Step 3: Wire the view into `App.tsx`**

In `src/App.tsx`:

1. Extend the type (around line 32):

```tsx
type ViewId = "usage" | "models" | "transcriptions" | "settings" | "dictionary";
```

2. Add the import next to the other view imports (around line 18):

```tsx
import { DictionaryView } from "./views/DictionaryView";
```

3. Add a render branch inside `<main className="main-content">` (next to the other `view` divs, around lines 446–458):

```tsx
            <div className={`view ${activeView === "dictionary" ? "active" : ""}`}>
              <DictionaryView />
            </div>
```

(`getTitleName` already capitalizes the id, so it yields "Dictionary" with no change.)

- [ ] **Step 4: Add i18n keys**

In `src/i18n/locales/en.json`, add `"dictionary": "Dictionary"` to the `nav` object, and add a top-level `dictionary` section:

```json
  "dictionary": {
    "title": "Dictionary",
    "description": "Replace spoken trigger phrases with text, the current time, or the current date.",
    "addRule": "Add rule",
    "remove": "Remove",
    "triggerPlaceholder": "Trigger phrase",
    "valuePlaceholder": "Replacement text",
    "actionText": "Text",
    "actionTime": "Current time",
    "actionDate": "Current date",
    "empty": "No rules yet. Add one to get started."
  },
```

In `src/i18n/locales/pl.json`, add `"dictionary": "Słownik"` to `nav`, and:

```json
  "dictionary": {
    "title": "Słownik własny",
    "description": "Zamieniaj wypowiedziane frazy na tekst, aktualną godzinę lub datę.",
    "addRule": "Dodaj regułę",
    "remove": "Usuń",
    "triggerPlaceholder": "Fraza wyzwalająca",
    "valuePlaceholder": "Tekst zamiany",
    "actionText": "Tekst",
    "actionTime": "Aktualna godzina",
    "actionDate": "Aktualna data",
    "empty": "Brak reguł. Dodaj pierwszą, aby zacząć."
  },
```

In `src/i18n/locales/de.json`, add `"dictionary": "Wörterbuch"` to `nav`, and:

```json
  "dictionary": {
    "title": "Wörterbuch",
    "description": "Ersetze gesprochene Auslöser-Phrasen durch Text, die aktuelle Uhrzeit oder das aktuelle Datum.",
    "addRule": "Regel hinzufügen",
    "remove": "Entfernen",
    "triggerPlaceholder": "Auslöser-Phrase",
    "valuePlaceholder": "Ersetzungstext",
    "actionText": "Text",
    "actionTime": "Aktuelle Uhrzeit",
    "actionDate": "Aktuelles Datum",
    "empty": "Noch keine Regeln. Füge eine hinzu, um zu starten."
  },
```

- [ ] **Step 5: Lint and type-check**

Run: `pnpm install --frozen-lockfile && pnpm lint`
Expected: PASS — no unused imports, no type errors, the three locale files stay structurally valid (run any existing i18n/JSON check the lint script includes).

- [ ] **Step 6: Commit**

```bash
git add src/views/DictionaryView.tsx src/App.tsx src/components/layout/Sidebar.tsx src/i18n/locales/en.json src/i18n/locales/pl.json src/i18n/locales/de.json
git commit -m "feat(dictionary): add Dictionary view, sidebar nav, and i18n"
```

---

### Task 4: Frontend — remove `custom_words` from Settings

Removes the old single-field dictionary UI now that the Dictionary page owns it.

**Files:**
- Modify: `src/views/SettingsView.tsx`
- Modify: `src/i18n/locales/en.json`, `src/i18n/locales/pl.json`, `src/i18n/locales/de.json`

**Interfaces:**
- Consumes: nothing new.
- Produces: nothing new (deletion only).

- [ ] **Step 1: Remove the state, handler, and load line**

In `src/views/SettingsView.tsx`, delete:
- the `const [customWords, setCustomWords] = useState("");` declaration (around line 142),
- the load line `setCustomWords(((getConfig("custom_words", []) as string[]) || []).join(", "));` (around line 277),
- the `handleCustomWordsChange` function (around lines 609–613).

- [ ] **Step 2: Remove the SettingRow**

In `src/views/SettingsView.tsx`, delete the `customWords` `SettingRow` block (around lines 1027–1034):

```tsx
          <SettingRow title={t("settings.customWords")} description={t("settings.customWordsDesc")}>
            <Input
              value={customWords}
              onChange={(e) => handleCustomWordsChange(e.target.value)}
              placeholder="ChatGPT, Kubernetes"
              className="w-64"
            />
          </SettingRow>
```

- [ ] **Step 3: Remove the i18n keys**

In each of `src/i18n/locales/{en,pl,de}.json`, remove the `settings.customWords` and `settings.customWordsDesc` keys.

- [ ] **Step 4: Lint and type-check**

Run: `pnpm lint`
Expected: PASS — no unused `customWords`/`Input` import left dangling (verify `Input` is still used elsewhere in `SettingsView.tsx`; if it became unused, remove the import).

- [ ] **Step 5: Commit**

```bash
git add src/views/SettingsView.tsx src/i18n/locales/en.json src/i18n/locales/pl.json src/i18n/locales/de.json
git commit -m "refactor(settings): remove custom_words field (moved to Dictionary page)"
```

---

## Final verification (after all tasks)

- [ ] `cd src-tauri && cargo test --lib` — all pass.
- [ ] `pnpm install --frozen-lockfile && pnpm lint` — pass.
- [ ] Eval baseline EXACT (0.000) across installed models — confirms no transcription regression (the harness bypasses the delivery layer; expect unchanged).
- [ ] Manual smoke (user): open the Dictionary page, add a `text` rule and a `time` rule, dictate, confirm substitution; confirm an existing install's `custom_words` appears pre-migrated.

## Self-Review notes

- **Spec coverage:** config schema (Task 2 reader), types + matching + fuzzy + formats (Task 1), biasing derivation + migration + chain placement + reader removal (Task 2), new page + nav + i18n (Task 3), Settings removal (Task 4), tests (Task 1), gates (each task + final). All spec sections mapped.
- **Type consistency:** `DictionaryRule`/`RuleAction`/`apply_dictionary_rules` signatures identical across Tasks 1–2; `dictionary_rules` config key identical across backend reader (Task 2) and frontend (Task 3); `RuleAction` variants (`text`/`time`/`date`) match the TS `RuleAction` union and i18n action labels.
- **No placeholders:** every code step contains complete code; commands have expected output.
