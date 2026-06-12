# README Shots Viewport Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix four README view captures that clip the app's right+bottom ~80 px by re-architecting capture.mjs to use an exact-size viewport (no 100vw/100vh inflation), capture a clean raw PNG per view, then composite window chrome in a separate second-pass frame.html page.

**Architecture:** Two-pass approach: (1) raw capture pass — viewport exactly 1280×800, no injected CSS or #root forced sizing, full-page screenshot → `_raw-${name}.png`; (2) chrome pass — open frame.html (a static HTML file with the window chrome baked in) at 1360×880, load the raw PNG via query param, screenshot full page → final `${name}.png`. Intermediates are deleted after each view; `.gitignore` covers `assets/readme/_*`.

**Tech Stack:** Playwright (chromium), Node.js ESM, pnpm, static frame.html served via `file://`.

**Working directory:** `/Users/woro/Documents/Simple/simplevoice` (branch `feat/readme-screenshots`).

---

## File structure

```
scripts/readme-shots/
├── capture.mjs     # MODIFIED: two-pass capture (raw + frame.html chrome)
└── frame.html      # NEW: static chrome wrapper (window, shadow, traffic dots)
assets/readme/
├── _raw-*.png      # intermediates (gitignored, deleted at end of run)
├── usage.png       # final @2x transparent PNG
├── models.png
├── transcriptions.png
└── settings.png
```

---

### Task 1: Create `scripts/readme-shots/frame.html`

**Files:**
- Create: `scripts/readme-shots/frame.html`

- [ ] **Step 1: Write frame.html**

```html
<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>
  html,body{margin:0;padding:0;background:transparent}
  .pad{padding:40px;display:inline-block}
  .win{position:relative;border-radius:12px;overflow:hidden;display:block;
    box-shadow:0 30px 80px -20px rgba(0,0,0,0.55), 0 0 0 1px rgba(255,255,255,0.06)}
  .win img{display:block;width:1280px;height:800px}
  .dots{position:absolute;left:16px;top:14px;display:flex;gap:8px}
  .dots i{width:12px;height:12px;border-radius:50%}
</style></head><body>
<div class="pad"><div class="win">
  <img id="shot" alt="" />
  <div class="dots"><i style="background:#ff5f57"></i><i style="background:#febc2e"></i><i style="background:#28c840"></i></div>
</div></div>
<script>document.getElementById("shot").src = new URLSearchParams(location.search).get("img");</script>
</body></html>
```

File path: `/Users/woro/Documents/Simple/simplevoice/scripts/readme-shots/frame.html`

No commit yet — Task 2 modifies capture.mjs and they commit together.

---

### Task 2: Rewrite `captureView` in `capture.mjs` — two-pass architecture

**Files:**
- Modify: `scripts/readme-shots/capture.mjs`

