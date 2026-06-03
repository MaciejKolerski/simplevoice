import { Activity, Box, History, Settings } from "lucide-react";
import { clsx } from "clsx";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

interface SidebarProps {
  collapsed: boolean;
  activeView: string;
  setActiveView: (view: string) => void;
}

const NAV_ITEMS = [
  { id: "usage", label: "Usage", Icon: Activity },
  { id: "models", label: "Models", Icon: Box },
  { id: "transcriptions", label: "Transcriptions", Icon: History },
] as const;

export function Sidebar({ collapsed, activeView, setActiveView }: SidebarProps) {
  const renderItem = (
    id: string,
    label: string,
    Icon: typeof Activity,
    extraClass?: string,
  ) => {
    const item = (
      <div
        className={clsx(
          "nav-item",
          collapsed && "justify-center",
          activeView === id && "active",
          extraClass,
        )}
        onClick={() => setActiveView(id)}
      >
        <Icon size={16} className="shrink-0" />
        {!collapsed && <span className="nav-label">{label}</span>}
      </div>
    );

    if (!collapsed) return item;

    return (
      <Tooltip key={id}>
        <TooltipTrigger render={item} />
        <TooltipContent side="right" sideOffset={10}>
          {label}
        </TooltipContent>
      </Tooltip>
    );
  };

  return (
    <aside data-tour="sidebar" className={clsx("sidebar", collapsed && "collapsed")}>
      <div className="flex flex-col gap-0.5 p-3 flex-1">
        {NAV_ITEMS.map(({ id, label, Icon }) => renderItem(id, label, Icon))}
        {renderItem("settings", "Settings", Settings, "mt-auto")}
      </div>
    </aside>
  );
}
