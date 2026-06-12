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

  // Console and error logging for debugging
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
  // For settings: scroll only the .view.active container to show Keyboard Shortcuts
  if (name === "settings") {
    await page.evaluate(() => {
      const viewEl = document.querySelector(".view.active");
      const shortcuts = document.querySelector('[data-tour="shortcuts-section"]');
      if (viewEl && shortcuts) {
        // offsetTop is relative to offsetParent — walk up to find offset relative to viewEl
        let el = shortcuts;
        let top = 0;
        while (el && el !== viewEl) {
          top += el.offsetTop;
          el = el.offsetParent;
        }
        // Scroll so the section heading is ~60px from the top of the view
        viewEl.scrollTop = Math.max(0, top - 60);
      }
    });
    await page.waitForTimeout(200);
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
