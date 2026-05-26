import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Menu, Minus, Square, X } from "lucide-react";

interface TitleBarProps {
  activeViewName: string;
  toggleSidebar: () => void;
}

export function TitleBar({ activeViewName, toggleSidebar }: TitleBarProps) {
  const [isMac, setIsMac] = useState(false);
  const [isWindows, setIsWindows] = useState(false);

  useEffect(() => {
    invoke<{ platform: string }>("check_permissions_status")
      .then((status) => {
        setIsMac(status.platform === "macos");
        setIsWindows(status.platform === "windows");
      })
      .catch(() => {
        setIsMac(false);
      });
  }, []);

  return (
    <div data-tauri-drag-region className="title-bar select-none">
      {/* Left section with Traffic Light space (macOS only) and Menu Toggle */}
      <div data-tauri-drag-region className="flex items-center w-[240px]">
        {isMac && (
          /* Space for native traffic lights */
          <div data-tauri-drag-region className="w-[80px] h-full"></div>
        )}

        <button
          className="p-1 rounded text-muted hover:text-foreground transition-colors title-bar-no-drag"
          onClick={(e) => {
            e.stopPropagation();
            toggleSidebar();
          }}
        >
          <Menu size={16} />
        </button>
      </div>

      {/* Center Title section */}
      <div
        data-tauri-drag-region
        className="flex items-center gap-2 text-[12px] font-medium text-muted"
      >
        <span data-tauri-drag-region>SimpleVoice</span>
        <span data-tauri-drag-region>/</span>
        <span data-tauri-drag-region className="text-foreground">
          {activeViewName}
        </span>
      </div>

      {/* Right section: Window controls (Windows) */}
      {isWindows && (
        <div className="flex items-center h-full ml-auto title-bar-no-drag">
          <button
            className="h-full px-4 hover:bg-white/10 transition-colors flex items-center justify-center"
            onClick={() => invoke("minimize_window")}
          >
            <Minus size={14} />
          </button>
          <button
            className="h-full px-4 hover:bg-white/10 transition-colors flex items-center justify-center"
            onClick={() => invoke("maximize_window")}
          >
            <Square size={13} />
          </button>
          <button
            className="h-full px-4 hover:bg-red-500/80 hover:text-white transition-colors flex items-center justify-center"
            onClick={() => invoke("close_window")}
          >
            <X size={14} />
          </button>
        </div>
      )}

      {/* Right placeholder for balance on macOS */}
      {isMac && <div data-tauri-drag-region className="w-[80px] h-full"></div>}
    </div>
  );
}
