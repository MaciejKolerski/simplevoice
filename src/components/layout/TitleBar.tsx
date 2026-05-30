import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Menu, Minus, Square, X } from "lucide-react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

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
    <div data-tauri-drag-region className="title-bar select-none relative">
      <div data-tauri-drag-region className="flex items-center w-[240px] pl-4">
        {isMac && !isWindows && (
          <div data-tauri-drag-region className="w-[80px] h-full"></div>
        )}

        <Tooltip>
          <TooltipTrigger
            render={
              <button
                className="p-1.5 rounded-md text-muted hover:text-foreground hover:bg-accent transition-colors title-bar-no-drag"
                onClick={(e) => {
                  e.stopPropagation();
                  toggleSidebar();
                }}
              >
                <Menu size={16} />
              </button>
            }
          />
          <TooltipContent side="bottom" sideOffset={6}>
            Toggle sidebar
          </TooltipContent>
        </Tooltip>
      </div>

      {/* Center Title section */}
      <div
        data-tauri-drag-region
        className="absolute left-1/2 -translate-x-1/2 h-full flex items-center gap-2 text-[12px] font-medium text-muted"
      >
        <span data-tauri-drag-region>Simplevoice</span>
        <span data-tauri-drag-region className="text-muted-dark">/</span>
        <span data-tauri-drag-region className="text-foreground">
          {activeViewName}
        </span>
      </div>

      {/* Right section: Window controls (Windows) */}
      {isWindows && (
        <div className="flex items-center h-full ml-auto title-bar-no-drag">
          <button
            className="h-full px-4 text-muted hover:text-foreground hover:bg-accent transition-colors flex items-center justify-center"
            onClick={() => invoke("minimize_window")}
          >
            <Minus size={14} />
          </button>
          <button
            className="h-full px-4 text-muted hover:text-foreground hover:bg-accent transition-colors flex items-center justify-center"
            onClick={() => invoke("maximize_window")}
          >
            <Square size={13} />
          </button>
          <button
            className="h-full px-4 text-muted hover:bg-red-500 hover:text-white transition-colors flex items-center justify-center"
            onClick={() => invoke("close_window")}
          >
            <X size={14} />
          </button>
        </div>
      )}

      {/* Right placeholder for balance on macOS */}
      {isMac && !isWindows && (
        <div data-tauri-drag-region className="w-[80px] h-full mr-4"></div>
      )}
    </div>
  );
}
