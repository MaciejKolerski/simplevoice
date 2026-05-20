import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { invoke } from "@tauri-apps/api/core";

let isInitializing = true;

const originalSetItem = localStorage.setItem;
localStorage.setItem = function (key: string, value: string) {
  originalSetItem.call(this, key, value);

  if (!isInitializing) {
    if (key !== "transcription_history") {
      const config: Record<string, string> = {};
      for (let i = 0; i < localStorage.length; i++) {
        const k = localStorage.key(i);
        if (k && k !== "transcription_history") {
          config[k] = localStorage.getItem(k) || "";
        }
      }
      invoke("save_config", { config: JSON.stringify(config) }).catch((err) =>
        console.error("Failed to save config to JSON file:", err)
      );
    }
  }
};

const originalRemoveItem = localStorage.removeItem;
localStorage.removeItem = function (key: string) {
  originalRemoveItem.call(this, key);

  if (!isInitializing) {
    if (key !== "transcription_history") {
      const config: Record<string, string> = {};
      for (let i = 0; i < localStorage.length; i++) {
        const k = localStorage.key(i);
        if (k && k !== "transcription_history") {
          config[k] = localStorage.getItem(k) || "";
        }
      }
      invoke("save_config", { config: JSON.stringify(config) }).catch((err) =>
        console.error("Failed to save config to JSON file:", err)
      );
    }
  }
};

async function initLocalStorage() {
  try {
    const configStr = await invoke<string>("load_config");
    const config = JSON.parse(configStr || "{}");
    for (const [key, value] of Object.entries(config)) {
      if (key !== "transcription_history") {
        originalSetItem.call(localStorage, key, String(value));
      }
    }

    const historyStr = await invoke<string>("load_history");
    originalSetItem.call(localStorage, "transcription_history", historyStr || "[]");
  } catch (err) {
    console.error("Failed to pre-load settings from JSON files:", err);
  } finally {
    isInitializing = false;
  }
}

initLocalStorage().finally(() => {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  );
});

