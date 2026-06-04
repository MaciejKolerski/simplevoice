import { invoke } from "@tauri-apps/api/core";
import i18n from "./index";
import { isSupported, Language } from "./detect";
import { pushTrayLabels } from "./tray";

// Read the persisted ui_language from config.json. If present, apply it. If absent
// (first run), persist whatever the OS-detected default resolved to at init.
export async function applyPersistedLanguage(): Promise<void> {
  try {
    const cfg = JSON.parse((await invoke<string>("load_config")) || "{}");
    const saved = cfg.ui_language;
    if (typeof saved === "string" && isSupported(saved)) {
      if (i18n.language !== saved) await i18n.changeLanguage(saved);
    } else {
      await persistLanguage(i18n.language as Language);
    }
  } catch (err) {
    console.error("i18n: failed to apply persisted language:", err);
  }
  await pushTrayLabels();
}

async function persistLanguage(lang: Language): Promise<void> {
  const cfg = JSON.parse((await invoke<string>("load_config")) || "{}");
  cfg.ui_language = lang;
  await invoke("save_config", { config: JSON.stringify(cfg) });
}

export async function changeLanguage(lang: Language): Promise<void> {
  await i18n.changeLanguage(lang);
  await persistLanguage(lang);
  await pushTrayLabels();
}
