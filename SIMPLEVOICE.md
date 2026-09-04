# SIMPLEVOICE.md

`AGENTS.md` and `CLAUDE.md` both delegate to this file. Treat it as the primary
repository instruction set and the living architecture reference. Read it before
making a change, then inspect the affected code because implementation and config
remain the final source of truth.

Last full repository audit: 2026-09-04, based on committed `main` at `9f8cd5e`.
Concurrent uncommitted work was preserved but is not treated as stable
architecture in this snapshot. Re-verify facts in any area changed after that
baseline.

## Product and toolchain

Simplevoice, also styled SimpleVoice in parts of the codebase, is a privacy-first
desktop speech-to-text and voice-typing application. It records microphone input,
transcribes locally by default, and optionally sends prepared audio chunks to a
user-selected BYOK cloud provider. It can copy, paste, or type the result into the
active application and keeps an on-device history with the source WAV recording.

- Product name: `Simplevoice`
- Package and Rust crate: `simplevoice`
- Bundle identifier: `com.woro.simplevoice`
- Current audited version: `0.1.9`
- Desktop shell: Tauri 2 and Rust 2021
- Frontend: React 19, TypeScript 5.8, Vite 7, Tailwind CSS 4
- Package manager: pnpm only
- Supported targets: macOS, Linux, and Windows
- UI languages: English, Polish, and German
- License: Apache-2.0

Use `pnpm tauri <command>`, never a direct Tauri CLI invocation. The
`scripts/tauri.js` wrapper adds the Cargo bin path, detects the project-local Linux
build toolchain, and applies the short Cargo target path and static CRT settings
needed on Windows.

## Non-negotiable working rules

- Inspect `git status` and the relevant diff before editing. Preserve all user
  changes and keep unrelated changes out of the requested work.
- Do not commit, push, tag, publish a release, update AUR, or open a pull request
  unless the user explicitly requests that action.
- Keep microphone callbacks and the audio consumer non-blocking. Expensive model
  work, file I/O, networking, and UI delivery do not belong on the CPAL callback.
- Treat every value received through Tauri IPC as untrusted, even though the
  current caller is the bundled frontend. Canonicalize and constrain filesystem
  targets before reading, writing, or deleting them.
- Keep credentials out of app files, `localStorage`, frontend responses, logs,
  errors, fixtures, and screenshots. Secret material belongs only in the OS
  keyring.
- Keep all visible UI text in i18n resources. Update `en.json`, `pl.json`, and
  `de.json` together and run the locale parity check.
- Preserve platform-specific behavior behind the existing
  `#[cfg(target_os = "...")]` boundaries. A successful build on one OS does not
  validate another OS.
- Do not reintroduce the removed onboarding, dictionary, voice-search, model
  conversion, OpenCC, or LLM cleanup flows without an explicit product decision.
- Do not use emojis in source, code comments, commit messages, or repository
  documentation.

## Repository map

- `src/main.tsx`: chooses the main or `recording_window` React root and disables
  production webview shortcuts and context menus.
- `src/App.tsx`: frontend orchestration for navigation, engine state, recording
  events, batch and live transcription, output, and history refreshes.
- `src/views/`: Usage, Models, Transcriptions, Settings, and Recording Window
  screens. Views are mounted lazily after their first visit.
- `src/components/`: layout, updater, native-history-audio UI, brand elements, and
  Base UI/shadcn-style primitives.
- `src/context/ConfigContext.tsx`: asynchronous `config.json` cache and serialized,
  debounced whole-document updates.
- `src/lib/byok.ts`: cloud provider catalog, lazy AI SDK adapters, secure binary
  fetch bridge, chunk workers, and cloud completion flow.
- `src/i18n/`: language detection, persisted language handling, tray labels, and
  the three locale files.
- `src/App.css`: Tailwind import, dark design tokens, desktop layout, overlay, and
  reduced-motion behavior.
- `src-tauri/src/lib.rs`: imperative backend shell. It owns Tauri setup and
  commands, state, tray, shortcuts, recording orchestration, output delivery,
  config access, history queries, and platform wiring.
- `src-tauri/src/audio.rs`: CPAL microphone capture, conversion to 16 kHz mono,
  buffering, VAD, WAV persistence, device-loss detection, and the live audio tap.
- `src-tauri/src/stt/`: local ASR abstractions, loaders, engines, chunking,
  downloading, streaming, text cleanup, and the intentionally disabled converter.
