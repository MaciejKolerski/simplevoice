import { invoke } from "@tauri-apps/api/core";
import i18n from "./index";

// Build the tray label set from the current language and hand it to the backend,
// which caches it and rebuilds the native tray. Tray translations therefore live
// in the same JSON files as the rest of the UI.
export function pushTrayLabels(): Promise<void> {
  const t = i18n.t.bind(i18n);
  const labels = {
    start_recording: t("tray.startRecording"),
    stop_recording: t("tray.stopRecording"),
    copy_last: t("tray.copyLast"),
    usage: t("nav.usage"),
    models: t("nav.models"),
    history: t("nav.transcriptions"),
    settings: t("nav.settings"),
    lock_window: t("tray.lockWindow"),
    unlock_window: t("tray.unlockWindow"),
    select_microphone: t("tray.selectMicrophone"),
    default_microphone: t("tray.defaultMicrophone"),
    quit: t("tray.quit"),
  };
  return invoke<void>("set_tray_labels", { labels }).catch((err) =>
    console.error("i18n: failed to push tray labels:", err),
  );
}
