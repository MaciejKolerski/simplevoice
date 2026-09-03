import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { createServer } from "node:net";
import { chromium } from "playwright";
import { installTauriMock } from "./readme-shots/mock.mjs";
import {
  CONFIG,
  DEVICES,
  MODELS,
  PERMISSIONS,
  usageStats,
} from "./readme-shots/fixtures.mjs";

function reservePort() {
  return new Promise((resolvePort, rejectPort) => {
    const server = createServer();
    server.once("error", rejectPort);
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      if (!address || typeof address === "string") {
        server.close();
        rejectPort(new Error("Could not reserve a Vite port"));
        return;
      }
      server.close((error) => {
        if (error) rejectPort(error);
        else resolvePort(address.port);
      });
    });
  });
}

function startVite(port) {
  const child = spawn(
    "pnpm",
    [
      "exec",
      "vite",
      "--host",
      "127.0.0.1",
      "--port",
      String(port),
      "--strictPort",
    ],
    { cwd: process.cwd(), stdio: ["ignore", "pipe", "pipe"] },
  );

  return new Promise((resolveVite, rejectVite) => {
    const timeout = setTimeout(() => {
      child.kill("SIGTERM");
      rejectVite(new Error("Vite did not start within 60 seconds"));
    }, 60_000);

    const fail = (error) => {
      clearTimeout(timeout);
      rejectVite(error);
    };

    child.once("error", fail);
    child.once("exit", (code) => {
      fail(new Error(`Vite exited before the test with code ${code}`));
    });
    child.stderr.on("data", (chunk) => process.stderr.write(chunk));
    child.stdout.on("data", (chunk) => {
      if (!String(chunk).includes("Local:")) return;
      clearTimeout(timeout);
      resolveVite(child);
    });
  });
}

const fixtures = {
  appVersion: "0.1.9",
  config: CONFIG,
  permissions: PERMISSIONS,
  usage: usageStats(),
  models: MODELS,
  devices: DEVICES,
  transcriptions: [
    {
      id: "history-audio-regression",
      timestamp: "12:00",
      date: "2026-09-03",
      text: "Long history recording",
      model: "test-model",
      wav_path:
        "/Users/you/Library/Application Support/com.woro.simplevoice/recordings/history-audio-regression/output.wav",
      duration_sec: 4_500,
    },
  ],
};

const port = await reservePort();
const vite = await startVite(port);
const configuredChromium = process.env.SIMPLEVOICE_TEST_CHROMIUM;
const systemChromium = "/usr/bin/chromium";
const executablePath = configuredChromium
  ? configuredChromium
  : existsSync(systemChromium)
    ? systemChromium
    : undefined;

let browser;
try {
  browser = await chromium.launch({
    ...(executablePath ? { executablePath } : {}),
    args: ["--js-flags=--max-old-space-size=64"],
  });
  const page = await browser.newPage();
  page.setDefaultTimeout(3_000);
  page.setDefaultNavigationTimeout(15_000);
  await page.addInitScript(installTauriMock, {
    fixtures,
    windowLabel: "main",
  });
  await page.goto(`http://127.0.0.1:${port}/`);
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(error.message));
  await page.evaluate(() => {
    window.__historyAudioBase64Calls = 0;
    window.__historyAudioCommands = [];
    const originalInvoke = window.__TAURI_INTERNALS__.invoke.bind(
      window.__TAURI_INTERNALS__,
    );
    window.__TAURI_INTERNALS__.invoke = async (command, args = {}) => {
      if (command === "get_audio_base64") {
        window.__historyAudioBase64Calls += 1;
        return "unexpected-base64-audio";
      }
      if (command === "play_history_audio") {
        window.__historyAudioCommands.push({ command, args });
        return {
          id: args.id,
          positionSec: args.positionSec ?? 0,
          durationSec: 4_500,
          playing: true,
        };
      }
      if (command === "get_history_audio_status") {
        return {
          id: "history-audio-regression",
          positionSec: 12.5,
          durationSec: 4_500,
          playing: true,
        };
      }
      if (
        command === "pause_history_audio" ||
        command === "seek_history_audio" ||
        command === "stop_history_audio"
      ) {
        window.__historyAudioCommands.push({ command, args });
        return {
          id: args.id ?? null,
          positionSec: args.positionSec ?? 0,
          durationSec: 4_500,
          playing: false,
        };
      }
      return originalInvoke(command, args);
    };
    window.__TAURI_INTERNALS__.convertFileSrc = (path, protocol = "asset") =>
      `${protocol}://localhost/${encodeURIComponent(path)}`;

    const originalCreateElement = Document.prototype.createElement;
    Document.prototype.createElement = function (name, options) {
      if (String(name).toLowerCase() === "audio") {
        throw new Error(
          "Native <audio> reached WebKit's unavailable GStreamer backend",
        );
      }
      return originalCreateElement.call(this, name, options);
    };
  });

  await page.getByRole("button", { name: "Transcriptions" }).first().click();
  await page.getByText("Long history recording").click();

  const player = page.locator('[data-history-audio-player="native"]');
  await player.waitFor().catch((error) => {
    if (pageErrors.length > 0) {
      throw new Error(`History player crashed the UI: ${pageErrors.join("; ")}`);
    }
    throw error;
  });
  await player.getByRole("button", { name: "Play recording" }).click();
  await page.getByText("0:12").waitFor();

  const base64Calls = await page.evaluate(
    () => window.__historyAudioBase64Calls,
  );
  const commands = await page.evaluate(() => window.__historyAudioCommands);

  if (pageErrors.length > 0) {
    throw new Error(`History player crashed the UI: ${pageErrors.join("; ")}`);
  }
  if ((await page.locator("audio").count()) !== 0) {
    throw new Error("History mounted a native <audio> element");
  }
  if (base64Calls !== 0) {
    throw new Error(`Audio crossed IPC as base64 ${base64Calls} time(s)`);
  }
  const play = commands.find(({ command }) => command === "play_history_audio");
  if (!play || play.args.id !== "history-audio-regression") {
    throw new Error("History did not start the native Rust audio controller");
  }

  process.stdout.write(
    "History stayed responsive and used the native Rust audio controller.\n",
  );
} finally {
  await browser?.close().catch(() => {});
  vite.kill("SIGTERM");
}
