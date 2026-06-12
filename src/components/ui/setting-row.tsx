import { ReactNode } from "react";
import { Label } from "@/components/ui/label";

type SettingRowProps = {
  title: ReactNode;
  description?: ReactNode;
  /** "row" puts the control to the right of the text (default); "column" stacks it below. */
  layout?: "row" | "column";
  children?: ReactNode;
  className?: string;
  "data-tour"?: string;
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
  className = "",
  ...rest
}: SettingRowProps) {
  if (layout === "column") {
    return (
      <div
        className={`flex flex-col p-5 border-b border-border last:border-b-0 ${className}`}
        {...rest}
      >
        <Label className={description ? "mb-1" : "mb-3"}>{title}</Label>
        {description && <p className="text-muted text-[13px] mb-3">{description}</p>}
        {children}
      </div>
    );
  }

  return (
    <div
      className={`flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0 ${className}`}
      {...rest}
    >
      <div className="flex-1 min-w-0">
        <div className="text-fg font-medium mb-1">{title}</div>
        {description && (
          <div className="text-muted text-[13px] leading-snug">{description}</div>
        )}
      </div>
      {children}
    </div>
  );
}
