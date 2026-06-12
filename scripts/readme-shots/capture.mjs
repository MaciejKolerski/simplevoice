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

    // Settings: scroll so the Keyboard Shortcuts heading sits cleanly near the top
    if (name === "settings") {
      await page.evaluate(() => {
        const headings = [...document.querySelectorAll("h2, h3")];
        const target = headings.find((h) => /keyboard shortcuts/i.test(h.textContent || ""));
        const scroller = document.querySelector(".view.active");
        if (target && scroller) {
          const top = target.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop;
          scroller.scrollTop = Math.max(0, top - 24);
        }
      });
      await page.waitForTimeout(300);
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