- `src-tauri/src/byok.rs`: cloud credential boundary, provider model discovery,
  request validation, direct provider integrations, WAV chunk preparation, and
  completion delivery.
- `src-tauri/src/history_audio.rs`: one native Rodio playback worker for history
  rows, including play, pause, seek, stop, and status.
- `src-tauri/src/media_control.rs`: selective media pause and resume on macOS,
  Windows, and Linux.
- `src-tauri/src/linux_shortcuts.rs`, `evdev_shortcuts.rs`, and
  `wayland_type.rs`: Linux desktop/WM shortcut integration, evdev capture, and
  native Wayland virtual-keyboard text delivery.
- `src-tauri/migrations/01_init.sql`: `transcriptions` and `daily_usage` tables.
- `src-tauri/tauri*.conf.json`, `capabilities/default.json`, `Info.plist`, and
  `Entitlements.plist`: app, window, permission, bundle, and platform config.
- `scripts/`: Tauri wrapper, i18n/layout/history checks, README capture tooling,
  and the macOS icon helper.
- `.github/workflows/`: release and package-publishing automation. At the audited
  commit, `release.yml` handled tag-triggered cross-platform builds and AUR
  publication; inspect the current workflow set before changing release logic.
- `aur/`: source and prebuilt Arch Linux package definitions.
- `assets/` and `src-tauri/icons|sounds/`: brand, README, bundle icon, and sound
  resources.

## Runtime architecture

### Startup and state

The main window starts hidden and the app is tray-oriented. Tauri installs the
single-instance, updater, process, dialog, autostart, global-shortcut, and SQL
plugins; creates shared controllers; opens SQLite; restores GPU and selected model
state; builds the tray; and wires platform integrations. Secondary invocations
forward `--toggle`, `--copy-last`, and `--toggle-bar` actions to the running
instance.

`ActiveConfig` mirrors the frontend's selected engine and cloud provider in Rust
so shortcuts and tray actions can validate recording readiness without depending
on the current view. The historical engine id `openai-cloud` means every cloud
provider, not only OpenAI. Do not rename it without a complete state migration.

### Recording pipeline

`audio.rs` captures a selected or default microphone, not system output. It
prefers a native 16 kHz input configuration, supports CPAL sample formats,
downmixes multichannel input, linearly resamples when required, applies a DC
blocker, and stores mono `f32` samples. The saved WAV is 16-bit PCM, mono, 16 kHz:

`app_local_data_dir/recordings/YYYY-MM-DD_HH-MM-SS/output.wav`

The callback writes to a bounded ring and never waits for transcription. The
consumer updates amplitude, recording state, VAD, the accumulated recording, and
the optional bounded live channel. Overflow is counted and logged once rather
than blocking capture.

VAD auto-stop occurs only after speech has been observed. Live mode disables the
recording-level VAD stop because the streaming segmenter owns utterance
boundaries. A device with no samples for five seconds is treated as disconnected.
Every recording has a hard 5,400-second limit and emits a warning at 5,100
seconds, leaving five minutes before automatic stop.

Recording is allowed when:

- local mode has a loaded, selected, or currently loading model; an idle-unloaded
  selected model is reloaded lazily, or
- cloud mode has all required non-secret routing settings and a non-empty keyring
  credential for the selected provider.

When enabled, system media is paused only after the microphone is resolved. The
app records which sessions it paused and resumes only those sessions.

### Local transcription

`SttController` serializes expensive model loads, owns the selected path and
engine, hands out leases so idle unload cannot occur during work, and can unload a
model after five idle minutes. `load_model` runs through `spawn_blocking`; the
outer Tauri command catches panics and can retry without GPU. Do not duplicate
these guards in React effects or bypass the controller.

Before decoding, leading and trailing near-silence is trimmed and RMS is
normalized. Long audio is split near a quiet window into 45 to 90 second chunks.
Local chunks run sequentially. Fully silent chunks are dropped. If a later chunk
fails, the successful prefix is retained and a localized truncation marker is
added.

Current local format behavior:

