import { useState } from "react";
import "./App.css";

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
