# README Screenshots Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Five PNG captures of the real app UI (Playwright + mocked Tauri IPC) embedded in the README as a 2+2+banner HTML table, plus targeted README freshness fixes.

**Architecture:** A self-contained capture script boots the Vite dev server, installs a `window.__TAURI_INTERNALS__` mock (fixtures for every read command, benign no-ops for writes, event firing for the overlay pose), navigates the REAL app, injects window chrome (rounded corners, shadow, macOS traffic dots), and saves transparent @2x PNGs to `assets/readme/`. No app `src/` changes.

**Tech Stack:** Playwright (chromium, devDependency), Node ESM scripts, pnpm. Spec: `docs/superpowers/specs/2026-06-12-readme-screenshots-design.md`.

**Working directory:** `/Users/woro/Documents/Simple/simplevoice` (branch `feat/readme-screenshots`).

---

## File structure

```
scripts/readme-shots/
├── capture.mjs     # orchestrator: vite lifecycle, browser, per-shot flow
├── mock.mjs        # __TAURI_INTERNALS__ init-script factory (pure, serializable)
├── fixtures.mjs    # fixture data builders (usage stats, transcriptions, models, config)
└── stage.html      # staging page for the recording banner (backdrop + overlay <img>)
assets/readme/      # output PNGs (committed)
```

Known invoke inventory (from controller's grep, complete): read-side commands to fixture: `load_config`, `check_permissions_status`, `get_usage_stats`, `get_transcriptions`, `get_model_status`, `get_active_model`, `get_models_dir`, `scan_models`, `list_audio_devices`, `list_cloud_models`, `get_gpu_enabled`, `has_secure_api_key`, `has_last_recording_samples`, `is_recording_window_locked_cmd`. All other commands (write-side) resolve `null`. `plugin:*` commands resolve `null` except `plugin:event|listen` / `plugin:event|unlisten` (callback registry — used to fire `recording-started` / `audio-amplitude` for the overlay pose). Onboarding is gated by `load_config` JSON containing `onboarding_completed: true`.

---

### Task 1: Field-name discovery (read-only, feeds fixtures)

**Files:** none created — produces exact names used in Task 2 code. The engineer MUST run these and adjust the marked fixture fields if names differ.

- [ ] **Step 1: Config keys read by the app**

Run: `grep -rhoE 'config(uration)?\.[a-zA-Z_]+' src/context/ConfigContext.tsx src/views/SettingsView.tsx src/App.tsx | sort -u | head -40` and `sed -n '1,80p' src/context/ConfigContext.tsx`
Record: the config object's key names (e.g. `shortcut`, `copy_shortcut`, `recording_window_mode`, `sound_feedback_enabled`, `pause_audio_on_record`, `ui_language`, `onboarding_completed`, `vad_enabled`, plus any others ConfigContext defaults). The Task 2 `CONFIG` fixture must contain every key SettingsView renders, with sensible macOS defaults.

- [ ] **Step 2: Transcription row shape**

Run: `sed -n '1,60p' src/views/TranscriptionsView.tsx`
Record: the row interface (field names for id, text, timestamp, duration, word count, audio path). Adjust the `transcriptions()` fixture field names accordingly (target content stays as written in Task 2).

- [ ] **Step 3: Models view shapes + commands' args**

Run: `sed -n '1,120p' src/views/ModelsView.tsx` and `grep -n "scan_models\|get_models_dir\|get_active_model\|list_cloud_models\|download_model" src/views/ModelsView.tsx | head -20`
Record: local-model entry shape (name/path/size/format fields), catalog structure (it may be a frontend constant — if the catalog is hardcoded in the view, the fixture only needs `scan_models` results + `get_active_model`), and what `list_cloud_models` returns. Adjust the `MODELS_*` fixtures.

- [ ] **Step 4: Recording-window routing**

Run: `sed -n '1,40p' src/main.tsx` and `grep -n "RecordingWindowView\|getCurrent" src/main.tsx src/App.tsx | head`
Record: how the app decides to render `RecordingWindowView` (expected: current window label `recording` via `@tauri-apps/api/window`, which reads `window.__TAURI_INTERNALS__.metadata.currentWindow.label`). Task 4 sets that metadata label. If routing uses a URL query instead, note the query and use it in Task 4's `page.goto`.

- [ ] **Step 5: Settings permission/devices calls**

Run: `grep -n "check_permissions_status\|list_audio_devices\|has_secure_api_key" src/views/SettingsView.tsx src/components -r | head`
Record: expected response shapes (e.g. `{ platform: "macos", microphone: true, accessibility: true }`; device list of `{ name, id? }`). Adjust fixtures.

No commit (no files changed).

---

### Task 2: Mock + fixtures modules

**Files:**
- Create: `scripts/readme-shots/fixtures.mjs`
- Create: `scripts/readme-shots/mock.mjs`

- [ ] **Step 1: Write `scripts/readme-shots/fixtures.mjs`**

```js
// Fixture data for README captures. Illustrative product-UI values consistent
// with existing brand assets (42m 13s / 48,210 / +12% / Parakeet TDT v3).
// Dates are computed at runtime so "today" is always the last chart bar.

const DAY_SECONDS = [241, 393, 177, 494, 291, 570, 367]; // Mon..Sun ≈ silhouette 38/62/28/78/46/90/58, sum 2533s = 42m13s
const WORDS_PER_SEC = 19.032; // yields ~48,210 words for 2533s

export function isoDaysAgo(n) {
  const d = new Date();
  d.setDate(d.getDate() - n);
  const y = d.getFullYear(), m = String(d.getMonth() + 1).padStart(2, "0"), r = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${r}`;
}

