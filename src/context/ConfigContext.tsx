import { createContext, useContext, useEffect, useState, ReactNode, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";

type Config = Record<string, any>;

interface ConfigContextType {
  config: Config;
  updateConfig: (key: string, value: any) => Promise<void>;
  /**
   * Same as updateConfig, but the disk write is coalesced. Use it for anything
   * driven by keystrokes (text fields, number steppers) so typing a value does
   * not rewrite config.json once per character.
   */
  updateConfigDebounced: (key: string, value: any, delayMs?: number) => void;
  getConfig: (key: string, defaultValue?: any) => any;
}

const ConfigContext = createContext<ConfigContextType | undefined>(undefined);

export function ConfigProvider({ children }: { children: ReactNode }) {
  const [config, setConfig] = useState<Config>({});
  // Single mutable snapshot so concurrent updateConfig calls never spread a
  // stale `config` closure over each other (that race reverted freshly saved
  // values when several settings were written in the same tick).
  const configRef = useRef<Config>({});

  const loadConfig = useCallback(async () => {
    try {
      const configStr = await invoke<string>("load_config");
      const parsed = JSON.parse(configStr || "{}");
      // Updates made while the initial load was in flight win over disk.
      configRef.current = { ...parsed, ...configRef.current };
      setConfig(configRef.current);
    } catch (err) {
      console.error("Failed to load config from backend:", err);
    }
  }, []);

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const persist = async () => {
    try {
      await invoke("save_config", {
        config: JSON.stringify(configRef.current)
      });
    } catch (err) {
      console.error("Failed to save config to backend:", err);
    }
  };

  const updateConfig = async (key: string, value: any) => {
    configRef.current = { ...configRef.current, [key]: value };
    setConfig(configRef.current);

    // A pending debounced write is now redundant: this save already carries the
    // whole snapshot, including whatever that timer was waiting to flush.
    if (saveTimerRef.current) {
      clearTimeout(saveTimerRef.current);
      saveTimerRef.current = null;
    }
    await persist();
  };

  // One shared timer, because save_config always writes the whole snapshot:
  // per-key timers would just make N redundant writes of the same object.
  const updateConfigDebounced = (key: string, value: any, delayMs = 400) => {
    configRef.current = { ...configRef.current, [key]: value };
    setConfig(configRef.current);

    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    saveTimerRef.current = setTimeout(() => {
      saveTimerRef.current = null;
      persist();
    }, delayMs);
  };

  // Never lose the last keystrokes to an unmount (window close, view teardown).
  useEffect(() => {
    return () => {
      if (saveTimerRef.current) {
        clearTimeout(saveTimerRef.current);
        saveTimerRef.current = null;
        persist();
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const getConfig = (key: string, defaultValue: any = null) => {
    return config[key] !== undefined ? config[key] : defaultValue;
  };

  return (
    <ConfigContext.Provider
      value={{ config, updateConfig, updateConfigDebounced, getConfig }}
    >
      {children}
    </ConfigContext.Provider>
  );
}

export const useConfig = () => {
  const context = useContext(ConfigContext);
  if (context === undefined) {
    throw new Error("useConfig must be used within a ConfigProvider");
  }
  return context;
};
