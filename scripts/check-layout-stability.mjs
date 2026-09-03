import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import {
  CONFIG,
  DEVICES,
  MODELS,
  PERMISSIONS,
  transcriptions,
  usageStats,
} from "./readme-shots/fixtures.mjs";
import { installTauriMock } from "./readme-shots/mock.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const port = 41000 + (process.pid % 20000);
const url = `http://127.0.0.1:${port}`;
const appVersion = JSON.parse(
  readFileSync(resolve(root, "package.json"), "utf8"),
).version;
const fixtures = {
  appVersion,
  config: CONFIG,
  permissions: PERMISSIONS,
  usage: usageStats(),
  transcriptions: transcriptions(),
  models: MODELS,
  devices: DEVICES,
};

async function waitForVite(viteProcess) {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (viteProcess.exitCode !== null) {
      throw new Error(`Vite exited early (${viteProcess.exitCode})`);
    }

    try {
      const response = await fetch(url);
      if (response.ok) return;
    } catch {}

    await new Promise((resolvePromise) => setTimeout(resolvePromise, 100));
  }

  throw new Error("Vite did not start within 10 seconds");
}

async function stopVite(viteProcess) {
  if (viteProcess.exitCode !== null) return;

  await new Promise((resolvePromise) => {
    const timeout = setTimeout(() => {
      viteProcess.kill("SIGKILL");
      resolvePromise();
    }, 2000);
    viteProcess.once("exit", () => {
      clearTimeout(timeout);
      resolvePromise();
    });
    viteProcess.kill("SIGTERM");
  });
}

async function measureActiveView(page) {
  return page.locator(".view.active").evaluate((element) => ({
    offsetWidth: element.offsetWidth,
    clientWidth: element.clientWidth,
    clientHeight: element.clientHeight,
    scrollHeight: element.scrollHeight,
    scrollbarGutter: getComputedStyle(element).scrollbarGutter,
    overflowY: getComputedStyle(element).overflowY,
    supportsStableGutter: CSS.supports("scrollbar-gutter", "stable"),
  }));
}

async function setActiveViewOverflow(page, shouldOverflow) {
  await page.locator(".view.active").evaluate((element, overflow) => {
    for (const child of element.children) {
      child.style.display = "none";
    }

    if (overflow) {
      const probe = document.createElement("div");
      probe.style.height = `${element.clientHeight * 2}px`;
      probe.style.width = "1px";
      element.append(probe);
    }
  }, shouldOverflow);
}

const vite = spawn(
  process.execPath,
  [
    resolve(root, "node_modules/vite/bin/vite.js"),
    "--host",
    "127.0.0.1",
    "--port",
    String(port),
    "--strictPort",
  ],
  { cwd: root, stdio: "ignore" },
);

let browser;
try {
  await waitForVite(vite);
  const bundledChromium = chromium.executablePath();
  const systemChromium = [
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/usr/bin/google-chrome",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
  ].find(existsSync);
  browser = await chromium.launch({
    executablePath:
      process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE ??
      (!existsSync(bundledChromium) ? systemChromium : undefined),
    headless: process.env.PLAYWRIGHT_HEADED !== "1",
  });
  const page = await browser.newPage({
    viewport: { width: 1280, height: 800 },
  });
  await page.addInitScript(installTauriMock, {
    fixtures,
    windowLabel: "main",
  });
  await page.goto(url);
  await page.evaluate(() => document.fonts.ready);
  await page.locator(".view.active").waitFor();

  await setActiveViewOverflow(page, false);
  const withoutOverflow = await measureActiveView(page);
  await page.locator(".nav-item").last().click();
  await page.locator(".nav-item.active").last().waitFor();
  await setActiveViewOverflow(page, true);
  const withOverflow = await measureActiveView(page);

  assert.ok(
    withoutOverflow.scrollHeight <= withoutOverflow.clientHeight,
    "The short view fixture must fit without vertical overflow",
  );
  assert.ok(
    withOverflow.scrollHeight > withOverflow.clientHeight,
    "The long view fixture must overflow vertically",
  );
  if (withoutOverflow.supportsStableGutter) {
    assert.match(
      withoutOverflow.scrollbarGutter,
      /stable/,
      "Views must reserve a stable vertical scrollbar gutter",
    );
  } else {
    assert.equal(
      withoutOverflow.overflowY,
      "scroll",
      "Views must always reserve scrollbar space in older webviews",
    );
  }
  assert.equal(
    withoutOverflow.offsetWidth,
    withOverflow.offsetWidth,
    "Views must occupy the same outer width",
  );
  assert.equal(
    withoutOverflow.clientWidth,
    withOverflow.clientWidth,
    `The content width changed by ${Math.abs(withOverflow.clientWidth - withoutOverflow.clientWidth)}px when the scrollbar appeared`,
  );

  console.log(
    `Layout width stable at ${withoutOverflow.clientWidth}px between overflowing and non-overflowing views`,
  );
} finally {
  await browser?.close();
  await stopVite(vite);
}