export function usageStats() {
  const daily = [];
  // current week: today = index 6 (Sun position in the chart = last bar)
  for (let i = 6; i >= 0; i--) {
    const sec = DAY_SECONDS[6 - i];
    daily.push({ date: isoDaysAgo(i), time_transcribed_sec: sec, words_generated: Math.round(sec * WORDS_PER_SEC) });
  }
  // previous week: ~12% lower per day -> +12% trend on both cards
  for (let i = 13; i >= 7; i--) {
    const sec = Math.round(DAY_SECONDS[13 - i] / 1.12);
    daily.push({ date: isoDaysAgo(i), time_transcribed_sec: sec, words_generated: Math.round((sec * WORDS_PER_SEC) / 1.0) });
  }
  const total_duration_sec = daily.reduce((s, d) => s + d.time_transcribed_sec, 0) + 14760; // all-time padding
  const total_words = daily.reduce((s, d) => s + d.words_generated, 0) + 281400;
  return { total_transcriptions: 162, total_words, total_duration_sec, daily };
}

// Field names verified in Task 1 Step 2 — adjust if the view's interface differs.
export function transcriptions() {
  const mk = (daysAgo, text, duration_sec) => ({
    id: Math.abs(text.length * 7919 + daysAgo),
    text,
    duration_sec,
    word_count: text.split(/\s+/).length,
    created_at: `${isoDaysAgo(daysAgo)} ${["09:14", "11:32", "14:05", "16:48", "10:21", "13:57"][daysAgo % 6]}:00`,
    wav_path: null,
  });
  return [
    mk(0, "Ship the release notes today, then schedule the launch review for Friday morning.", 9),
    mk(0, "Sounds great — let's lock Friday for the launch review.", 5),
    mk(1, "Draft the changelog before standup and flag anything risky for the desktop build.", 8),
    mk(2, "Remember to test the new recording overlay position controls on the external display.", 9),
    mk(3, "Shipping the fix in ten minutes.", 4),
    mk(5, "Walk through the onboarding flow once more and tighten the copy on the final step.", 8),
  ];
}

export const CONFIG = {
  onboarding_completed: true,
  ui_language: "en",
  shortcut: "CmdOrCtrl+Shift+Space",
  copy_shortcut: "CmdOrCtrl+Shift+C",
  sound_feedback_enabled: true,
  pause_audio_on_record: false,
  recording_window_mode: "recording",
  recording_window_has_custom_pos: false,
  vad_enabled: true,
  // extend with every key found in Task 1 Step 1
};

export const MODELS = {
  modelsDir: "/Users/you/Library/Application Support/com.woro.simplevoice/models",
  active: "parakeet-tdt-0.6b-v3.onnx",
  scan: [
    { name: "parakeet-tdt-0.6b-v3.onnx", size_bytes: 671088640, format: "onnx" },
    { name: "ggml-base.en.bin", size_bytes: 147951465, format: "ggml" },
  ], // shape per Task 1 Step 3
  cloud: ["whisper-1", "gpt-4o-mini-transcribe"],
};

