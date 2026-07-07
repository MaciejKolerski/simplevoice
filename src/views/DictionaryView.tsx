import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { Languages, Plus, RotateCcw, Search, Trash2 } from "lucide-react";
import { useConfig } from "../context/ConfigContext";
import { cn } from "@/lib/utils";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
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

/** A voice-search command: site keyword(s) spoken after the prefix → open the
 * site with the rest of the utterance as the query. Mirrors the Rust
 * `SearchCommand` struct. */
interface SearchCommand {
  id: string;
  name: string;
  triggers: string[];
  url: string;
  enabled: boolean;
  builtin: boolean;
}

const ACTIONS: RuleAction[] = ["text", "time", "date"];

const genId = () =>
  typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `cmd-${Date.now().toString(36)}${Math.random().toString(36).slice(2)}`;

/** Strips a leading legacy wake word ("hej"/"hey") from a stored trigger so
 * pre-prefix full-phrase triggers ("hej google") collapse to the bare keyword
 * ("google") the prefix model now expects. Idempotent for keyword-only data. */
function stripWakeWord(trigger: string): string {
  const parts = trigger.trim().split(/\s+/);
  if (parts.length > 1) {
    const first = parts[0].toLowerCase().replace(/[^\p{L}\p{N}]+/gu, "");
    if (first === "hej" || first === "hey") return parts.slice(1).join(" ");
  }
  return trigger.trim();
}

/** Coerces a stored/loaded command into a well-formed `SearchCommand` so a
 * malformed config entry can never crash the view. Triggers are the site keyword
 * only (the wake word is the global prefix), so any legacy wake word is stripped
 * and duplicates collapsed. */