| Input | Backend | Audited status |
| --- | --- | --- |
| Whisper GGML `.bin` | `whisper-rs` | Supported and used by the in-app catalog |
| Whisper GGUF `.gguf` | `whisper-rs` | Detected and routed, but compatibility is not covered by the catalog or a real-model regression test |
| Hugging Face Whisper directory | Candle | Supported for safetensors or PyTorch layouts when the `candle` feature is enabled |
| Hugging Face CTC architectures | Candle Wav2Vec wrapper | Detected, but initialization intentionally returns an unsupported error and directs users to ONNX |
| ONNX model directory | `sherpa-onnx` | Supported for transducer models such as Parakeet/Zipformer and Moonshine layouts |
| NeMo `.nemo` | None | Detected only to return an explicit unsupported error |

`convert_model` is a compatibility command that always returns an actionable
error. On-device conversion was removed because it installed unpinned executable
dependencies and used trusted remote code. Do not present conversion as a working
feature. The model UI and format metadata still contain legacy conversion state.

### Live transcription

Live transcription is local-only. Cloud mode always uses batch transcription
after recording stops. The Rust streaming controller installs a bounded 64-item
audio channel, coalesces backlog, and runs LocalAgreement-2 on a dedicated worker.
It re-decodes the growing utterance, commits only text stable across consecutive
hypotheses, and treats CJK, Hangul, and Thai characters as agreement units rather
than whitespace-delimited words.

Committed text is append-only. `transcription-committed` carries the delta that
the frontend may type in order; `transcription-partial` is tentative overlay text
and must never be delivered as final output. `transcription-final` is the complete
session text used for history. The configured utterance cap is clamped to 5 to 120
seconds, and worker finalization waits at most roughly five seconds before
detaching.

In live autopaste mode, `App.tsx` serializes committed `type_text` calls so deltas
cannot interleave. With live autopaste disabled, it types the full final text once.
The frontend also plays the done sound, stores the last transcription, and saves
history for the final event. Keep this path distinct from batch delivery.

### Cloud BYOK transcription

The audited provider set is OpenAI, Groq, Deepgram, AssemblyAI, Speechmatics,
Gladia, Rev AI, ElevenLabs, Together AI, Fireworks AI, DeepInfra, Lemonfox.ai,
Cloudflare Workers AI, Replicate, Hugging Face, Azure AI Speech, Google Cloud
Speech-to-Text, Google AI Studio, and Amazon Transcribe. Keep the Rust catalog,
frontend fallback catalog, adapters, settings UI, and translations synchronized.

`src/lib/byok.ts` lazy-loads compatible AI SDK adapters. Other providers use the
AI SDK transcription interface over the Rust binary bridge or a native Rust
integration. Rust prepares the shared silence-aware chunks; Replicate uses 10 to
20 second limits and Google Cloud uses 30 to 55 second limits. The frontend runs
at most three chunk workers and stops scheduling later chunks after the earliest
failure. Completion keeps the successful ordered prefix and marks truncation.

The bridge accepts only bounded GET/POST requests, strips frontend authorization,
allows only the selected provider's exact HTTPS host and path shape, and injects
the credential in Rust. Request metadata, request bodies, and responses are
bounded. Preserve those checks when adding a provider. Never add a command that
returns a stored credential to JavaScript.

### Batch completion and output

After a non-live recording stops, `App.tsx` chooses local `transcribe_audio` or
the cloud chunk flow. Final batch delivery occurs in Rust so a hidden or App
Nap-throttled webview cannot delay user-facing output. Delivery applies the
enabled repeat/filler/case/trailing-space cleanup, updates the clipboard and last
transcription, uses clipboard-only, paste, or direct typing behavior, optionally
restores the previous text clipboard, plays feedback, and clears transcribing
state. The frontend then persists history and refreshes views.

Text delivery is platform-specific. macOS uses accessibility-backed Enigo work
on the main thread. Linux Wayland uses the native
`zwp_virtual_keyboard_v1` protocol; paste sends a fixed-keymap Ctrl+V, while
direct Unicode typing builds safe printable XKB keymaps in batches. GNOME/Mutter
does not expose this protocol, so the clipboard remains the manual fallback.
There is no runtime dependency on `wtype` or `wl-copy`.

### History, usage, and native playback

SQLite stores transcription metadata and aggregate daily usage. The frontend
pages history and renders 7-day, 30-day, and all-time usage. WAV playback must go
through `history_audio.rs`; it canonicalizes paths under the recordings directory
and uses one native Rodio worker. Do not reintroduce base64 audio transfer or
browser/WebKit audio playback.