export const DEVICES = [{ name: "MacBook Pro Microphone" }, { name: "AirPods Pro" }]; // shape per Task 1 Step 5
export const PERMISSIONS = { platform: "macos", microphone: true, accessibility: true }; // shape per Task 1 Step 5
```

- [ ] **Step 2: Write `scripts/readme-shots/mock.mjs`**

```js
// Builds the init-script installed via page.addInitScript. Must be fully
// self-contained when serialized: receives plain-JSON payload, no closures.
export function installTauriMock(payload) {
  const { fixtures, windowLabel } = payload;
  const listeners = new Map(); // event -> [callbackId]

  const respond = (cmd, args) => {
    switch (cmd) {
      case "load_config": return JSON.stringify(fixtures.config);
      case "check_permissions_status": return fixtures.permissions;
      case "get_usage_stats": return fixtures.usage;
      case "get_transcriptions": return fixtures.transcriptions;
      case "get_model_status": return { active: fixtures.models.modelsDir + "/" + fixtures.models.active, loading: null };
      case "get_active_model": return fixtures.models.modelsDir + "/" + fixtures.models.active;
      case "get_models_dir": return fixtures.models.modelsDir;
      case "scan_models": return fixtures.models.scan;
      case "list_cloud_models": return fixtures.models.cloud;
      case "list_audio_devices": return fixtures.devices;
      case "get_gpu_enabled": return true;
      case "has_secure_api_key": return false;
      case "has_last_recording_samples": return false;
      case "is_recording_window_locked_cmd": return true;
      default: return null; // all write-side commands succeed silently
    }
  };

  let nextCb = 1000;
  const internals = {
    metadata: {
      currentWindow: { label: windowLabel },
      currentWebview: { label: windowLabel, windowLabel },
    },
    transformCallback(cb) {
      const id = nextCb++;
      window[`_${id}`] = cb;
      return id;
    },
    async invoke(cmd, args = {}) {
      if (cmd === "plugin:event|listen") {
        const ev = args.event;
        if (!listeners.has(ev)) listeners.set(ev, []);
        listeners.get(ev).push(args.handler);
        return nextCb++; // event id
      }
      if (cmd === "plugin:event|unlisten") return null;
      if (cmd.startsWith("plugin:")) return null; // updater check -> no update, etc.
      return respond(cmd, args);
    },
  };
  window.__TAURI_INTERNALS__ = internals;
  window.__fireTauriEvent = (event, payloadData) => {
    for (const id of listeners.get(event) ?? []) {
      const fn = window[`_${id}`];
      if (fn) fn({ event, id: 0, payload: payloadData });
    }
  };
  try {
    localStorage.setItem("asr_engine", "local");
    localStorage.setItem("live_overlay_mode", "full");
  } catch {}
}
```

- [ ] **Step 3: Commit**

```bash
git add scripts/readme-shots/fixtures.mjs scripts/readme-shots/mock.mjs
git commit -m "feat(readme-shots): Tauri IPC mock + illustrative fixtures"
```

---

### Task 3: Capture orchestrator + four view shots

**Files:**
- Create: `scripts/readme-shots/capture.mjs`
- Modify: `package.json` (devDependency `playwright`, script `"shots"`)

- [ ] **Step 1: Install Playwright**

Run: `pnpm add -D playwright && pnpm exec playwright install chromium`
Expected: chromium downloaded (~120 MB), lockfile updated.

- [ ] **Step 2: Write `scripts/readme-shots/capture.mjs`**

```js
// Self-contained README capture: boots vite, mocks Tauri IPC, screenshots the
// real UI with window chrome, writes transparent @2x PNGs to assets/readme/.
import { chromium } from "playwright";
import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { installTauriMock } from "./mock.mjs";
import { CONFIG, DEVICES, MODELS, PERMISSIONS, transcriptions, usageStats } from "./fixtures.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const OUT = resolve(root, "assets/readme");
const PORT = 5199;
const URL = `http://localhost:${PORT}`;

const fixtures = {
  config: CONFIG,
  permissions: PERMISSIONS,
  usage: usageStats(),
  transcriptions: transcriptions(),
  models: MODELS,
  devices: DEVICES,
};

