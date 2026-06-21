import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Trash2 } from "lucide-react";
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
        action: (ACTIONS.includes(r.action as RuleAction)
          ? (r.action as RuleAction)
          : "text"),
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

  return (
    <div className="flex flex-col animate-[fadeIn_0.3s_ease-out]">
      <div className="mb-6">
        <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
          {t("dictionary.title")}
        </h1>
        <p className="mt-1 text-sm text-muted-foreground">
          {t("dictionary.description")}
        </p>
      </div>

      <section className="flex flex-col gap-3 max-w-2xl">
        {rules.length === 0 && (
          <p className="text-sm text-muted-foreground">{t("dictionary.empty")}</p>
        )}

        {rules.map((rule, i) => (
          <div key={i} className="flex items-center gap-2">
            <Input
              value={rule.trigger}
              onChange={(e) => patchRule(i, { trigger: e.target.value })}
              placeholder={t("dictionary.triggerPlaceholder")}
              className="flex-1"
            />

            <Select
              value={rule.action}
              onValueChange={(v) =>
                patchRule(i, { action: (v ?? "text") as RuleAction })
              }
              items={Object.fromEntries(ACTIONS.map((a) => [a, actionLabels[a]]))}
            >
              <SelectTrigger className="w-36 bg-black">
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

            {rule.action === "text" && (
              <Input
                value={rule.value}
                onChange={(e) => patchRule(i, { value: e.target.value })}
                placeholder={t("dictionary.valuePlaceholder")}
                className="flex-1"
              />
            )}

            <Button
              variant="ghost"
              size="icon"
              onClick={() => removeRule(i)}
              aria-label={t("dictionary.remove")}
            >
              <Trash2 size={16} />
            </Button>
          </div>
        ))}

        <Button variant="outline" onClick={addRule} className="self-start">
          {t("dictionary.addRule")}
        </Button>
      </section>
    </div>
  );
}