## Persistence boundaries

- `app_local_data_dir/config.json`: durable backend/frontend settings. Writes use
  a process mutex, a sibling temporary file, and rename. `gpu_enabled` and
  `active_model_path` are backend-owned keys and must survive frontend whole-file
  saves.
- `app_local_data_dir/models/`: downloaded and imported local models, partial
  downloads, and completion manifests.
- `app_local_data_dir/recordings/`: timestamped WAV directories.
- `app_local_data_dir/logs/`: rolling daily tracing logs.
- `app_config_dir/simplevoice.db`: SQLite transcription metadata and daily usage.
- Webview `localStorage`: UI and routing state, including engine/provider/model,
  shortcuts, selected device, VAD/live flags, language hints, overlay mode,
  sidebar state, and provider-scoped non-secret settings.
- OS keyring service `simplevoice`, account `api_key_<provider>`: API tokens,
  subscription keys, Google service-account JSON, and AWS secret access keys.

Cloudflare account ID, Azure/AWS region, and AWS access key ID are routing values,
not secrets, and may live in provider-scoped `localStorage`. If a new provider
field can authorize requests by itself, treat it as a secret.

## UI and platform contracts

- The main and recording windows share the configured Tauri capability. Minimize
  the permission set and validate custom command inputs in Rust.
- `RecordingWindowView` is a separate transparent window with waveform, timer,
  live committed/tentative text, progress, warning, and lock state.
- `recording_window_mode` is `always`, `recording`, or `never`. The default reset
  position is top-center. Do not repeat the stale bottom-center assumption.
- On macOS the overlay is converted to an `NSPanel`, kept at a high level, and
  uses Cmd-hold polling to temporarily disable click-through for dragging.
- On Linux and Windows the persisted lock toggle controls dragging and
  click-through. The tray and Settings can toggle/reset it.
- Linux full desktop environments use their native shortcut stores. Supported
  WMs prefer evdev when `/dev/input` is readable and otherwise use marked config
  sections that invoke the single running binary. Evdev observes keys rather than
  consuming them and handles push-to-talk release.
- The tray is rebuilt when recording, transcription, model, device, language, or
  menu state changes. Its label and status dot must reflect actual backend state.
- Bundled `start.wav`, `stop.wav`, and `done.wav` are resolved from Tauri
  resources. macOS has system-sound fallbacks and uses `afplay`; Linux uses
  `pw-play`; Windows plays the bundled file through Rodio.
- The updater checks shortly after startup. Package-managed Linux installs are
  directed to `yay -S simplevoice-bin`; other supported builds use the signed
  Tauri updater flow.
- React StrictMode runs effects twice in development. Every listener, timer,
  model load, and backend registration must be idempotent and cleaned up.

## Security and correctness invariants

- Preserve keyring-only credential storage and the BYOK endpoint allowlist.
- Bound audio, network payloads, queues, retries, and worker lifetimes. A slow
  model or provider must not create an unbounded capture queue.
- For filesystem IPC, derive paths from trusted roots or database records. Reject
  traversal, absolute-path escape, symlink escape, and destructive operations
  outside the intended root.
- Do not claim a multi-step database/filesystem mutation is atomic unless it is
  protected by a real transaction and has an explicit file rollback strategy.
- Keep selected-model and engine state coherent across Rust state,
  `config.json`, and the legacy frontend `localStorage` mirror.
- Preserve partial transcription on later-chunk failure and keep progress
  monotonic and ordered.
- Never resume arbitrary media. Resume only a session recorded as paused by this
  app.
- Never log request authorization, credentials, service-account JSON, raw
  clipboard contents, or full transcription text in production diagnostics.

## Known audit findings

These findings were present on 2026-09-04 and were not fixed by the documentation
update. Do not describe the affected behavior as safe or transactional. If a task
touches one of these paths, address the finding or explicitly report why it
remains.

1. `delete_transcription_cmd` trusts a frontend-supplied WAV path and attempts to
   delete it before looking up the row. It does not canonicalize the path under
   the recordings root. Derive the path from the database by `id` and constrain
   it before deletion.
2. `discard_download` validates neither the single-file `files[0]` deletion nor
   the multi-file `repo_id` deletion as strictly as `run_download`. A crafted IPC
   request can escape the models directory. Reuse one canonical safe-relative
   validator for creation and deletion.