function startVite() {
  const child = spawn("pnpm", ["exec", "vite", "--port", String(PORT), "--strictPort"], {
    cwd: root, stdio: ["ignore", "pipe", "pipe"],
  });
  return new Promise((res, rej) => {
    const onData = (d) => {
      if (String(d).includes("Local:")) res(child);
    };
    child.stdout.on("data", onData);
    child.stderr.on("data", (d) => process.stderr.write(d));
    child.on("exit", (c) => rej(new Error(`vite exited early (${c})`)));
    setTimeout(() => rej(new Error("vite start timeout")), 60000);
  });
}

const CHROME_CSS = `
  html, body { background: transparent !important; }
  body { padding: 40px; }
  #root {
    border-radius: 12px; overflow: hidden; position: relative;
    box-shadow: 0 30px 80px -20px rgba(0,0,0,0.55), 0 0 0 1px rgba(255,255,255,0.06);
  }
`;

async function addTrafficDots(page) {
  await page.evaluate(() => {
    const bar = document.querySelector(".title-bar");
    if (!bar) return;
    const wrap = document.createElement("div");
    wrap.setAttribute("data-shot-dots", "");
    wrap.style.cssText = "position:absolute;left:16px;top:50%;transform:translateY(-50%);display:flex;gap:8px;z-index:99";
    for (const c of ["#ff5f57", "#febc2e", "#28c840"]) {
      const s = document.createElement("span");
      s.style.cssText = `width:12px;height:12px;border-radius:50%;background:${c}`;
      wrap.appendChild(s);
    }
    bar.appendChild(wrap);
  });
}

async function settle(page) {
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(600); // view fade-in animation (0.3s) + paint
}

async function captureView(browser, { name, navLabel }) {
  const ctx = await browser.newContext({
    viewport: { width: 1360, height: 880 }, // 1280x800 app + 40px chrome padding
    deviceScaleFactor: 2,
  });
  const page = await ctx.newPage();
  await page.addInitScript(installTauriMock, { fixtures, windowLabel: "main" });
  await page.goto(URL);
  await page.addStyleTag({ content: CHROME_CSS });
  await page.evaluate(() => {
    const r = document.getElementById("root");
    if (r) { r.style.width = "1280px"; r.style.height = "800px"; }
  });
  await settle(page);
  if (navLabel) {
    await page.getByRole("button", { name: navLabel }).first().click();
    await settle(page);
  }
  await addTrafficDots(page);
  await page.screenshot({
    path: resolve(OUT, `${name}.png`),
    omitBackground: true,
    clip: { x: 0, y: 0, width: 1360, height: 880 },
  });
  console.log(`captured ${name}.png`);
  await ctx.close();
}

