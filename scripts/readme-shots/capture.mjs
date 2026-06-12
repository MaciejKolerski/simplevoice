// Self-contained README capture: boots vite, mocks Tauri IPC, screenshots the
// real UI with window chrome, writes transparent @2x PNGs to assets/readme/.
// Two-pass: (1) raw 1280x800 viewport capture, (2) frame.html chrome composite.
import { chromium } from "playwright";
import { spawn } from "node:child_process";
import { mkdirSync, unlinkSync, statSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { installTauriMock } from "./mock.mjs";
import { readFileSync } from "node:fs";
import { CONFIG, DEVICES, MODELS, PERMISSIONS, transcriptions, usageStats } from "./fixtures.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const OUT = resolve(root, "assets/readme");
const FRAME_HTML = resolve(root, "scripts/readme-shots/frame.html");
const PORT = 5199;
const URL = `http://localhost:${PORT}`;

const appVersion = JSON.parse(readFileSync(resolve(root, "package.json"), "utf8")).version;
const fixtures = {
  appVersion,
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

    // Settings stays UNSCROLLED: the two-column layout means any mid-scroll
    // position slices a card in the other column. The top of the view is the
    // only crop with no cut rows; the README caption matches what it shows.

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
let browser;
try {
  browser = await chromium.launch();
  await captureView(browser, { name: "usage", navLabel: null });
  await captureView(browser, { name: "models", navLabel: "Models" });
  await captureView(browser, { name: "transcriptions", navLabel: "Transcriptions" });
  await captureView(browser, { name: "settings", navLabel: "Settings" });

  // --- recording overlay: pose the REAL RecordingWindowView, then stage it ---
  {
    const overlayRawPath = resolve(OUT, "_overlay-raw.png");
    const ctx = await browser.newContext({
      viewport: { width: 200, height: 180 },
      deviceScaleFactor: 4,
    });
    const page = await ctx.newPage();

    // Install Playwright clock BEFORE goto so the view's setInterval uses fake time.
    await page.clock.install();

    page.on("console", (msg) => {
      process.stderr.write(`[overlay] console.${msg.type()}: ${msg.text()}\n`);
    });
    page.on("pageerror", (err) => {
      process.stderr.write(`[overlay] pageerror: ${err.message}\n`);
    });

    await page.addInitScript(installTauriMock, { fixtures, windowLabel: "recording_window" });
    await page.goto(URL);
    await page.evaluate(() => document.fonts.ready);
    await page.waitForTimeout(300);

    // Fire recording-started to transition status → "recording" and start the timer.
    // The view captures Date.now() as startedAtRef.current at this moment.
    await page.evaluate(() => {
      window.__fireTauriEvent("recording-started", null);
    });

    // Advance fake clock by 7 seconds so the elapsed interval fires 7 times → timer shows 0:07.
    await page.clock.fastForward(7000);

    // Fire amplitude sequence to get bars into mid-speech pose.
    await page.evaluate(async () => {
      const AMPS = [0.04, 0.09, 0.13, 0.11, 0.15, 0.12, 0.16, 0.13];
      for (let i = 0; i < AMPS.length; i++) {
        window.__fireTauriEvent("audio-amplitude", AMPS[i]);
        await new Promise((r) => setTimeout(r, 50));
      }
      // Final settle pose — mid-speech amplitude
      window.__fireTauriEvent("audio-amplitude", 0.155);
    });

    // Let rAF complete a few frames to render the waveform bars at the new amplitude.
    await page.waitForTimeout(200);

    await page.screenshot({
      path: overlayRawPath,
      omitBackground: true,
    });
    console.log("captured _overlay-raw.png");
    await ctx.close();

    // Stage: composite the overlay onto a dark backdrop with a generic Notes window.
    const STAGE_HTML = resolve(root, "scripts/readme-shots/stage.html");
    const stageUrl =
      "file://" +
      STAGE_HTML +
      "?img=" +
      encodeURIComponent("file://" + overlayRawPath);

    // Try DSF 2 first; fall back to DSF 1 if result exceeds 600 KB.
    for (const dsf of [2, 1]) {
      const stageCtx = await browser.newContext({
        viewport: { width: 2400, height: 700 },
        deviceScaleFactor: dsf,
      });
      const stagePage = await stageCtx.newPage();
      await stagePage.goto(stageUrl);
      // Wait for the overlay image to load inside the stage.
      await stagePage.waitForFunction(
        () => {
          const img = document.getElementById("overlay");
          return img && img.complete && img.naturalWidth > 0;
        },
        { timeout: 10000 }
      );
      await stagePage.waitForTimeout(300);
      await stagePage.screenshot({
        path: resolve(OUT, "recording.png"),
        clip: { x: 0, y: 0, width: 2400, height: 700 },
      });
      await stageCtx.close();

      const { size } = statSync(resolve(OUT, "recording.png"));
      const kb = size / 1024;
      console.log(`captured recording.png (DSF ${dsf}, ${Math.round(kb)} KB)`);
      if (dsf === 2 && kb > 600) {
        console.log("recording.png exceeds 600 KB at DSF 2 — retrying at DSF 1");
        continue;
      }
      break;
    }

    // Clean up intermediate
    try { unlinkSync(overlayRawPath); } catch {}
  }
} finally {
  await browser?.close();
  vite.kill("SIGTERM");
}
console.log("done");
