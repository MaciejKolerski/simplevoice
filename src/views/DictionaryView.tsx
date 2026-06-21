import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Languages, Trash2 } from "lucide-react";
import { useConfig } from "../context/ConfigContext";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

type RuleAction = "text" | "time" | "date";

interface DictionaryRule {
  trigger: string;
  action: RuleAction;
  value: string;
}

const ACTIONS: RuleAction[] = ["text", "time", "date"];

/** Live previews of what time/date actions insert, in the backend's formats
 * (`%H:%M:%S` / `%Y-%m-%d`). Snapshotted at render — illustrative, not a clock. */
function actionPreviews(): Record<"time" | "date", string> {
  const now = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return {
    time: `${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`,
    date: `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`,
  };
}

export function DictionaryView() {
  const { t } = useTranslation();
  const { getConfig, updateConfig } = useConfig();

  const [rules, setRules] = useState<DictionaryRule[]>(() => {
    const stored = getConfig("dictionary_rules", null) as
      | { trigger?: string; action?: string; value?: string }[]
      | null;
    if (Array.isArray(stored)) {
      return stored.map((r) => ({
        trigger: r.trigger ?? "",
        action: ACTIONS.includes(r.action as RuleAction)
          ? (r.action as RuleAction)
          : "text",
        value: r.value ?? "",
      }));
    }
    // Migrate the legacy `custom_words` array into `text` rules.
    const legacy = (getConfig("custom_words", []) as string[]) || [];
    return legacy.map((w) => ({ trigger: w, action: "text" as RuleAction, value: w }));
  });

  const persist = (next: DictionaryRule[]) => {
    setRules(next);
    updateConfig("dictionary_rules", next);
    // Once the new shape is written, the legacy field is no longer used.
    updateConfig("custom_words", []);
  };

  const addRule = () =>
    persist([...rules, { trigger: "", action: "text", value: "" }]);
  const removeRule = (index: number) =>
    persist(rules.filter((_, i) => i !== index));
  const patchRule = (index: number, patch: Partial<DictionaryRule>) =>
    persist(rules.map((r, i) => (i === index ? { ...r, ...patch } : r)));

  const actionLabels: Record<RuleAction, string> = {
    text: t("dictionary.actionText"),
    time: t("dictionary.actionTime"),
    date: t("dictionary.actionDate"),
  };
  const previews = actionPreviews();

  return (
    <div className="flex flex-col w-full animate-[fadeIn_0.3s_ease-out]">
      <div className="flex items-center justify-between gap-4 mb-6">
        <div>
          <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
            {t("dictionary.title")}
          </h1>
          <p className="text-xs text-muted mt-1 leading-normal">
            {t("dictionary.description")}
          </p>
        </div>
        {rules.length > 0 && (
          <Button variant="outline" size="sm" onClick={addRule} className="shrink-0">
            {t("dictionary.addRule")}
          </Button>
        )}
      </div>

      {rules.length === 0 ? (
        <div className="flex flex-col items-center justify-center p-12 text-center border border-dashed border-border rounded-xl bg-secondary">
          <div className="flex size-14 items-center justify-center rounded-full bg-surface-active text-muted mb-4">
            <Languages size={26} />
          </div>
          <p className="text-muted text-sm max-w-md mb-4 leading-relaxed">
            {t("dictionary.empty")}
          </p>
          <Button variant="outline" size="sm" onClick={addRule}>
            {t("dictionary.addRule")}
          </Button>
        </div>
      ) : (
        <div className="border border-border rounded-xl overflow-hidden bg-secondary">
          <div className="flex items-center gap-3 px-5 py-2.5 bg-black/30 border-b border-border/50 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            <span className="flex-1">{t("dictionary.colTrigger")}</span>
            <span className="w-40 shrink-0">{t("dictionary.colAction")}</span>
            <span className="flex-1">{t("dictionary.colValue")}</span>
            <span className="size-8 shrink-0" aria-hidden="true" />
          </div>

          {rules.map((rule, i) => (
            <div
              key={i}
              className="flex items-center gap-3 px-5 py-3 border-b border-border/50 last:border-b-0 hover:bg-surface-hover transition-colors"
            >
              <Input
                value={rule.trigger}
                onChange={(e) => patchRule(i, { trigger: e.target.value })}
                placeholder={t("dictionary.triggerPlaceholder")}
                className="flex-1 bg-black"
              />

              <Select
                value={rule.action}
                onValueChange={(v) =>
                  patchRule(i, { action: (v ?? "text") as RuleAction })
                }
                items={Object.fromEntries(ACTIONS.map((a) => [a, actionLabels[a]]))}
              >
                <SelectTrigger className="w-40 shrink-0 bg-black">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {ACTIONS.map((a) => (
                    <SelectItem key={a} value={a}>
                      {actionLabels[a]}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>

              <div className="flex-1 min-w-0">
                {rule.action === "text" ? (
                  <Input
                    value={rule.value}
                    onChange={(e) => patchRule(i, { value: e.target.value })}
                    placeholder={t("dictionary.valuePlaceholder")}
                    className="w-full bg-black"
                  />
                ) : (
                  <span className="font-mono text-[13px] text-muted">
                    → {previews[rule.action]}
                  </span>
                )}
              </div>

              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => removeRule(i)}
                aria-label={t("dictionary.remove")}
                className="shrink-0 text-muted hover:text-danger"
              >
                <Trash2 size={16} />
              </Button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