3. History save/delete/clear spans filesystem operations and multiple SQL
   statements without a transaction. `INSERT OR IGNORE` can skip a duplicate
   transcription while daily totals are still incremented. Failures can leave
   files, rows, and aggregates inconsistent.
4. `open_folder` is also used to open provider dashboard URLs, but its only input
   check rejects a literal `..` component. It is a broad opener, not a validated
   app-directory-only command. Split trusted URL opening from constrained folder
   opening before relying on it as a security boundary.
5. `save_config` writes malformed incoming JSON verbatim when parsing fails, even
   though readers and comments assume a parseable object. Reject non-object JSON
   instead of persisting it.
6. Tauri's CSP is currently `null`, and both windows receive the same default
   capability. Any frontend injection would therefore reach a wide custom IPC
   surface. Define a restrictive CSP and keep custom command validation as the
   primary boundary.
7. At audited commit `9f8cd5e`, the only tracked GitHub workflow was the `v*`
   release workflow. It did not run lint, tests, locale parity, or a separate
   pre-release quality job, and dependency installation was not frozen. Verify
   newer workflow work before relying on this finding.
8. `pnpm audit --prod` reported 42 advisories at this snapshot, including 19 high.
   Most paths run through build-time tooling declared in `dependencies`, notably
   `shadcn` and `@tailwindcss/vite`; Vite itself also had patched releases
   available. Node modules are not shipped as the Tauri runtime, but the build
   and development supply chain still requires remediation. Re-run the audit
   because advisory data changes over time.
9. The production frontend build succeeds but warns about a roughly 723 kB
   minified main chunk. Keep large provider adapters lazy and measure bundle
   changes.
10. `scripts/build-macos-icon.mjs` refers to a root `Simplevoice.icon` and claims
    bundle hooks/resources that are absent from the audited Tauri config; the
    tracked source is under `assets/`. Treat the helper as stale until it is wired
    and tested.
11. `src-tauri/src/stt/candle/mod.rs` checks `cfg(feature = "cuda")`, but
    `Cargo.toml` declares only `candle`, `onnx`, and `default`. Rust therefore
    emits two `unexpected_cfgs` warnings, and `clippy -D warnings` cannot be a
    clean gate until the feature definition or stale branches are corrected.
12. `cargo fmt --all -- --check` fails across the existing Rust tree. Do not
    claim repository-wide formatting is clean and do not hide a broad mechanical
    rewrite inside a functional commit. Format touched Rust and handle the
    baseline drift in a dedicated change.

## Source comment contract

This section is mandatory. It applies to Rust, TypeScript, TSX, JavaScript, CSS,
SQL, shell and PKGBUILD files, workflows, code-bearing configuration, tests, and
examples. Documentation and localized user-facing strings are not code comments.

- Default to no comment. Prefer a clear name, small function, explicit type, or
  simpler control flow.
- Every comment that is added or modified must be technical and written only in
  English. No non-English code comments are allowed.
- A comment is justified only when it explains a non-obvious reason, invariant,
  safety boundary, concurrency or lifetime rule, performance constraint,
  algorithm, protocol/API contract, or platform workaround.
- Comments that narrate syntax or restate the next line are forbidden.
- Conversational notes, marketing language, task history, change logs, reviewer
  instructions, authorship notes, jokes, and commented-out code are forbidden.
- `TODO`, `FIXME`, `HACK`, and `XXX` comments are forbidden unless they identify
  concrete technical debt and include a traceable issue reference. Prefer fixing
  the issue in the current change.
- Keep an inline comment to one or two short lines. Use a longer English doc
  comment only when a public contract or complex algorithm genuinely needs it.
- When editing a block, remove or rewrite any comment in that block that violates
  these rules. Never preserve a bad comment merely because it predates the task.
- Comments must describe the current code. Do not write phrases such as "now",
  "new", "recently", "fixed", or "previously" unless historical context is
  essential to understanding a compatibility constraint.

Allowed:

```rust
// Drop the lock before joining so the worker can finish without deadlocking.
drop(guard);
```

Forbidden:

```rust
// Now stop the worker.
worker.stop();
```

## Git commit contract

Only create a commit when the user explicitly asks. Before committing, inspect
`git status`, `git diff`, `git diff --cached`, and `git diff --check`. Stage only
the intended logical change and base the message on the staged diff.

