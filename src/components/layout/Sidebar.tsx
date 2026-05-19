import { Activity, Box, History, Settings } from "lucide-react";
import { clsx } from "clsx";

interface SidebarProps {
  collapsed: boolean;
  activeView: string;
  setActiveView: (view: string) => void;
}

export function Sidebar({
  collapsed,
  activeView,
  setActiveView,
}: SidebarProps) {
  return (
    <aside className={clsx("sidebar", collapsed && "collapsed")}>
      <div className="flex flex-col gap-0.5 p-3 flex-1">
        <div
          className={clsx("nav-item", activeView === "usage" && "active")}
          onClick={() => setActiveView("usage")}
        >
          <Activity size={16} />
          {!collapsed && <span className="nav-label">Usage</span>}
        </div>
        <div
          className={clsx("nav-item", activeView === "models" && "active")}
          onClick={() => setActiveView("models")}
        >
          <Box size={16} />
          {!collapsed && <span className="nav-label">Models</span>}
        </div>
        <div
          className={clsx(
            "nav-item",
            activeView === "transcriptions" && "active",
          )}
          onClick={() => setActiveView("transcriptions")}
        >
          <History size={16} />
          {!collapsed && <span className="nav-label">Transcriptions</span>}
        </div>
        <div
          className={clsx(
            "nav-item mt-auto",
            activeView === "settings" && "active",
          )}
          onClick={() => setActiveView("settings")}
        >
          <Settings size={16} />
          {!collapsed && <span className="nav-label">Settings</span>}
        </div>
      </div>
    </aside>
  );
}
