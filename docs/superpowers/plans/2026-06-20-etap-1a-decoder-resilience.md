# Etap 1a: Whisper decoder resilience (A1/A4/A5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore whisper.cpp's standard decoding resilience (temperature-fallback ladder, no-context, non-speech suppression) and add a conservative output sanitizer, without regressing the Etap 0 baseline.

**Architecture:** Two independent changes. (1) A pure `sanitize_output` helper in `stt/mod.rs`, unit-tested, applied on the local path (`transcribe_with_progress`, per chunk) and the cloud path (`lib.rs`, on the joined text before the truncation marker is appended). (2) Three decoder-param edits in `ggml_whisper.rs`. No new dependencies.

**Tech Stack:** Rust (edition 2021), `whisper-rs` 0.16 `FullParams` setters (all confirmed present: `set_temperature_inc`, `set_no_context`, `set_suppress_nst`).

## Global Constraints

- **Zero new dependencies.** `sanitize_output` is hand-rolled (no `regex`).
- Rust edition 2021; comments default to none, explain *why* only; no emojis.
- `whisper-rs` 0.16.0 confirmed to expose `set_temperature_inc`, `set_no_context`, `set_suppress_nst`, `set_temperature` (verified in the installed crate).
- **The cloud transcription must be sanitized BEFORE the truncation marker is appended** (`lib.rs:1821-1824`). The truncation marker contains square brackets (`[transcription stopped at …]`); sanitizing after the append would strip the marker. This is the one ordering trap in the stage.
- `sanitize_output` is pure and total: never panics, never errors; all-marker input returns `""`.
- Tests pass under default features (`cargo test`). The decoder edits have no unit test (decoder behavior needs hard audio); they are gated by the controller-run no-regression baseline.
- Run all cargo commands from `src-tauri/`.

---

### Task 1: Output sanitizer (`sanitize_output`) + local & cloud wiring

**Files:**
- Modify: `src-tauri/src/stt/mod.rs` (add `sanitize_output` + the marker list; add unit tests to the existing `#[cfg(test)] mod tests`; wire into `transcribe_with_progress`)
- Modify: `src-tauri/src/lib.rs:1821` (sanitize the joined cloud transcription)

**Interfaces:**
- Consumes: nothing new.
- Produces: `crate::stt::sanitize_output(text: &str) -> String` (`pub(crate)`).

- [ ] **Step 1: Add the sanitizer unit tests to `src-tauri/src/stt/mod.rs`**

Inside the existing `#[cfg(test)] mod tests { ... }` block (which already has `use super::*;`), append:

```rust
    #[test]
    fn sanitize_strips_square_bracket_markers() {
        assert_eq!(sanitize_output("hello [BLANK_AUDIO] world"), "hello world");
        assert_eq!(sanitize_output("[ Silence ]"), "");
        assert_eq!(sanitize_output("[Music] hi"), "hi");
    }

    #[test]
    fn sanitize_strips_known_paren_markers_only() {
        assert_eq!(sanitize_output("hi (music) there"), "hi there");
        assert_eq!(sanitize_output("(applause)"), "");
        // a non-marker parenthetical is kept verbatim
        assert_eq!(sanitize_output("note (see below) please"), "note (see below) please");
    }

    #[test]
    fn sanitize_collapses_whitespace_and_trims() {
        assert_eq!(sanitize_output("  a   b  "), "a b");
    }

    #[test]
    fn sanitize_keeps_plain_text_and_real_parens() {
        assert_eq!(sanitize_output("To jest nagranie"), "To jest nagranie");
        assert_eq!(sanitize_output("koszt (netto) wynosi"), "koszt (netto) wynosi");
    }
```

- [ ] **Step 2: Run the tests to confirm they fail (function does not exist)**

Run: `cargo test --lib stt::tests::sanitize`
Expected: FAIL — compile error, `cannot find function sanitize_output`.

- [ ] **Step 3: Implement `sanitize_output` in `src-tauri/src/stt/mod.rs`**

Add near the top of the file, after the `use` line and before `prepare_samples` (it is a sibling pure helper):

```rust
/// Non-speech markers that Whisper sometimes emits inside parentheses. Square-
/// bracketed spans are stripped unconditionally; parenthesized spans are stripped
/// only when their inner text matches one of these (so real dictated parentheticals
/// like "(see below)" are kept).
const NONSPEECH_PAREN_MARKERS: &[&str] = &[
    "blank_audio", "silence", "music", "applause", "laughter", "noise", "inaudible",
];

/// Conservatively removes leftover non-speech artifacts from transcribed text and
/// normalizes whitespace. Total and pure: never panics, never errors; text that is
/// only markers becomes empty. Applied above every engine (local and cloud), so a
/// belt-and-suspenders complement to Whisper's `suppress_nst`.
pub(crate) fn sanitize_output(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '[' {
            // Drop the maximal [...] span; users do not dictate square brackets.
            if let Some(rel) = chars[i + 1..].iter().position(|&ch| ch == ']') {
                i += 1 + rel + 1;
                continue;
            }
        } else if c == '(' {
            if let Some(rel) = chars[i + 1..].iter().position(|&ch| ch == ')') {
                let inner: String = chars[i + 1..i + 1 + rel].iter().collect();
                if NONSPEECH_PAREN_MARKERS.contains(&inner.trim().to_lowercase().as_str()) {
                    i += 1 + rel + 1;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}
```