Every agent-authored commit message must be in English and use Conventional
Commits with this exact subject shape:

```text
<type>(<scope>): <imperative summary>
```

If no honest scope exists for a genuinely cross-cutting change, use the only
allowed fallback form: `<type>: <imperative summary>`.

Subject rules:

- Allowed types: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `build`, `ci`,
  `chore`, and `revert`.
- Use a narrow lowercase scope when one subsystem is clear, for example `audio`,
  `stt`, `streaming`, `byok`, `history`, `ui`, `i18n`, `linux`, `macos`, `windows`,
  `release`, or `agents`. Omit the scope only for a genuinely cross-cutting change.
- Write an imperative, specific summary in lowercase after the colon. Do not end
  it with a period. Keep the complete subject at 72 characters or fewer.
- Describe one logical change. Do not hide unrelated work behind `and`, `misc`,
  `updates`, `cleanup`, or `WIP`.
- Use `!` before the colon for a breaking change and add a `BREAKING CHANGE:`
  footer that states the migration impact.
- Do not add emojis, AI attribution, `Co-authored-by`, or `Signed-off-by` trailers
  unless the user or repository policy explicitly requires them.

The commit body is mandatory for agent-authored commits. Leave one blank line
after the subject and use these exact sections:

```text
Why:
- <reason or user-visible problem>

What:
- <concrete implementation change>

Verification:
- `<command that passed>`
```

Add more bullets when needed, but keep them factual. Explain why the change was
needed and what behavior changed, not a file-by-file diff. List only commands or
manual checks that actually passed. For anything required but unavailable, write
`- Not run: <specific reason>` instead of implying success. Wrap body prose near
100 characters where practical. Put issue references or breaking-change notes in
footers after the verification section.

Canonical example:

```text
fix(stt): preserve partial text after chunk failure

Why:
- Long recordings should retain successful chunks when a later decode fails.

What:
- Return the completed prefix with a localized truncation marker.
- Keep chunk progress monotonic and ordered.

Verification:
- `cargo test --locked --lib`
- `pnpm lint`
```

For this documentation change, the appropriate subject would be:

```text
docs(agents): refresh repository guidance and commit rules
```

Keep one logical change per commit. Do not amend, rebase, force-push, or push a
commit unless the user explicitly asks for that operation.

## Verification matrix

Run the smallest complete set for the affected risk, and report every skipped or
blocked check. A command name is not proof of its scope: `pnpm lint` currently
runs strict TypeScript only and does not run ESLint.

Baseline for source changes:

```bash
git diff --check
pnpm lint
pnpm check:i18n
pnpm build
```

Rust changes:

```bash
cd src-tauri
cargo fmt --all -- --check
cargo test --locked --lib
cargo check --locked
cargo clippy --locked --all-targets -- -D warnings
```

Targeted checks:

- Locale or visible-copy changes: `pnpm check:i18n`.
- Layout, scrolling, or view-shell changes: `pnpm check:layout`.
- History playback or its IPC contract: `pnpm test:history-audio`.
- Audio, STT, model lifecycle, output, shortcuts, tray, updater, or recording
  overlay changes: run `pnpm tauri dev` and exercise the affected path on every
  relevant target OS.
- Real local-model smoke test:
  `cargo test --locked --test engine_smoke -- --ignored --nocapture` with
  `SV_TEST_MODEL` and `SV_TEST_WAV`.
- Long-audio/chunking integration test:
  `cargo test --locked --test long_audio -- --ignored --nocapture` with
  `SIMPLEVOICE_MODEL` and `SIMPLEVOICE_WAV`.
- Offline quality evaluation: `cargo run --locked --example eval` with
  `SV_EVAL_MANIFEST` and `SIMPLEVOICE_MODEL`; optional `SV_EVAL_GPU` and
  `SV_EVAL_OUT`.
- Dependency changes: `pnpm audit --prod` and an available Rust advisory scanner,
  then review findings for runtime versus build-only reachability.
- Release changes: test `pnpm tauri build` on the affected platform and keep
  version metadata synchronized across package, Cargo, Tauri, updater, and release
  inputs.

Linux Rust builds with the default Vulkan Whisper feature require Clang/libclang
and Vulkan build tools. If they are absent, report the missing prerequisite
verbatim. Never mark `cargo test`, `cargo check`, `clippy`, or a Tauri smoke test
as passed when compilation did not complete.
