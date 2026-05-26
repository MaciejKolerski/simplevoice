import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { RecordingWindowView } from "./views/RecordingWindowView";
import { ConfigProvider } from "./context/ConfigContext";
import { getCurrentWindow } from "@tauri-apps/api/window";

function Root() {
  const [label, setLabel] = useState<string>("");

  useEffect(() => {
    try {
      const win = getCurrentWindow();
      setLabel(win.label);
    } catch (e) {
      console.error("Failed to get window label:", e);
    }
  }, []);

  if (label === "") {
    return null; // Wait until window is resolved
  }

  if (label === "recording_window") {
    return (
      <ConfigProvider>
        <RecordingWindowView />
      </ConfigProvider>
    );
  }

  return (
    <ConfigProvider>
      <App />
    </ConfigProvider>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
