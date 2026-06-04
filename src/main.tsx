import React, { useEffect } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { RecordingWindowView } from "./views/RecordingWindowView";
import { ConfigProvider } from "./context/ConfigContext";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Toaster } from "@/components/ui/sonner";
import { OnboardingProvider } from "./components/onboarding/OnboardingProvider";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./i18n";
import { applyPersistedLanguage } from "./i18n/language";

let initialLabel = "";
try {
  initialLabel = getCurrentWindow().label;
} catch (e) {
  console.error("Failed to get window label synchronously:", e);
}

if (initialLabel === "recording_window") {
  document.documentElement.style.background = "transparent";
  document.documentElement.style.backgroundColor = "transparent";
  document.body.style.background = "transparent";
  document.body.style.backgroundColor = "transparent";
  const style = document.createElement("style");
  style.innerHTML = `
    html, body, #root {
      background: transparent !important;
      background-color: transparent !important;
    }
  `;
  document.head.appendChild(style);
}

function Root() {
  useEffect(() => {
    if (initialLabel === "recording_window") return;
    applyPersistedLanguage();
  }, []);

  useEffect(() => {
    if (import.meta.env.PROD) {
      const handleContextMenu = (e: MouseEvent) => {
        e.preventDefault();
      };

      const handleKeyDown = (e: KeyboardEvent) => {
        // Prevent reload / refresh keys: F5, Ctrl+R, Cmd+R, Ctrl+Shift+R, Cmd+Shift+R
        if (
          e.key === "F5" ||
          ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "r")
        ) {
          e.preventDefault();
          return;
        }

        // Prevent devtools keys: F12, Ctrl+Shift+I, Cmd+Alt+I, Ctrl+Shift+J, Cmd+Alt+J, Ctrl+Shift+C, Cmd+Alt+C
        if (
          e.key === "F12" ||
          ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === "i") ||
          ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === "j") ||
          ((e.ctrlKey || e.metaKey) && e.shiftKey && e.key.toLowerCase() === "c") ||
          (e.metaKey && e.altKey && e.key.toLowerCase() === "i") ||
          (e.metaKey && e.altKey && e.key.toLowerCase() === "j") ||
          (e.metaKey && e.altKey && e.key.toLowerCase() === "c")
        ) {
          e.preventDefault();
          return;
        }

        // Prevent view source: Ctrl+U, Cmd+U, Cmd+Alt+U
        if (
          ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "u") ||
          (e.metaKey && e.altKey && e.key.toLowerCase() === "u")
        ) {
          e.preventDefault();
          return;
        }
      };

      window.addEventListener("contextmenu", handleContextMenu);
      window.addEventListener("keydown", handleKeyDown);

      return () => {
        window.removeEventListener("contextmenu", handleContextMenu);
        window.removeEventListener("keydown", handleKeyDown);
      };
    }
  }, []);

  if (initialLabel === "recording_window") {
    return (
      <ConfigProvider>
        <RecordingWindowView />
      </ConfigProvider>
    );
  }

  return (
    <ConfigProvider>
      <TooltipProvider delay={300}>
        <OnboardingProvider>
          <App />
        </OnboardingProvider>
        <Toaster />
      </TooltipProvider>
    </ConfigProvider>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