mkdirSync(OUT, { recursive: true });
const vite = await startVite();
const browser = await chromium.launch();
try {
  await captureView(browser, { name: "usage", navLabel: null });           // default view
  await captureView(browser, { name: "models", navLabel: "Models" });
  await captureView(browser, { name: "transcriptions", navLabel: "Transcriptions" });
  await captureView(browser, { name: "settings", navLabel: "Settings" });
} finally {
  await browser.close();
  vite.kill("SIGTERM");
}
console.log("done");
```

- [ ] **Step 3: Add the package script**

In `package.json` scripts add: `"shots": "node scripts/readme-shots/capture.mjs"` (keep ordering tidy near `lint`).

- [ ] **Step 4: Run and inspect**

Run: `pnpm shots`
Expected: four `captured *.png` lines; files in `assets/readme/` at 2720×1760.
Then VIEW each PNG. Pass criteria per shot: dark app UI fills the rounded window; traffic dots visible top-left; Inter font (no serif); Usage shows 42m 13s / 48,210 / +12% twice / chart with bright last bar; Models shows the active Parakeet model; Transcriptions shows 6 rows with realistic dates; Settings shows ⌘⇧Space / ⌘⇧C shortcuts, no error toasts, no empty states, no scrollbars. If a section is blank, find the failing invoke via `page.on('console')` (add temporarily) and extend the mock/fixtures — that is the Task 1 inventory contract.

- [ ] **Step 5: Commit**

```bash
git add scripts/readme-shots/capture.mjs package.json pnpm-lock.yaml assets/readme/usage.png assets/readme/models.png assets/readme/transcriptions.png assets/readme/settings.png
git commit -m "feat(readme-shots): capture orchestrator + four real-UI view captures"
```

---

### Task 4: Recording-overlay banner

**Files:**
- Create: `scripts/readme-shots/stage.html`
- Modify: `scripts/readme-shots/capture.mjs` (append banner flow)

- [ ] **Step 1: Write `scripts/readme-shots/stage.html`**

```html
<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>
  html,body{margin:0;padding:0}
  .stage{width:2400px;height:700px;position:relative;overflow:hidden;border-radius:24px;
    background:radial-gradient(120% 95% at 50% 20%,#101010,#000 75%)}
  .notes{position:absolute;left:50%;top:300px;transform:translateX(-50%);width:1280px;
    background:#060606;border:1px solid #1f1f1f;border-radius:14px;
    box-shadow:0 40px 80px -30px rgba(0,0,0,.95);text-align:left}
  .nbar{display:flex;align-items:center;gap:8px;padding:13px 15px;border-bottom:1px solid #141414}
  .nbar i{width:13px;height:13px;border-radius:50%;background:#2a2a2a}
  .nbar span{font:12px "JetBrains Mono",monospace;color:#555;margin-left:10px}
  .nbody{height:240px;padding:22px;font:300 20px/1.6 Inter,system-ui,sans-serif;color:rgba(255,255,255,.95)}
  .caret{display:inline-block;width:2.5px;height:22px;background:#ededed;vertical-align:-3px}
  .pill{position:absolute;left:50%;top:120px;transform:translateX(-50%) scale(2.2);transform-origin:top center}
</style></head><body>
<div class="stage">
  <div class="notes">
    <div class="nbar"><i></i><i></i><i></i><span>Notes — Untitled</span></div>
    <div class="nbody"><span class="caret"></span></div>
  </div>
  <div class="pill"><img id="overlay" alt="" /></div>
</div>
<script>
  const u = new URLSearchParams(location.search).get("img");
  document.getElementById("overlay").src = u;
</script>
</body></html>
```

- [ ] **Step 2: Append the banner flow to `capture.mjs`** (before `console.log("done")`, inside the try block)

```js
  // --- recording overlay: pose the REAL RecordingWindowView, then stage it ---
  {
    const ctx = await browser.newContext({ viewport: { width: 220, height: 200 }, deviceScaleFactor: 4 });
    const page = await ctx.newPage();
    await page.addInitScript(installTauriMock, { fixtures, windowLabel: "recording" }); // label per Task 1 Step 4
    await page.goto(URL);
    await page.evaluate(() => document.fonts.ready);
    await page.waitForTimeout(300);
    await page.evaluate(async () => {
      window.__fireTauriEvent("recording-started", null);
      const AMPS = [0.04, 0.09, 0.13, 0.11, 0.15, 0.12, 0.16, 0.13];
      for (let i = 0; i < AMPS.length; i++) {
        window.__fireTauriEvent("audio-amplitude", AMPS[i]);
        await new Promise((r) => setTimeout(r, 50));
      }
      window.__fireTauriEvent("audio-amplitude", 0.155); // settle pose mid-speech
    });
    // pose the timer at 0:07 by back-dating the recording start the view captured
    await page.waitForTimeout(200);
    await page.screenshot({ path: resolve(OUT, "_overlay-raw.png"), omitBackground: true });
    await ctx.close();

    const stageCtx = await browser.newContext({ viewport: { width: 2400, height: 700 }, deviceScaleFactor: 1 });
    const stagePage = await stageCtx.newPage();
    const stageUrl = "file://" + resolve(root, "scripts/readme-shots/stage.html") +
      "?img=" + encodeURIComponent("file://" + resolve(OUT, "_overlay-raw.png"));
    await stagePage.goto(stageUrl);
    await stagePage.waitForTimeout(400);
    await stagePage.screenshot({ path: resolve(OUT, "recording.png") });
    await stageCtx.close();
  }
```

Timer note: the overlay's elapsed timer starts at the `recording-started` event; after ~0.7 s of posing it reads `0:00`. For the README a `0:00`→ acceptable, but `0:07` reads better: before screenshotting, run `await page.waitForTimeout(...)`? — too slow. Instead fake the clock: `await page.clock.install()` BEFORE goto, then after firing `recording-started`, `await page.clock.fastForward(7000)` and fire the amplitude pose — Playwright's clock API advances `Date.now()` and timers, so the view's interval renders `0:07`. Use that (adjust order: `const page = await ctx.newPage(); await page.clock.install();`).

- [ ] **Step 3: Run, inspect, clean intermediates**

Run: `pnpm shots`
Expected: previous four PNGs regenerate identically + `recording.png` (2400×700): glassy pill with live purple waveform and `0:07` timer floating over the dark stage and Notes window. The pill must show the indigo→violet gradient bars (NOT idle dots) and the timer text. Delete the intermediate: `rm assets/readme/_overlay-raw.png` (and add `assets/readme/_*` to `.gitignore`).

- [ ] **Step 4: Commit**

```bash
git add scripts/readme-shots/stage.html scripts/readme-shots/capture.mjs assets/readme/recording.png .gitignore
git commit -m "feat(readme-shots): recording-overlay banner staged from the live component"
```

---

### Task 5: README update + freshness pass

**Files:**
- Modify: `README.md` (screenshots block, lines ~32–37; version badge if stale)

- [ ] **Step 1: Check the version badge**

Run: `grep -n '"version"' src-tauri/tauri.conf.json package.json | head -3`
If the app version ≠ `0.1.0`, update the README badge line accordingly (`img.shields.io/badge/version-X.Y.Z-1f1f1f`).

- [ ] **Step 2: Replace the screenshots block**

Replace the current centered block:

```html
<div align="center">
  <br/>
  <img src="assets/screenshot-usage.svg" width="840" alt="..." />
  <br/><br/>
  <img src="assets/screenshot-recording.svg" width="660" alt="..." />
</div>
```

with:

```html
## Screenshots

<table>
  <tr>
    <td align="center"><img src="assets/readme/usage.png" alt="Usage dashboard" /><br/><sub>Usage dashboard — time transcribed, words generated, active model and 7-day activity</sub></td>
    <td align="center"><img src="assets/readme/models.png" alt="Model manager" /><br/><sub>Built-in model manager — download, import and switch local models</sub></td>
  </tr>
  <tr>
    <td align="center"><img src="assets/readme/transcriptions.png" alt="Transcription history" /><br/><sub>Full local history — every transcription stored on-device in SQLite</sub></td>
    <td align="center"><img src="assets/readme/settings.png" alt="Settings" /><br/><sub>Settings — global shortcuts, recording overlay, sounds and engines</sub></td>
  </tr>
  <tr>
    <td colspan="2" align="center"><img src="assets/readme/recording.png" alt="Floating recording overlay" /><br/><sub>The floating recording overlay with a live waveform — speak anywhere, text lands in the active app</sub></td>
  </tr>
</table>
```

Placement: where the old block sat (after the intro paragraph, before `## Features`). Note the section heading `## Screenshots` replaces the bare images — add `Screenshots` to the top nav links row (`<a href="#screenshots">Screenshots</a> ·` after Features).

- [ ] **Step 3: Freshness fixes**

Confirm the Configuration section mentions the recording-bar position controls (recent feature, commit 14ae640): under `### Keyboard shortcuts` table add one sentence: `The floating recording bar can be repositioned by dragging; its position controls live under **Settings → Recording & Feedback**.` Keep everything else untouched.

- [ ] **Step 4: Weight + render check**

Run: `du -ch assets/readme/*.png | tail -1`
Expected: total < 2.5 MB. If over: re-capture with `deviceScaleFactor: 1.5` (change one constant in capture.mjs) and re-check.
Render check: view README.md (markdown preview) — table renders 2+2+banner, images load, captions legible.

- [ ] **Step 5: Lint + commit**

Run: `pnpm lint`
Expected: passes (tsc strict — scripts/*.mjs are not type-checked but must not break anything; confirm).

```bash
git add README.md
git commit -m "docs: README screenshots — five live-UI captures in a grid; freshness fixes"
```

---

## Self-review notes (already applied)

- Spec coverage: tooling (T2/T3), fixtures (T2), five panels (T3/T4), table+captions (T5), freshness (T5), verification embedded per task. Invoke inventory = Task 1 + the Task 3 Step 4 blank-section contract.
- Field-name uncertainty is contained: Task 1 records exact names; Task 2 marks every fixture that may need renaming.
- Type consistency: `installTauriMock(payload)` signature matches both call sites; fixture exports match imports in capture.mjs.
```
