import { Menu } from "lucide-react";

interface TitleBarProps {
  activeViewName: string;
  toggleSidebar: () => void;
}

export function TitleBar({ activeViewName, toggleSidebar }: TitleBarProps) {
  return (
    <div data-tauri-drag-region className="title-bar select-none">
      {/* Left section with Traffic Light space and Menu Toggle */}
      <div data-tauri-drag-region className="flex items-center w-[240px]">
        {/* Space for native traffic lights (Overlay style) */}
        <div data-tauri-drag-region className="w-[80px] h-full"></div>

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

      {/* Right placeholder for balance */}
      <div data-tauri-drag-region className="w-[240px] h-full"></div>
    </div>
  );
}