function normalizeCommand(c: any): SearchCommand {
  const rawTriggers = Array.isArray(c?.triggers)
    ? c.triggers.filter((x: any) => typeof x === "string")
    : [];
  const keywords = rawTriggers
    .map(stripWakeWord)
    .map((s: string) => s.trim())
    .filter(Boolean);
  return {
    id: typeof c?.id === "string" && c.id ? c.id : genId(),
    name: typeof c?.name === "string" ? c.name : "",
    triggers: Array.from(new Set<string>(keywords)),
    url: typeof c?.url === "string" ? c.url : "",
    enabled: c?.enabled !== false,
    builtin: c?.builtin === true,
  };
}

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
  const { config, getConfig, updateConfig } = useConfig();

  // The shared wake-word prefix (edited in Settings); surfaced here so the keyword
  // fields below read as the full spoken phrase ("hej" + "google").
  const prefixRaw = getConfig("search_command_prefix", "hej");
  const prefix = (typeof prefixRaw === "string" ? prefixRaw : "hej").trim();

  // ─── Voice search commands ────────────────────────────────────────────────
  const [searchEnabled, setSearchEnabled] = useState(true);
  const [commands, setCommands] = useState<SearchCommand[]>([]);
  // Per-row editing buffer for the comma-separated triggers field, so typing a
  // trailing comma isn't eaten by the array round-trip. Committed on blur.
  const [triggerDrafts, setTriggerDrafts] = useState<Record<string, string>>({});
  // Guards the one-time defaults fetch: once we've resolved commands (from config
  // or the backend), config re-loads must not refetch defaults over user edits.
  const resolvedRef = useRef(false);

  // Sync from config whenever it (re)loads. All views mount at startup, before
  // the async config load resolves, so this effect — not a mount-time read — is
  // what populates the list (same pattern as SettingsView).
  useEffect(() => {
    setSearchEnabled(getConfig("search_commands_enabled", true) !== false);
    const stored = getConfig("search_commands", null);
    if (Array.isArray(stored)) {
      resolvedRef.current = true;
      setCommands(stored.map(normalizeCommand));
    } else if (!resolvedRef.current) {
      // No stored commands: show the built-in defaults. They are NOT persisted
      // until the user edits something, so unmodified installs keep receiving new
      // built-ins on update (the backend applies the same defaults when the key
      // is absent), while an edit freezes the list to the user's version.
      resolvedRef.current = true;
      invoke<SearchCommand[]>("get_default_search_commands")
        .then((defs) => setCommands(defs.map(normalizeCommand)))
        .catch((e) => console.error("Failed to load default search commands:", e));
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [config]);

  const persistCommands = (next: SearchCommand[]) => {
    setCommands(next);
    updateConfig("search_commands", next);
  };
  const patchCommand = (id: string, patch: Partial<SearchCommand>) =>
    persistCommands(commands.map((c) => (c.id === id ? { ...c, ...patch } : c)));
  const removeCommand = (id: string) =>
    persistCommands(commands.filter((c) => c.id !== id));
  const addCommand = () =>
    persistCommands([
      ...commands,
      { id: genId(), name: "", triggers: [], url: "https://", enabled: true, builtin: false },
    ]);
  const restoreDefaults = () =>
    invoke<SearchCommand[]>("get_default_search_commands")
      .then((defs) => {
        // Reset the built-ins, keep the user's own custom commands.
        const custom = commands.filter((c) => !c.builtin);
        persistCommands([...defs.map(normalizeCommand), ...custom]);
      })
      .catch((e) => console.error("Failed to restore default search commands:", e));

  const toggleSearchEnabled = (v: boolean) => {
    setSearchEnabled(v);
    updateConfig("search_commands_enabled", v);
  };

  const triggerText = (c: SearchCommand) =>
    triggerDrafts[c.id] ?? c.triggers.join(", ");
  const commitTriggers = (c: SearchCommand) => {
    const draft = triggerDrafts[c.id];
    if (draft === undefined) return;
    const parsed = draft.split(",").map((s) => s.trim()).filter(Boolean);
    patchCommand(c.id, { triggers: parsed });
    setTriggerDrafts((d) => {
      const next = { ...d };
      delete next[c.id];
      return next;
    });
  };

  // ─── Custom dictionary (text / time / date) ──────────────────────────────
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
    <div className="flex flex-col w-full gap-12 animate-[fadeIn_0.3s_ease-out]">
      {/* ─── Voice search ─────────────────────────────────────────────────── */}
      <section>
        <div className="flex items-start justify-between gap-4 mb-5">
          <div>
            <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
              {t("dictionary.voiceSearchTitle")}
            </h1>
            <p className="text-xs text-muted mt-1 leading-relaxed max-w-2xl">
              {t("dictionary.voiceSearchDescription")}
            </p>
            <p className="text-[11px] text-muted-foreground mt-1.5 leading-relaxed max-w-2xl">
              {prefix
                ? t("dictionary.prefixHint", { prefix })
                : t("dictionary.prefixHintNone")}
            </p>
          </div>
          <label className="flex items-center gap-2 shrink-0 pt-1 cursor-pointer">
            <span className="text-xs text-muted select-none">
              {searchEnabled ? t("common.on") : t("common.off")}
            </span>
            <Switch
              checked={searchEnabled}
              onCheckedChange={toggleSearchEnabled}
              aria-label={t("dictionary.voiceSearchTitle")}
            />
          </label>
        </div>

        <div className={cn("transition-opacity", !searchEnabled && "opacity-50")}>
          {commands.length === 0 ? (
            <div className="flex flex-col items-center justify-center p-10 text-center border border-dashed border-border rounded-xl bg-secondary">
              <div className="flex size-14 items-center justify-center rounded-full bg-surface-active text-muted mb-4">
                <Search size={26} />
              </div>
              <p className="text-muted text-sm max-w-md mb-4 leading-relaxed">
                {t("dictionary.voiceSearchEmpty")}
              </p>
            </div>
          ) : (
            <div className="border border-border rounded-xl overflow-hidden bg-secondary">
              {commands.map((c) => (
                <div
                  key={c.id}
                  className="flex items-start gap-3 px-4 py-3.5 border-b border-border/50 last:border-b-0 hover:bg-surface-hover transition-colors"
                >
                  <Switch
                    checked={c.enabled}
                    onCheckedChange={(v) => patchCommand(c.id, { enabled: v })}
                    className="mt-2 shrink-0"
                    aria-label={c.name || t("dictionary.colName")}
                  />

                  <div className="flex-1 min-w-0 flex flex-col gap-2">
                    <div className="flex flex-col sm:flex-row gap-2">
                      <Input
                        value={c.name}
                        onChange={(e) => patchCommand(c.id, { name: e.target.value })}
                        placeholder={t("dictionary.namePlaceholder")}
                        className="bg-black font-medium sm:w-44 sm:shrink-0"
                      />
                      <Input
                        value={triggerText(c)}
                        onChange={(e) =>
                          setTriggerDrafts((d) => ({ ...d, [c.id]: e.target.value }))
                        }
                        onBlur={() => commitTriggers(c)}
                        placeholder={t("dictionary.triggersPlaceholder")}
                        className="bg-black flex-1"
                      />
                    </div>
                    <Input
                      value={c.url}
                      onChange={(e) => patchCommand(c.id, { url: e.target.value })}
                      placeholder={t("dictionary.urlPlaceholder")}
                      spellCheck={false}
                      autoCapitalize="off"
                      autoCorrect="off"
                      className="bg-black font-mono text-[12px] text-muted"
                    />
                  </div>

                  <Button
                    variant="ghost"
                    size="icon-sm"
                    onClick={() => removeCommand(c.id)}
                    aria-label={t("dictionary.remove")}
                    className="shrink-0 mt-1 text-muted hover:text-danger"
                  >
                    <Trash2 size={16} />
                  </Button>
                </div>
              ))}
            </div>
          )}

          <div className="flex items-center gap-2 mt-3">
            <Button variant="outline" size="sm" onClick={addCommand}>
              <Plus size={15} />
              {t("dictionary.addSearch")}
            </Button>
            <Button variant="ghost" size="sm" onClick={restoreDefaults} className="text-muted">
              <RotateCcw size={14} />
              {t("dictionary.restoreDefaults")}
            </Button>
          </div>
          <p className="text-[11px] text-muted-foreground mt-2 leading-relaxed">
            {t("dictionary.urlHint")}
          </p>
        </div>
      </section>

      {/* ─── Custom dictionary ────────────────────────────────────────────── */}
      <section className="border-t border-border/50 pt-8">
        <div className="flex items-center justify-between gap-4 mb-6">
          <div>
            <h2 className="m-0 text-xl font-medium text-white tracking-tight">
              {t("dictionary.title")}
            </h2>
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
      </section>
    </div>
  );
}
