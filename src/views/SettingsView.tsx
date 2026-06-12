import { useEffect, useState, useRef } from "react";
import { Cpu, Shield, Keyboard, Check, Mic, Info, RefreshCw, Languages, Radio } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen, emit } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { enable, isEnabled, disable } from "@tauri-apps/plugin-autostart";
import { useConfig } from "../context/ConfigContext";
import { useTranslation, Trans } from "react-i18next";
import { changeLanguage } from "@/i18n/language";
import { SUPPORTED_LANGUAGES, Language } from "@/i18n/detect";
import { toast } from "sonner";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import { SettingRow } from "@/components/ui/setting-row";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

function formatShortcutDisplay(str: string, noneLabel: string): string {
  if (!str) return noneLabel;
  return str
    .split("+")
    .map((part) => {
      if (part === "CommandOrControl") return "Cmd";
      if (part === "Command") return "Cmd";
      if (part === "Control") return "Ctrl";
      if (part === "Shift") return "Shift";
      if (part === "Alt") return "Option";
      if (part === "Super") return "Super";
      return part;
    })
    .join(" + ");
}

function formatKeycapLabel(key: string): string {
  const isMac = navigator.userAgent.includes("Mac");
  if (key === "CommandOrControl") return isMac ? "⌘ Cmd" : "Ctrl";
  if (key === "Command") return "⌘ Cmd";
  if (key === "Control") return "Ctrl";
  if (key === "Shift") return isMac ? "⇧ Shift" : "Shift";
  if (key === "Alt") return isMac ? "⌥ Opt" : "Alt";
  if (key === "Super") return "Super";
  return key;
}

const LANGUAGE_GROUPS: { label: string; labelKey: string; items: [string, string][] }[] = [
  {
    label: "Popular",
    labelKey: "settings.languageGroupPopular",
    items: [
      ["en", "English"],
      ["fr", "French (Français)"],
      ["de", "German (Deutsch)"],
      ["it", "Italian (Italiano)"],
      ["pl", "Polish (Polski)"],
      ["es", "Spanish (Español)"],
    ],
  },
  {
    label: "Europe",
    labelKey: "settings.languageGroupEurope",
    items: [
      ["bg", "Bulgarian (Български)"],
      ["hr", "Croatian (Hrvatski)"],
      ["cs", "Czech (Čeština)"],
      ["da", "Danish (Dansk)"],
      ["nl", "Dutch (Nederlands)"],
      ["fi", "Finnish (Suomi)"],
      ["el", "Greek (Ελληνικά)"],
      ["hu", "Hungarian (Magyar)"],
      ["no", "Norwegian (Norsk)"],
      ["pt", "Portuguese (Português)"],
      ["ro", "Romanian (Română)"],
      ["ru", "Russian (Русский)"],
      ["sr", "Serbian (Српски)"],
      ["sk", "Slovak (Slovenčina)"],
      ["sv", "Swedish (Svenska)"],
      ["tr", "Turkish (Türkçe)"],
      ["uk", "Ukrainian (Українська)"],
    ],
  },
  {
    label: "Asia & Middle East",
    labelKey: "settings.languageGroupAsiaMiddleEast",
    items: [
      ["ar", "Arabic (العربية)"],
      ["zh", "Chinese (中文)"],
      ["he", "Hebrew (עברית)"],
      ["hi", "Hindi (हिन्दी)"],
      ["id", "Indonesian (Bahasa Indonesia)"],
      ["ja", "Japanese (日本語)"],
      ["ko", "Korean (한국어)"],
      ["ms", "Malay (Bahasa Melayu)"],
      ["fa", "Persian (فارسی)"],
      ["th", "Thai (ไทย)"],
      ["vi", "Vietnamese (Tiếng Việt)"],
    ],
  },
  {
    label: "Other",
    labelKey: "settings.languageGroupOther",
    items: [
      ["af", "Afrikaans"],
      ["sw", "Swahili (Kiswahili)"],
      ["tl", "Tagalog"],
    ],
  },
];

const LANGUAGE_NAMES: Record<string, string> = Object.fromEntries(
  LANGUAGE_GROUPS.flatMap((g) => g.items),
);

const LIVE_SPEED_MS: Record<string, number> = {
  fast: 350,
  balanced: 600,
  accurate: 1000,
};

