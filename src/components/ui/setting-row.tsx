import { createContext, useContext, useId, ReactNode } from "react";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

/**
 * Id of the row's title element. Form controls dropped into a SettingRow read
 * it and point `aria-labelledby` at it, so a row's visible title is also its
 * control's accessible name without every call site repeating an aria-label.
 */
const SettingRowLabelContext = createContext<string | undefined>(undefined);

export function useSettingRowLabelId() {
  return useContext(SettingRowLabelContext);
}

type SettingRowProps = {
  title: ReactNode;
  description?: ReactNode;
  /** "row" puts the control to the right of the text (default); "column" stacks it below. */
  layout?: "row" | "column";
  children?: ReactNode;
  className?: string;
};

/**
 * One bordered row inside a settings card. Encapsulates the
 * title/description/control pattern shared by SettingsView and ModelsView so
 * spacing, typography and dividers stay consistent.
 */
export function SettingRow({
  title,
  description,
  layout = "row",
  children,
  className,
  ...rest
}: SettingRowProps) {
  const labelId = useId();

  if (layout === "column") {
    return (
      <div
        className={cn("flex flex-col p-5 border-b border-border last:border-b-0", className)}
        {...rest}
      >
        <Label id={labelId} className={description ? "mb-1" : "mb-3"}>
          {title}
        </Label>
        {description && <p className="text-muted text-[13px] mb-3">{description}</p>}
        <SettingRowLabelContext.Provider value={labelId}>
          {children}
        </SettingRowLabelContext.Provider>
      </div>
    );
  }

  return (
    <div
      className={cn("flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0", className)}
      {...rest}
    >
      <div className="flex-1 min-w-0">
        <div id={labelId} className="text-foreground font-medium mb-1">
          {title}
        </div>
        {description && (
          <div className="text-muted text-[13px] leading-snug">{description}</div>
        )}
      </div>
      <SettingRowLabelContext.Provider value={labelId}>
        {children}
      </SettingRowLabelContext.Provider>
    </div>
  );
}
