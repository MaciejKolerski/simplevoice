import { Activity, Box, History, Languages, Settings } from "lucide-react";
import { clsx } from "clsx";
import { useTranslation } from "react-i18next";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

interface SidebarProps {
  collapsed: boolean;
  activeView: string;
  setActiveView: (view: string) => void;
}

const NAV_ITEMS = [
  { id: "usage", Icon: Activity },
  { id: "models", Icon: Box },
  { id: "transcriptions", Icon: History },
  { id: "dictionary", Icon: Languages },
] as const;

export function Sidebar({ collapsed, activeView, setActiveView }: SidebarProps) {
  const { t } = useTranslation();
  const renderItem = (
    id: string,
    label: string,
    Icon: typeof Activity,
    extraClass?: string,
  ) => {
    const item = (
      <button
        type="button"
        className={clsx(
          "nav-item",
          collapsed && "justify-center",
          activeView === id && "active",
          extraClass,
        )}
        onClick={() => setActiveView(id)}
        aria-current={activeView === id ? "page" : undefined}
      >
        <Icon size={16} className="shrink-0" />
        {!collapsed && <span className="nav-label">{label}</span>}
      </button>
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
        {NAV_ITEMS.map(({ id, Icon }) => renderItem(id, t(`nav.${id}`), Icon))}
        {renderItem("settings", t("nav.settings"), Settings, "mt-auto")}
      </div>
    </aside>
  );
}