export function SettingsView() {
  const { updateConfig, getConfig, config } = useConfig();
  const { t, i18n } = useTranslation();
  const [vadEnabled, setVadEnabled] = useState(false);
  const [liveEnabled, setLiveEnabled] = useState(false);
  const [liveAutopaste, setLiveAutopaste] = useState(true);
  const [liveOverlayMode, setLiveOverlayMode] = useState("full");
  const [liveSpeed, setLiveSpeed] = useState("balanced");
  const [soundEnabled, setSoundEnabled] = useState(true);
  const [pauseAudioEnabled, setPauseAudioEnabled] = useState(false);
  const [gpuEnabled, setGpuEnabled] = useState(true);
  const [asrLanguage, setAsrLanguage] = useState("auto");
  const [autostartEnabled, setAutostartEnabled] = useState(false);
  const [recordingWindowMode, setRecordingWindowMode] = useState("always");
  const [appVersion, setAppVersion] = useState("");
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [barUnlocked, setBarUnlocked] = useState(false);

  const [devices, setDevices] = useState<string[]>([]);
  const [selectedDevice, setSelectedDevice] = useState<string>("");
  const [isRecordingShortcut, setIsRecordingShortcut] = useState(false);
  const [shortcutTarget, setShortcutTarget] = useState<
    "record" | "copy" | null
  >(null);
  const [shortcutText, setShortcutText] = useState(
    "CommandOrControl+Shift+Space",
  );
  const [copyShortcutText, setCopyShortcutText] = useState(
    "CommandOrControl+Shift+C",
  );
  const [activeKeys, setActiveKeys] = useState<string[]>([]);
  const [isCompleted, setIsCompleted] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  const isCompletedRef = useRef(false);
  const shortcutTargetRef = useRef<"record" | "copy" | null>(null);

  const startRecordingShortcut = (target: "record" | "copy") => {
    setShortcutTarget(target);
    setIsRecordingShortcut(true);
    setActiveKeys([]);
    setErrorMessage(null);
    setIsCompleted(false);
    shortcutTargetRef.current = target;
  };

  // Permission and environment states
  const [accessibilityGranted, setAccessibilityGranted] = useState(true);
  const [microphoneGranted, setMicrophoneGranted] = useState(true);
  const [platform, setPlatform] = useState("unknown");
  const [desktopEnv, setDesktopEnv] = useState("none");
  const isMac = platform === "macos";
  const [shortcutError, setShortcutError] = useState<string | null>(null);
  const [copyShortcutError, setCopyShortcutError] = useState<string | null>(null);
  const [showManualWMInstructions, setShowManualWMInstructions] = useState(false);

  useEffect(() => {
    isCompletedRef.current = isCompleted;
  }, [isCompleted]);

  useEffect(() => {
    shortcutTargetRef.current = shortcutTarget;
  }, [shortcutTarget]);

  useEffect(() => {
    const saved =
      localStorage.getItem("global_record_shortcut") ||
      "CommandOrControl+Shift+Space";
    setShortcutText(saved);
    invoke("register_shortcut", { shortcutStr: saved }).catch((err) => {
      console.error("Failed to register shortcut on mount:", err);
      setShortcutError(String(err));
    });

    const savedCopy =
      localStorage.getItem("global_copy_shortcut") ||
      "CommandOrControl+Shift+C";
    setCopyShortcutText(savedCopy);
    invoke("register_copy_shortcut", { shortcutStr: savedCopy }).catch(
      (err) => {
        console.error("Failed to register copy shortcut on mount:", err);
        setCopyShortcutError(String(err));
      },
    );

    const savedVad = localStorage.getItem("vad_enabled") === "true";
    setVadEnabled(savedVad);
    invoke("set_vad_enabled", { enabled: savedVad }).catch((err) => {
      console.error("Failed to set VAD state on mount:", err);
    });

    // App.tsx reads this flag from localStorage in its event handlers, so
    // localStorage stays the frontend store. Mirror it to config.json for the
    // backend (is_live_transcription_enabled) only when the key actually
    // exists: a fresh webview storage (dev build, reinstall) must not clobber
    // a setting some other install already persisted.
    const storedLive = localStorage.getItem("live_transcription_enabled");
    setLiveEnabled(storedLive === "true");
    if (storedLive !== null) {
      updateConfig("live_transcription_enabled", storedLive === "true");
    }

    // Frontend-only flag (App.tsx reads it); default on.
    setLiveAutopaste(localStorage.getItem("live_autopaste") !== "false");

    const savedOverlayTextMode =
      localStorage.getItem("live_overlay_mode") || "full";
    setLiveOverlayMode(savedOverlayTextMode);

    const savedLang = localStorage.getItem("asr_language") || "auto";
    setAsrLanguage(savedLang);

    isEnabled().then(setAutostartEnabled);

    const savedOverlayMode = localStorage.getItem("recording_window_mode") || "always";
    setRecordingWindowMode(savedOverlayMode);
    invoke("set_recording_window_mode", { mode: savedOverlayMode }).catch((err) => {
      console.error("Failed to initialize recording window mode:", err);
    });
  }, []);

  // config.json is the source of truth for settings the backend reads at
  // runtime. It loads asynchronously, so re-sync the switches whenever it
  // arrives instead of seeding them from localStorage (which differs between
  // dev and installed builds and used to silently overwrite the config).
  useEffect(() => {
    setSoundEnabled(getConfig("sound_feedback_enabled", true) !== false);
    setPauseAudioEnabled(getConfig("pause_audio_on_record", false) === true);
    const chunkMs = getConfig("live_min_chunk_ms", null);
    const speed = Object.entries(LIVE_SPEED_MS).find(([, ms]) => ms === chunkMs)?.[0];
    if (speed) {
      setLiveSpeed(speed);
    }
  }, [config]);

  useEffect(() => {
    getVersion()
      .then(setAppVersion)
      .catch((err) => console.error("Failed to read app version:", err));

    const handleCheckComplete = () => setCheckingUpdate(false);
    window.addEventListener("update-check-complete", handleCheckComplete);
    return () =>
      window.removeEventListener("update-check-complete", handleCheckComplete);
  }, []);

  // Check system permissions and Wayland status on mount and periodically
  useEffect(() => {
    const checkPermissions = async () => {
      try {
        const status = await invoke<{
          accessibility: boolean;
          microphone: boolean;
          platform: string;
          is_wayland: boolean;
          desktop_env: string;
        }>("check_permissions_status");
        setAccessibilityGranted(status.accessibility);
        setMicrophoneGranted(status.microphone);
        setPlatform(status.platform);
        setDesktopEnv(status.desktop_env);
      } catch (err) {
        console.error("Failed to check permissions:", err);
      }
    };
    checkPermissions();

    // Re-check every 5 seconds so the UI updates after the user grants access
    const interval = setInterval(checkPermissions, 5000);
    return () => clearInterval(interval);
  }, []);

  useEffect(() => {
    if (!isRecordingShortcut) return;

    setActiveKeys([]);
    setIsCompleted(false);
    setErrorMessage(null);

    const currentTarget = shortcutTargetRef.current;

    // Helper to determine active modifier keys based on target platform
    const getModifiers = (event: KeyboardEvent) => {
      const mods: string[] = [];
      const isMac = platform === "macos";
      if (isMac) {
        if (event.metaKey || event.ctrlKey) {
          mods.push("CommandOrControl");
        }
      } else {
        if (event.ctrlKey) {
          mods.push("Control");
        }
        if (event.metaKey) {
          mods.push("Super");
        }
      }
      if (event.altKey) mods.push("Alt");
      if (event.shiftKey) mods.push("Shift");
      return mods;
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (isCompletedRef.current) {
        e.preventDefault();
        e.stopPropagation();
        return;
      }

      e.preventDefault();
      e.stopPropagation();

      if (e.key === "Escape") {
        setIsRecordingShortcut(false);
        return;
      }

      const keys = getModifiers(e);
      const isModifier = ["Control", "Shift", "Alt", "Meta", "OS", "Super"].includes(e.key);

      if (!isModifier) {
        let mainKey = e.key;
        // Normalize spacebar inputs across different layout/platform representations
        if (
          mainKey === " " ||
          mainKey === " " ||
          mainKey === "\xa0" ||
          mainKey === "Spacebar"
        ) {
          mainKey = "Space";
        } else if (mainKey.length === 1) {
          mainKey = mainKey.toUpperCase();
        } else {
          mainKey = mainKey.charAt(0).toUpperCase() + mainKey.slice(1);
        }

        keys.push(mainKey);
        setActiveKeys(keys);
        setIsCompleted(true);
        setErrorMessage(null);

        const shortcutStr = keys.join("+");

        const commandName =
          currentTarget === "copy"
            ? "register_copy_shortcut"
            : "register_shortcut";
        const storageKey =
          currentTarget === "copy"
            ? "global_copy_shortcut"
            : "global_record_shortcut";

        invoke(commandName, { shortcutStr: shortcutStr })
          .then(() => {
            localStorage.setItem(storageKey, shortcutStr);
            if (currentTarget === "copy") {
              setCopyShortcutText(shortcutStr);
              setCopyShortcutError(null);
            } else {
              setShortcutText(shortcutStr);
              setShortcutError(null);
            }
            setTimeout(() => {
              setIsRecordingShortcut(false);
              setIsCompleted(false);
            }, 800);
          })
          .catch((err) => {
            console.error("Failed to register shortcut:", err);
            setIsCompleted(false);
            setActiveKeys([]);
            setErrorMessage(
              shortcutStr.includes("+")
                ? t("settings.shortcutSystemError", { error: String(err) })
                : t("settings.shortcutNeedsModifier"),
            );
            setTimeout(() => {
              setErrorMessage(null);
            }, 4000);
          });
      } else {
        setActiveKeys(keys);
      }
    };

    const handleKeyUp = (e: KeyboardEvent) => {
      if (isCompletedRef.current) {
        e.preventDefault();
        e.stopPropagation();
        return;
      }

      e.preventDefault();
      e.stopPropagation();

      const keys = getModifiers(e);
      setActiveKeys(keys);
    };

    window.addEventListener("keydown", handleKeyDown, true);
    window.addEventListener("keyup", handleKeyUp, true);
    return () => {
      window.removeEventListener("keydown", handleKeyDown, true);
      window.removeEventListener("keyup", handleKeyUp, true);
    };
  }, [isRecordingShortcut, platform]);

  useEffect(() => {
    const loadDevices = async () => {
      try {
        const list = await invoke<string[]>("list_audio_devices");
        setDevices(list);

        const saved = localStorage.getItem("selected_audio_device");
        // Preserve the explicit choice when present, or when the list came back
        // empty (transient glitch); only reset to the system default when the
        // saved device is genuinely missing from a populated list.
        if (saved && (list.includes(saved) || list.length === 0)) {
          setSelectedDevice(saved);
          await invoke("set_selected_device", { device: saved });
        } else {
          setSelectedDevice("default");
          await invoke("set_selected_device", { device: null });
        }
      } catch (err) {
        console.error("Failed to load audio devices:", err);
      }
    };
    loadDevices();
  }, []);

  // Load GPU setting
  useEffect(() => {
    const loadGpuSetting = async () => {
      try {
        const enabled = await invoke<boolean>("get_gpu_enabled");
        setGpuEnabled(enabled);
      } catch (err) {
        console.error("Failed to load GPU setting:", err);
      }
    };
    loadGpuSetting();
  }, []);

  useEffect(() => {
    const unlisten = listen<string | null>("device-changed", (event) => {
      setSelectedDevice(event.payload || "default");
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  useEffect(() => {
    invoke<boolean>("is_recording_window_locked_cmd")
      .then((locked) => setBarUnlocked(!locked))
      .catch(() => {});
    const unlisten = listen<boolean>("recording-window-lock-status", (event) => {
      setBarUnlocked(!event.payload);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const handleDeviceChange = async (val: string) => {
    setSelectedDevice(val);
    try {
      if (val === "default") {
        localStorage.removeItem("selected_audio_device");
        await invoke("set_selected_device", { device: null });
      } else {
        localStorage.setItem("selected_audio_device", val);
        await invoke("set_selected_device", { device: val });
      }
    } catch (err) {
      console.error("Failed to set selected device:", err);
    }
  };

  const handleVadToggle = async (checked: boolean) => {
    setVadEnabled(checked);
    localStorage.setItem("vad_enabled", String(checked));
    try {
      await invoke("set_vad_enabled", { enabled: checked });
    } catch (err) {
      console.error("Failed to set VAD state:", err);
    }
  };

  const handleLiveToggle = (checked: boolean) => {
    setLiveEnabled(checked);
    localStorage.setItem("live_transcription_enabled", String(checked));
    updateConfig("live_transcription_enabled", checked);
  };

  const handleLiveAutopasteToggle = (checked: boolean) => {
    setLiveAutopaste(checked);
    localStorage.setItem("live_autopaste", String(checked));
  };

  const handleOverlayModeChange = (mode: string) => {
    setLiveOverlayMode(mode);
    localStorage.setItem("live_overlay_mode", mode);
    // Notify the (separate) overlay window so it updates live.
    emit("live-overlay-mode-changed", mode).catch(() => {});
  };

  const handleSpeedChange = (speed: string) => {
    setLiveSpeed(speed);
    updateConfig("live_min_chunk_ms", LIVE_SPEED_MS[speed] ?? 600);
  };

  const handleSoundToggle = (checked: boolean) => {
    setSoundEnabled(checked);
    updateConfig("sound_feedback_enabled", checked);
  };

  const handlePauseAudioToggle = (checked: boolean) => {
    setPauseAudioEnabled(checked);
    updateConfig("pause_audio_on_record", checked);
  };

  const handleAsrLanguageChange = (val: string) => {
    setAsrLanguage(val);
    localStorage.setItem("asr_language", val);
  };

  const handleRecordingWindowModeChange = async (val: string) => {
    setRecordingWindowMode(val);
    localStorage.setItem("recording_window_mode", val);
    try {
      await invoke("set_recording_window_mode", { mode: val });
    } catch (err) {
      console.error("Failed to set recording window mode:", err);
    }
  };

  const handleBarLockToggle = async (checked: boolean) => {
    setBarUnlocked(checked);
    try {
      await invoke("set_recording_window_locked", { locked: !checked });
    } catch (err) {
      console.error("Failed to toggle recording bar lock:", err);
    }
  };

  const handleResetBarPosition = async () => {
    try {
      await invoke("reset_recording_window_position");
      toast.success(t("settings.barPositionResetDone"));
    } catch (err) {
      console.error("Failed to reset recording bar position:", err);
      toast.error(t("settings.barPositionResetError"));
    }
  };

  const handleAutostartToggle = async (checked: boolean) => {
    setAutostartEnabled(checked);
    if (checked) {
      await enable();
    } else {
      await disable();
    }
  };

  const handleGpuToggle = async (checked: boolean) => {
    setGpuEnabled(checked);
    try {
      await invoke("set_gpu_enabled", { enabled: checked });
      console.log(`[Settings] GPU toggled to: ${checked}`);

      const activeModel = localStorage.getItem("active_local_model_path");
      if (activeModel) {
        console.log(`[Settings] Reloading model with GPU=${checked}: ${activeModel}`);
        await invoke("load_model", { modelPath: activeModel });
      } else {
        console.log("[Settings] No active model to reload");
      }
    } catch (err) {
      console.error("Failed to set GPU state:", err);
    }
  };

  const handleCheckForUpdates = () => {
    setCheckingUpdate(true);
    window.dispatchEvent(new Event("check-for-updates"));
  };

  const micItems: Record<string, string> = {
    default: t("settings.defaultMicrophone"),
    ...Object.fromEntries(devices.map((d) => [d, d])),
  };

  const languageLabels: Record<string, string> = {
    auto: t("settings.autoDetect"),
    ...LANGUAGE_NAMES,
  };

  const recordingModeLabels: Record<string, string> = {
    always: t("settings.recordingModeAlways"),
    recording: t("settings.recordingModeRecording"),
    never: t("settings.recordingModeNever"),
  };

  return (
    <div className="flex flex-col">
      <div className="mb-6">
        <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
          {t("settings.preferencesTitle")}
        </h1>
      </div>

      <div className="w-full columns-1 lg:columns-2 2xl:columns-3 gap-6 [&>section]:mb-6 [&>section]:break-inside-avoid">
        {/* GROUP: Interface */}
        <section>
          <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
            <Languages size={16} className="text-muted" /> {t("settings.interfaceLanguageGroup")}
          </h2>
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">
            <SettingRow layout="column" title={t("settings.interfaceLanguage")}>
              <Select
                value={i18n.language}
                onValueChange={(v) => changeLanguage((v ?? "en") as Language)}
                items={Object.fromEntries(
                  SUPPORTED_LANGUAGES.map((l) => [l, t(`languages.${l}`)]),
                )}
              >
                <SelectTrigger className="w-full bg-black">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {SUPPORTED_LANGUAGES.map((l) => (
                    <SelectItem key={l} value={l}>
                      {t(`languages.${l}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </SettingRow>
          </div>
        </section>

        {/* GROUP: Audio & Speech-to-Text */}
        <section>
          <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
            <Cpu size={16} className="text-muted" /> {t("settings.audioSttGroup")}
          </h2>
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">
          <SettingRow layout="column" title={t("settings.inputMicrophone")}>
            <Select
              value={selectedDevice || "default"}
              onValueChange={(v) => handleDeviceChange(v ?? "default")}
              items={micItems}
            >
              <SelectTrigger className="w-full bg-black">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="default">{t("settings.defaultMicrophone")}</SelectItem>
                {devices.map((device) => (
                  <SelectItem key={device} value={device}>
                    {device}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </SettingRow>

          <SettingRow layout="column" title={t("settings.transcriptionLanguage")} data-tour="language-select">
            <Select
              value={asrLanguage}
              onValueChange={(v) => handleAsrLanguageChange(v ?? "auto")}
              items={languageLabels}
            >
              <SelectTrigger className="w-full bg-black">
                <SelectValue />
              </SelectTrigger>
              <SelectContent className="max-h-80">
                <SelectItem value="auto">{t("settings.autoDetect")}</SelectItem>
                {LANGUAGE_GROUPS.map((group) => (
                  <SelectGroup key={group.label}>
                    <SelectLabel>{t(group.labelKey)}</SelectLabel>
                    {group.items.map(([code, label]) => (
                      <SelectItem key={code} value={code}>
                        {label}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                ))}
              </SelectContent>
            </Select>
            <p className="text-muted text-[13px] mt-2">
              {t("settings.transcriptionLanguageHelp")}
            </p>
          </SettingRow>

          {!isMac && (
            <SettingRow
              title={t("settings.gpuAcceleration")}
              description={t("settings.gpuAccelerationDesc")}
            >
              <Switch checked={gpuEnabled} onCheckedChange={handleGpuToggle} />
            </SettingRow>
          )}
          </div>
        </section>

        {/* GROUP: Live transcription */}
        <section>
          <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
            <Radio size={16} className="text-muted" /> {t("settings.liveGroup")}
          </h2>
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">
            <SettingRow
              title={t("settings.liveTranscription")}
              description={t("settings.liveTranscriptionDesc")}
            >
              <Switch checked={liveEnabled} onCheckedChange={handleLiveToggle} />
            </SettingRow>

            <div
              className={
                liveEnabled ? "" : "opacity-50 pointer-events-none select-none"
              }
              aria-disabled={!liveEnabled}
            >
              <SettingRow
                title={t("settings.liveAutopaste")}
                description={t("settings.liveAutopasteDesc")}
              >
                <Switch
                  checked={liveAutopaste}
                  disabled={!liveEnabled}
                  onCheckedChange={handleLiveAutopasteToggle}
                />
              </SettingRow>

              <SettingRow
                layout="column"
                title={t("settings.liveOverlayText")}
                description={t("settings.liveOverlayTextDesc")}
              >
                <Select
                  value={liveOverlayMode}
                  onValueChange={(v) => handleOverlayModeChange(v ?? "full")}
                  items={{
                    full: t("settings.liveOverlayFull"),
                    recent: t("settings.liveOverlayRecent"),
                  }}
                >
                  <SelectTrigger className="w-full bg-black" disabled={!liveEnabled}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="full">
                      {t("settings.liveOverlayFull")}
                    </SelectItem>
                    <SelectItem value="recent">
                      {t("settings.liveOverlayRecent")}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </SettingRow>

              <SettingRow
                layout="column"
                title={t("settings.liveSpeed")}
                description={t("settings.liveSpeedDesc")}
              >
                <Select
                  value={liveSpeed}
                  onValueChange={(v) => handleSpeedChange(v ?? "balanced")}
                  items={{
                    fast: t("settings.liveSpeedFast"),
                    balanced: t("settings.liveSpeedBalanced"),
                    accurate: t("settings.liveSpeedAccurate"),
                  }}
                >
                  <SelectTrigger className="w-full bg-black" disabled={!liveEnabled}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="fast">
                      {t("settings.liveSpeedFast")}
                    </SelectItem>
                    <SelectItem value="balanced">
                      {t("settings.liveSpeedBalanced")}
                    </SelectItem>
                    <SelectItem value="accurate">
                      {t("settings.liveSpeedAccurate")}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </SettingRow>
            </div>
          </div>
        </section>

        {/* GROUP: Recording & Feedback */}
        <section data-tour="recording-section">
          <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
            <Mic size={16} className="text-muted" /> {t("settings.recordingFeedbackGroup")}
          </h2>
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">
            <SettingRow
              title={t("settings.autoStart")}
              description={t("settings.autoStartDesc")}
            >
              <Switch
                checked={autostartEnabled}
                onCheckedChange={handleAutostartToggle}
              />
            </SettingRow>

          <SettingRow title={t("settings.vad")} description={t("settings.vadDesc")}>
            <Switch checked={vadEnabled} onCheckedChange={handleVadToggle} />
          </SettingRow>

          <SettingRow title={t("settings.soundEffects")} description={t("settings.soundEffectsDesc")}>
            <Switch checked={soundEnabled} onCheckedChange={handleSoundToggle} />
          </SettingRow>

          <SettingRow title={t("settings.pauseSystemAudio")} description={t("settings.pauseSystemAudioDesc")}>
            <Switch
              checked={pauseAudioEnabled}
              onCheckedChange={handlePauseAudioToggle}
            />
          </SettingRow>

          {(isMac || platform === "linux" || platform === "windows") && (
            <>
              <SettingRow
                title={t("settings.recordingOverlayWindow")}
                description={t("settings.recordingOverlayWindowDesc")}
              >
                <Select
                  value={recordingWindowMode}
                  onValueChange={(v) => handleRecordingWindowModeChange(v ?? "always")}
                  items={recordingModeLabels}
                >
                  <SelectTrigger className="w-48 bg-black shrink-0">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="always">{t("settings.recordingModeAlways")}</SelectItem>
                    <SelectItem value="recording">{t("settings.recordingModeRecording")}</SelectItem>
                    <SelectItem value="never">{t("settings.recordingModeNever")}</SelectItem>
                  </SelectContent>
                </Select>
              </SettingRow>
              {isMac && (
                <SettingRow
                  title={t("settings.barPositionMoveTitle")}
                  description={
                    <Trans
                      i18nKey="settings.barPositionMoveDescMac"
                      components={{
                        kbd: (
                          <kbd className="inline-flex items-center justify-center px-1.5 py-0.5 mx-0.5 rounded-md border border-border bg-surface-active font-mono text-[11px] text-foreground" />
                        ),
                      }}
                    />
                  }
                />
              )}
              {(platform === "linux" || platform === "windows") && (
                <SettingRow
                  title={t("settings.barPositionUnlockTitle")}
                  description={t("settings.barPositionUnlockDesc")}
                >
                  <Switch checked={barUnlocked} onCheckedChange={handleBarLockToggle} />
                </SettingRow>
              )}
              <SettingRow
                title={t("settings.barPositionResetTitle")}
                description={t("settings.barPositionResetDesc")}
              >
                <Button variant="outline" size="sm" onClick={handleResetBarPosition}>
                  {t("settings.barPositionReset")}
                </Button>
              </SettingRow>
            </>
          )}
          </div>
        </section>

        {/* GROUP: Keyboard Shortcuts */}
        <section data-tour="shortcuts-section">
          <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
            <Keyboard size={16} className="text-muted" /> {t("settings.keyboardShortcutsGroup")}
          </h2>
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">

          <SettingRow
            title={t("settings.startStopRecording")}
            description={t("settings.startStopRecordingDesc")}
          >
            <button
              data-tour="record-shortcut"
              onClick={() => startRecordingShortcut("record")}
              className="font-mono text-sm px-3.5 py-1.5 bg-surface-active rounded-md border border-border text-foreground min-w-[150px] text-center hover:border-border-hover hover:bg-surface-hover active:scale-[0.985] transition-all select-none"
              title={t("settings.clickToChangeShortcut")}
            >
              {formatShortcutDisplay(shortcutText, t("settings.shortcutNone"))}
            </button>
          </SettingRow>

          <SettingRow
            title={t("settings.copyLastTranscription")}
            description={t("settings.copyLastTranscriptionDesc")}
          >
            <button
              onClick={() => startRecordingShortcut("copy")}
              className="font-mono text-sm px-3.5 py-1.5 bg-surface-active rounded-md border border-border text-foreground min-w-[150px] text-center hover:border-border-hover hover:bg-surface-hover active:scale-[0.985] transition-all select-none"
              title={t("settings.clickToChangeShortcut")}
            >
              {formatShortcutDisplay(copyShortcutText, t("settings.shortcutNone"))}
            </button>
          </SettingRow>

          {/* Linux Native / Wayland warning block */}
          {platform === "linux" && (
            <div className="p-5 pt-0">
              {["niri", "hyprland", "sway", "i3", "unknown"].includes(desktopEnv) ? (
                <div className="mt-4 p-4 bg-success/10 border border-success/20 rounded-lg text-success text-xs leading-relaxed flex flex-col gap-1.5">
                  <div className="font-semibold flex items-center gap-1.5 text-sm">
                    <Check size={14} /> {t("settings.nativeGlobalHotkeysActive")}
                  </div>
                  <p className="text-success/85">
                    {t("settings.nativeGlobalHotkeysActiveDesc")}
                  </p>
                  <button
                    onClick={() => setShowManualWMInstructions(!showManualWMInstructions)}
                    className="text-left text-[11px] text-warning hover:text-warning/80 underline font-medium mt-1 cursor-pointer select-none transition-colors bg-transparent border-0 p-0"
                  >
                    {showManualWMInstructions ? t("settings.hideTroubleshooting") : t("settings.showTroubleshooting")}
                  </button>

                  {showManualWMInstructions && (
                    <div className="mt-3 font-medium border-t border-success/10 pt-3 flex flex-col gap-2 text-muted text-[11px]">
                      <p>
                        <Trans
                          i18nKey="settings.hotkeyTroubleshootingInput"
                          components={{ strong: <strong /> }}
                        />
                      </p>
                      <pre className="bg-black/50 p-2.5 rounded font-mono text-[11px] text-warning/90 overflow-x-auto">sudo usermod -aG input $USER</pre>
                      <p>
                        {t("settings.hotkeyTroubleshootingObserved")}
                      </p>
                    </div>
                  )}
                </div>
              ) : ["gnome", "kde", "xfce", "cinnamon", "mate"].includes(desktopEnv) ? (
                <div className="mt-4 p-4 bg-success/10 border border-success/20 rounded-lg text-success text-xs leading-relaxed flex flex-col gap-1.5">
                  <div className="font-semibold flex items-center gap-1.5 text-sm">
                    <Check size={14} /> {t("settings.nativeLinuxShortcutIntegrationActive")}
                  </div>
                  <p className="text-success/85">
                    <Trans
                      i18nKey="settings.nativeLinuxShortcutIntegrationActiveDesc"
                      values={{
                        env:
                          desktopEnv === "gnome" ? "GNOME" :
                          desktopEnv === "kde" ? "KDE Plasma" :
                          desktopEnv === "xfce" ? "XFCE" :
                          desktopEnv === "cinnamon" ? "Cinnamon" : "MATE",
                      }}
                      components={{ strong: <strong /> }}
                    />
                  </p>
                </div>
              ) : null}
            </div>
          )}

          {/* Fallback generic error warning if registration fails on other platforms */}
          {platform !== "linux" && (shortcutError || copyShortcutError) && (
            <div className="p-5 pt-0">
              <Alert variant="destructive" className="mt-4 border-danger/20 bg-danger/5">
                <Shield />
                <AlertTitle>{t("settings.shortcutRegistrationError")}</AlertTitle>
                <AlertDescription>{shortcutError || copyShortcutError}</AlertDescription>
              </Alert>
            </div>
          )}
        </div>
        </section>

        {/* GROUP: System Permissions */}
        {platform === "macos" && (
          <section data-tour="permissions-section">
            <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
              <Shield size={16} className="text-muted" /> {t("settings.systemPermissionsGroup")}
            </h2>
            <div className="border border-border rounded-xl overflow-hidden bg-secondary">
              <SettingRow
                title={
                  <span className="flex items-center gap-2">
                    {t("settings.accessibility")}
                    <span
                      className={`inline-block w-2 h-2 rounded-full ${
                        accessibilityGranted
                          ? "bg-success shadow-[0_0_6px_rgba(52,211,153,0.5)]"
                          : "bg-warning shadow-[0_0_6px_rgba(251,191,36,0.5)] animate-pulse"
                      }`}
                    />
                  </span>
                }
                description={
                  <>
                    {t("settings.accessibilityDesc")}
                    {!accessibilityGranted && (
                      <span className="text-warning font-medium"> {t("settings.accessibilityNotGranted")}</span>
                    )}
                  </>
                }
              >
                {!accessibilityGranted ? (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={async () => {
                      try {
                        await invoke("request_accessibility_permission");
                      } catch (err) {
                        console.error("Failed to request accessibility:", err);
                      }
                    }}
                    className="border-warning/30 bg-warning/10 text-warning hover:bg-warning/20 hover:text-warning"
                  >
                    {t("settings.grantAccess")}
                  </Button>
                ) : (
                  <span className="inline-flex items-center gap-1.5 px-2 text-xs font-medium text-success">
                    <Check size={14} /> {t("settings.granted")}
                  </span>
                )}
              </SettingRow>

              <SettingRow
                title={
                  <span className="flex items-center gap-2">
                    {t("settings.microphone")}
                    <span
                      className={`inline-block w-2 h-2 rounded-full ${
                        microphoneGranted
                          ? "bg-success shadow-[0_0_6px_rgba(52,211,153,0.5)]"
                          : "bg-warning shadow-[0_0_6px_rgba(251,191,36,0.5)] animate-pulse"
                      }`}
                    />
                  </span>
                }
                description={
                  <>
                    {t("settings.microphoneDesc")}
                    {!microphoneGranted && (
                      <span className="text-warning font-medium"> {t("settings.microphoneNotGranted")}</span>
                    )}
                  </>
                }
              >
                {!microphoneGranted ? (
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={async () => {
                      try {
                        await invoke("open_folder", {
                          path: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
                        });
                      } catch {
                        // Fallback: open System Settings directly
                        await invoke("open_folder", {
                          path: "/System/Library/PreferencePanes/Security.prefPane",
                        });
                      }
                    }}
                    className="border-warning/30 bg-warning/10 text-warning hover:bg-warning/20 hover:text-warning"
                  >
                    {t("settings.grantAccess")}
                  </Button>
                ) : (
                  <span className="inline-flex items-center gap-1.5 px-2 text-xs font-medium text-success">
                    <Check size={14} /> {t("settings.granted")}
                  </span>
                )}
              </SettingRow>
            </div>
          </section>
        )}

        {/* GROUP: About */}
        <section>
          <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
            <Info size={16} className="text-muted" /> {t("settings.aboutGroup")}
          </h2>
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">
            <SettingRow
              title="Simplevoice"
              description={t("settings.version", { version: appVersion || "…" })}
            >
              <Button
                variant="outline"
                size="sm"
                onClick={handleCheckForUpdates}
                disabled={checkingUpdate}
              >
                <RefreshCw
                  size={14}
                  className={checkingUpdate ? "animate-spin" : ""}
                />
                {checkingUpdate ? t("settings.checkingForUpdates") : t("settings.checkForUpdates")}
              </Button>
            </SettingRow>
          </div>
        </section>
      </div>

      {isRecordingShortcut && (
        <div
          ref={overlayRef}
          onClick={(e) => {
            if (e.target === overlayRef.current && !isCompleted) {
              setIsRecordingShortcut(false);
            }
          }}
          className="fixed inset-0 z-50 flex flex-col items-center justify-center bg-black/90 backdrop-blur-xl transition-all duration-300 animate-[fadeIn_0.2s_ease-out]"
        >
          <div className="flex flex-col items-center justify-center text-center max-w-sm w-full mx-4">
            <div className="text-muted-dark font-mono text-[10px] uppercase tracking-[0.2em] mb-5 select-none">
              {shortcutTarget === "copy"
                ? t("settings.overlayCopyLastTranscription")
                : t("settings.overlayStartStopRecording")}
            </div>
            <div className="flex items-center justify-center gap-2 h-16 w-full mb-6">
              {activeKeys.length === 0 ? (
                <div className="text-white/20 font-mono text-[11px] tracking-[0.2em] animate-pulse select-none">
                  {errorMessage ? t("settings.invalidCombination") : t("settings.pressKeys")}
                </div>
              ) : (
                activeKeys.map((key, index) => (
                  <span key={key} className="inline-flex items-center">
                    {index > 0 && (
                      <span className="text-white/25 font-mono text-xs px-1 select-none animate-in fade-in duration-200">
                        +
                      </span>
                    )}
                    <kbd
                      className={`inline-flex items-center justify-center px-3.5 py-2 rounded-lg text-xs font-mono font-bold shadow-xl transition-all duration-200 animate-in fade-in slide-in-from-bottom-2 ${
                        isCompleted
                          ? "bg-white text-black border-white scale-105 shadow-[0_0_15px_rgba(255,255,255,0.25)]"
                          : "bg-white/10 text-white/90 border border-white/10"
                      }`}
                    >
                      {formatKeycapLabel(key)}
                    </kbd>
                  </span>
                ))
              )}
            </div>

            <div className="h-10 flex items-center justify-center">
              {isCompleted ? (
                <span className="text-success font-mono text-[10px] uppercase tracking-[0.25em] animate-in zoom-in-95 duration-200">
                  {t("settings.shortcutSaved")}
                </span>
              ) : errorMessage ? (
                <span className="text-danger/90 font-mono text-[10px] leading-relaxed max-w-[280px] animate-in fade-in duration-200">
                  {errorMessage}
                </span>
              ) : (
                <span className="text-white/30 font-mono text-[10px] uppercase tracking-[0.2em] select-none">
                  {t("settings.pressKeysToAssign")}
                </span>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