The current `captureView` function (lines 70–126) does:
1. Opens a context at 1360×880 (intended to include chrome padding)
2. Injects CHROME_CSS style tag (body padding + #root shadow/radius)
3. Forces `#root` width/height to 1280×800 via JS
4. Calls `addTrafficDots`
5. Takes a `clip`-based screenshot (clips to 1360×880)

This causes the 100vw/100vh inflation bug: the app sizes itself to the 1360×880 viewport, then #root is forced smaller, clipping the right/bottom.

The fix:
- **Raw pass**: viewport exactly 1280×800; no CHROME_CSS; no #root sizing; no dots; full-page screenshot → `_raw-${name}.png`
- **Frame pass**: new context 1360×880; navigate to `file://...frame.html?img=file://..._raw-${name}.png`; wait for img load; full-page screenshot → `${name}.png`; delete `_raw-${name}.png`

- [ ] **Step 1: Replace the full `capture.mjs` with the two-pass version**

Replace the entire file `/Users/woro/Documents/Simple/simplevoice/scripts/readme-shots/capture.mjs` with:

```js
// Self-contained README capture: boots vite, mocks Tauri IPC, screenshots the
// real UI with window chrome, writes transparent @2x PNGs to assets/readme/.
// Two-pass: (1) raw 1280x800 viewport capture, (2) frame.html chrome composite.
import { chromium } from "playwright";
import { spawn } from "node:child_process";
import { mkdirSync, unlinkSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { installTauriMock } from "./mock.mjs";
import { CONFIG, DEVICES, MODELS, PERMISSIONS, transcriptions, usageStats } from "./fixtures.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const OUT = resolve(root, "assets/readme");
const FRAME_HTML = resolve(root, "scripts/readme-shots/frame.html");
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

async function settle(page) {
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(600); // view fade-in animation (0.3s) + paint
}

async function captureView(browser, { name, navLabel }) {
  // --- Pass 1: raw capture at exact app size, no chrome ---
  const rawPath = resolve(OUT, `_raw-${name}.png`);
  {
    const ctx = await browser.newContext({
      viewport: { width: 1280, height: 800 },
      deviceScaleFactor: 2,
    });
    const page = await ctx.newPage();

    page.on("console", (msg) => {
      if (msg.type() === "error" || msg.type() === "warn") {
        process.stderr.write(`[${name}] console.${msg.type()}: ${msg.text()}\n`);
      }
    });
    page.on("pageerror", (err) => {
      process.stderr.write(`[${name}] pageerror: ${err.message}\n`);
    });

    await page.addInitScript(installTauriMock, { fixtures, windowLabel: "main" });
    await page.goto(URL);
    await settle(page);

    if (navLabel) {
      await page.getByRole("button", { name: navLabel }).first().click();
      await settle(page);
    }

    // Settings: try un-scrolled first; scroll only if Keyboard Shortcuts not visible
    if (name === "settings") {
      const shortcutsVisible = await page.evaluate(() => {
        const el = document.querySelector('[data-tour="shortcuts-section"]');
        if (!el) return false;
        const rect = el.getBoundingClientRect();
        return rect.top >= 0 && rect.bottom <= window.innerHeight;
      });

      if (!shortcutsVisible) {
        // Scroll the active view container so Keyboard Shortcuts section is ~60px from top
        await page.evaluate(() => {
          const viewEl = document.querySelector(".view.active");
          const shortcuts = document.querySelector('[data-tour="shortcuts-section"]');
          if (viewEl && shortcuts) {
            let el = shortcuts;
            let top = 0;
            while (el && el !== viewEl) {
              top += el.offsetTop;
              el = el.offsetParent;
            }
            viewEl.scrollTop = Math.max(0, top - 60);
          }
        });
        await page.waitForTimeout(200);
      }
      // Log which composition was used
      const usedScroll = !shortcutsVisible;
      process.stderr.write(`[settings] shortcuts visible without scroll: ${!usedScroll}; used scroll: ${usedScroll}\n`);
    }

    await page.screenshot({
      path: rawPath,
      omitBackground: true,
      fullPage: false, // viewport-only (1280x800 exact)
    });
    console.log(`raw ${name}.png captured`);
    await ctx.close();
  }

  // --- Pass 2: chrome composite via frame.html ---
  {
    const ctx = await browser.newContext({
      viewport: { width: 1360, height: 880 },
      deviceScaleFactor: 2,
    });
    const page = await ctx.newPage();
    const frameUrl = `file://${FRAME_HTML}?img=${encodeURIComponent(`file://${rawPath}`)}`;
    await page.goto(frameUrl);
    await page.waitForFunction(
      () => {
        const img = document.getElementById("shot");
        return img && img.complete && img.naturalWidth > 0;
      },
      { timeout: 10000 }
    );
    await page.screenshot({
      path: resolve(OUT, `${name}.png`),
      omitBackground: true,
      fullPage: true,
    });
    console.log(`captured ${name}.png`);
    await ctx.close();
  }

  // Clean up intermediate
  try { unlinkSync(rawPath); } catch {}
}

