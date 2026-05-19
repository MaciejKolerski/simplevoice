import { useEffect, useState } from "react";
import "./App.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { TitleBar } from "./components/layout/TitleBar";
import { Sidebar } from "./components/layout/Sidebar";
import { UsageView } from "./views/UsageView";
import { ModelsView } from "./views/ModelsView";
import { TranscriptionsView } from "./views/TranscriptionsView";
import { SettingsView } from "./views/SettingsView";

type ViewId = "usage" | "models" | "transcriptions" | "settings";

function App() {
  const [activeView, setActiveView] = useState<ViewId>("usage");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);

  useEffect(() => {
    const initDevice = async () => {
      try {
        const saved = localStorage.getItem("selected_audio_device");
        if (saved) {
          const list = await invoke<string[]>("list_audio_devices");
          if (list.includes(saved)) {
            await invoke("set_selected_device", { device: saved });
            return;
          }
        }
        await invoke("set_selected_device", { device: null });
      } catch (err) {
        console.error("Failed to initialize audio device:", err);
      }
    };
    initDevice();
  }, []);

  useEffect(() => {
    const unlisten = listen<string>("navigate", (event) => {
      setActiveView(event.payload as ViewId);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const getTitleName = (id: ViewId) => {
    return id.charAt(0).toUpperCase() + id.slice(1);
  };

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden bg-black">
      <TitleBar
        activeViewName={getTitleName(activeView)}
        toggleSidebar={() => setSidebarCollapsed(!sidebarCollapsed)}
      />
      <div className="flex flex-1 overflow-hidden">
        <Sidebar
          collapsed={sidebarCollapsed}
          activeView={activeView}
          setActiveView={(v) => setActiveView(v as ViewId)}
        />

        <main className="main-content">
          <div className={`view ${activeView === "usage" ? "active" : ""}`}>
            <UsageView />
          </div>
          <div className={`view ${activeView === "models" ? "active" : ""}`}>
            <ModelsView />
          </div>
          <div
            className={`view ${activeView === "transcriptions" ? "active" : ""}`}
          >
            <TranscriptionsView />
          </div>
          <div className={`view ${activeView === "settings" ? "active" : ""}`}>
            <SettingsView />
          </div>
        </main>
      </div>
    </div>
  );
}

export default App;