- [ ] **Step 4: Run the sanitizer tests to confirm they pass**

Run: `cargo test --lib stt::tests::sanitize`
Expected: PASS — 4 tests.

- [ ] **Step 5: Wire the sanitizer into the local path (`transcribe_with_progress`)**

In `src-tauri/src/stt/mod.rs`, the per-chunk success arm currently reads:

```rust
                Ok(text) => {
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        parts.push(text);
                    }
                }
```

Replace the body with sanitization (sanitize trims and normalizes internally):

```rust
                Ok(text) => {
                    let text = sanitize_output(&text);
                    if !text.is_empty() {
                        parts.push(text);
                    }
                }
```

- [ ] **Step 6: Wire the sanitizer into the cloud path (`lib.rs`)**

In `src-tauri/src/lib.rs`, the cloud branch joins chunk results then appends the
truncation marker:

```rust
            let mut joined = parts.join(" ");
            if let Some(secs) = truncated_at {
                joined.push_str(&truncation_marker(&app_handle, secs));
            }
            joined
```

Sanitize the joined transcription BEFORE the marker is appended (the marker itself
contains square brackets and must survive):

```rust
            let mut joined = crate::stt::sanitize_output(&parts.join(" "));
            if let Some(secs) = truncated_at {
                joined.push_str(&truncation_marker(&app_handle, secs));
            }
            joined
```

- [ ] **Step 7: Run the stt tests and build to confirm nothing regressed**

Run: `cargo test --lib stt::` and `cargo build --lib`
Expected: PASS — the new sanitizer tests plus the existing `stt::tests` (the
`FakeEngine` chunk tests still pass: `sanitize_output("part1") == "part1"`, empty
stays empty). Build clean.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/stt/mod.rs src-tauri/src/lib.rs
git commit -m "feat(stt): sanitize non-speech markers from transcription output (A5)"
```

---

### Task 2: Whisper decoder resilience setters (`ggml_whisper.rs`)

**Files:**
- Modify: `src-tauri/src/stt/ggml_whisper.rs` (the `FullParams` setup in `transcribe`, lines ~61-78)

**Interfaces:**
- Consumes: nothing.
- Produces: nothing (behavior change only).

There is no unit test: these are decoder-param changes whose effect only shows on
hard audio. They are verified by `cargo build --lib` here and the controller-run
no-regression baseline in Final verification.

- [ ] **Step 1: Enable temperature fallback (A1)**

In `src-tauri/src/stt/ggml_whisper.rs`, change the sampling strategy from
`best_of: 1` to `best_of: 2`:

```rust
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 2 });
```

and add `set_temperature_inc` immediately after the existing `set_temperature(0.0)`:

```rust
        params.set_temperature(0.0);
        params.set_temperature_inc(0.2);
```

- [ ] **Step 2: Set no-context (A4) and suppress non-speech tokens (A5)**

Change the existing `set_suppress_nst(false)` to `true`:

```rust
        params.set_suppress_nst(true);
```

and add `set_no_context(true)` in the same params block (e.g. immediately after the
existing `params.set_no_speech_thold(0.6);` line):

```rust
        params.set_no_speech_thold(0.6);
        params.set_no_context(true);
```

- [ ] **Step 3: Build to confirm the setters compile against whisper-rs 0.16**

Run: `cargo build --lib`
Expected: builds with no errors (all four setters exist in `whisper-rs` 0.16.0).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/stt/ggml_whisper.rs
git commit -m "feat(whisper): temperature fallback, no_context, suppress_nst (A1/A4/A5)"
```

---

## Final verification (whole stage, controller-run)

- [ ] **Full unit suite**

Run: `cargo test`
Expected: all tests pass (existing 82 + the 4 new sanitizer tests = 86), 2 ignored.

- [ ] **No-regression baseline gate**

Re-run the eval harness across the 4 installed models on `test/output.wav` and
confirm WER stays **0.000 / EXACT** for every model:

```bash
cd src-tauri
MODELS="$HOME/Library/Application Support/com.woro.simplevoice/models"
for m in ggml-small.bin ggml-large-v3-turbo.bin ggml-large-v3.bin csukuangfj--sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8; do
  SV_EVAL_MANIFEST=/Users/woro/Documents/Simple/test/manifest.json \
  SIMPLEVOICE_MODEL="$MODELS/$m" SV_EVAL_GPU=1 \
  SV_EVAL_OUT="/Users/woro/Documents/Simple/test/eval-$m.json" \
  cargo run --quiet --bin eval
done
```

Expected: every model reports `EXACT`, WER 0.000. Any regression to non-zero WER on
this clean clip blocks the stage and must be investigated before merge.

- [ ] **Frontend check (no-op confirmation)**

Run: `pnpm lint`
Expected: passes (no frontend changes).