mkdirSync(OUT, { recursive: true });
const vite = await startVite();
const browser = await chromium.launch();
try {
  await captureView(browser, { name: "usage", navLabel: null });
  await captureView(browser, { name: "models", navLabel: "Models" });
  await captureView(browser, { name: "transcriptions", navLabel: "Transcriptions" });
  await captureView(browser, { name: "settings", navLabel: "Settings" });
} finally {
  await browser.close();
  vite.kill("SIGTERM");
}
console.log("done");
```

- [ ] **Step 2: Verify `.gitignore` covers `assets/readme/_*`**

Run: `grep "assets/readme/_" /Users/woro/Documents/Simple/simplevoice/.gitignore`

If no match, append to `.gitignore`:
```
assets/readme/_*
```

---

### Task 3: Run `pnpm shots` and verify output

**Files:** None modified.

- [ ] **Step 1: Run the capture script**

Run from `/Users/woro/Documents/Simple/simplevoice`: `pnpm shots`

Expected console output (in order):
```
raw usage.png captured
captured usage.png
raw models.png captured
captured models.png
raw transcriptions.png captured
captured transcriptions.png
raw settings.png captured
captured settings.png
done
```

Expected stderr line for settings: `[settings] shortcuts visible without scroll: true; used scroll: false` OR `... used scroll: true` — either is acceptable; record which.

No `_raw-*.png` files should remain in `assets/readme/` after the run.

- [ ] **Step 2: View and verify `usage.png`**

Open `/Users/woro/Documents/Simple/simplevoice/assets/readme/usage.png`.

Pass criteria:
- Right edge: Activity legend fully visible (all labels, no truncation); "Active Model" card fully visible including its right edge and content
- Bottom edge: no clipped rows; the bottom bar of the 7-day chart is complete
- "Selected" navigation button (left sidebar) is fully rendered, right edge intact
- Settings nav item visible and NOT clipped at sidebar bottom
- Traffic dots visible in title bar top-left (red/yellow/green circles)
- Window has rounded corners and drop-shadow
- Fonts are crisp (deviceScaleFactor 2 → sharp at displayed size)

- [ ] **Step 3: View and verify `models.png`**

Open `/Users/woro/Documents/Simple/simplevoice/assets/readme/models.png`.

Pass criteria:
- Right edge of model cards intact (size badges, action buttons fully visible)
- Settings nav item visible at sidebar bottom, not clipped
- Active model indicator visible
- Traffic dots present; rounded corners; sharp fonts

- [ ] **Step 4: View and verify `transcriptions.png`**

Open `/Users/woro/Documents/Simple/simplevoice/assets/readme/transcriptions.png`.

Pass criteria:
- All 6 transcription rows visible; no row clipped at right or bottom
- Rightmost column (duration/word-count badges) fully visible
- Settings nav item visible at sidebar bottom, not clipped
- Traffic dots present; rounded corners; sharp fonts

- [ ] **Step 5: View and verify `settings.png`**

Open `/Users/woro/Documents/Simple/simplevoice/assets/readme/settings.png`.

Pass criteria:
- Keyboard Shortcuts section visible (⌘⇧Space / ⌘⇧C shortcuts shown)
- Right side of settings panel intact (no truncated form controls)
- Settings nav item highlighted/selected in sidebar; other nav items (Usage, Models, Transcriptions) visible above it
- Traffic dots present; rounded corners; sharp fonts
- Record the composition choice: "unscrolled" (shortcuts visible at page load) or "scrolled" (had to scroll into view)

---

### Task 4: Commit

**Files:** All modified files.

- [ ] **Step 1: Stage and commit**

```bash
git add scripts/readme-shots/capture.mjs scripts/readme-shots/frame.html assets/readme/usage.png assets/readme/models.png assets/readme/transcriptions.png assets/readme/settings.png
```

If `.gitignore` was modified: also `git add .gitignore`

```bash
git commit -m "fix(readme-shots): exact-size viewport + second-pass window chrome (no more 100vw clipping)"
```

Expected: commit succeeds; `git log --oneline -1` shows the commit SHA.

---

## Self-review

**Spec coverage:**
1. captureView viewport changed to exact 1280×800 — Task 2 ✓
2. DELETE CHROME_CSS style tag — Task 2 ✓ (removed from new code)
3. DELETE #root forced sizing — Task 2 ✓ (removed from new code)
4. DELETE addTrafficDots from raw pass — Task 2 ✓ (removed from new code)
5. Full-page screenshot to `_raw-${name}.png` — Task 2 ✓
6. New `frame.html` chrome wrapper — Task 1 ✓
7. Frame context 1360×880 dpr 2 — Task 2 ✓
8. `waitForFunction` for img load — Task 2 ✓
9. Full-page screenshot → `${name}.png` — Task 2 ✓
10. Delete `_raw-*.png` intermediates — Task 2 ✓
11. `.gitignore` covers `assets/readme/_*` — Task 2 Step 2 ✓
12. Settings: try unscrolled, scroll only if shortcuts not visible — Task 2 ✓
13. Report settings composition choice — Task 3 Step 5 ✓
14. View all four PNGs and verify — Task 3 ✓
15. Commit with exact message — Task 4 ✓

**Placeholder scan:** No TODOs, no "implement later", no vague steps. All code is complete.

**Type consistency:** `unlinkSync` imported alongside `mkdirSync` from `node:fs` — consistent throughout. `installTauriMock` signature unchanged from `mock.mjs`. Fixture exports unchanged.
