import { createContext, useContext, useEffect, useState, ReactNode, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

type Config = Record<string, any>;

interface ConfigContextType {
  config: Config;
  updateConfig: (key: string, value: any) => Promise<void>;
  getConfig: (key: string, defaultValue?: any) => any;
}

const ConfigContext = createContext<ConfigContextType | undefined>(undefined);

export function ConfigProvider({ children }: { children: ReactNode }) {
  const [config, setConfig] = useState<Config>({});

  const loadConfig = useCallback(async () => {
    try {
      const configStr = await invoke<string>("load_config");
      const parsed = JSON.parse(configStr || "{}");
      setConfig(parsed);
    } catch (err) {
      console.error("Failed to load config from backend:", err);
    }
  }, []);

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  const updateConfig = async (key: string, value: any) => {
    const newConfig = { ...config, [key]: value };
    setConfig(newConfig);

    try {
      await invoke("save_config", { 
        config: JSON.stringify(newConfig) 
      });
    } catch (err) {
      console.error("Failed to save config to backend:", err);
    }
  };

  const getConfig = (key: string, defaultValue: any = null) => {
    return config[key] !== undefined ? config[key] : defaultValue;
  };

  return (
    <ConfigContext.Provider value={{ config, updateConfig, getConfig }}>
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
