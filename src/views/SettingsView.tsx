import { useEffect, useState, useRef } from "react";
import { Cpu, Shield, Keyboard, Check, Mic, Info, RefreshCw, Languages } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { enable, isEnabled, disable } from "@tauri-apps/plugin-autostart";
import { useConfig } from "../context/ConfigContext";
import { useTranslation } from "react-i18next";
import { changeLanguage } from "@/i18n/language";
import { SUPPORTED_LANGUAGES, Language } from "@/i18n/detect";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
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

function formatShortcutDisplay(str: string): string {
  if (!str) return "None";
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

const LANGUAGE_GROUPS: { label: string; items: [string, string][] }[] = [
  {
    label: "Popular",
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
    items: [
      ["af", "Afrikaans"],
      ["sw", "Swahili (Kiswahili)"],
      ["tl", "Tagalog"],
    ],
  },
];

const LANGUAGE_LABELS: Record<string, string> = {
  auto: "Auto-detect",
  ...Object.fromEntries(LANGUAGE_GROUPS.flatMap((g) => g.items)),
};

const RECORDING_MODE_LABELS: Record<string, string> = {
  always: "Always Show",
  recording: "Show During Recording",
  never: "Do Not Show",
};

export function SettingsView() {
  const { updateConfig } = useConfig();
  const { t, i18n } = useTranslation();
  const [vadEnabled, setVadEnabled] = useState(false);
  const [soundEnabled, setSoundEnabled] = useState(true);
  const [pauseAudioEnabled, setPauseAudioEnabled] = useState(false);
  const [gpuEnabled, setGpuEnabled] = useState(true);
  const [asrLanguage, setAsrLanguage] = useState("auto");
  const [autostartEnabled, setAutostartEnabled] = useState(false);
  const [recordingWindowMode, setRecordingWindowMode] = useState("always");
  const [appVersion, setAppVersion] = useState("");
  const [checkingUpdate, setCheckingUpdate] = useState(false);

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

    const savedSound =
      localStorage.getItem("sound_feedback_enabled") !== "false";
    setSoundEnabled(savedSound);
    updateConfig("sound_feedback_enabled", savedSound);

    const savedPauseAudio =
      localStorage.getItem("pause_audio_on_record") === "true";
    setPauseAudioEnabled(savedPauseAudio);
    updateConfig("pause_audio_on_record", savedPauseAudio);

    const savedLang = localStorage.getItem("asr_language") || "auto";
    setAsrLanguage(savedLang);

    isEnabled().then(setAutostartEnabled);

    const savedOverlayMode = localStorage.getItem("recording_window_mode") || "always";
    setRecordingWindowMode(savedOverlayMode);
    invoke("set_recording_window_mode", { mode: savedOverlayMode }).catch((err) => {
      console.error("Failed to initialize recording window mode:", err);
    });
  }, []);

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
                ? `System error: ${err}`
                : "Global shortcuts require a modifier (Cmd, Shift, Alt, etc.) or a Function key (F1-F12).",
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
        if (saved && list.includes(saved)) {
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

  const handleSoundToggle = (checked: boolean) => {
    setSoundEnabled(checked);
    localStorage.setItem("sound_feedback_enabled", String(checked));
    updateConfig("sound_feedback_enabled", checked);
  };

  const handlePauseAudioToggle = (checked: boolean) => {
    setPauseAudioEnabled(checked);
    localStorage.setItem("pause_audio_on_record", String(checked));
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
    default: "Default System Microphone",
    ...Object.fromEntries(devices.map((d) => [d, d])),
  };

  return (
    <div className="flex flex-col">
      <div className="mb-6">
        <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
          Preferences
        </h1>
      </div>

      <div className="w-full columns-1 lg:columns-2 2xl:columns-3 gap-6 [&>section]:mb-6 [&>section]:break-inside-avoid">
        {/* GROUP: Interface */}
        <section>
          <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
            <Languages size={16} className="text-muted" /> {t("settings.interfaceLanguageGroup")}
          </h2>
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">
            <div className="flex justify-between items-center gap-6 p-5">
              <div className="min-w-0">
                <div className="text-fg font-medium mb-1">{t("settings.interfaceLanguage")}</div>
                <div className="text-xs text-muted leading-snug">
                  {t("settings.interfaceLanguageDesc")}
                </div>
              </div>
              <Select
                value={i18n.language}
                onValueChange={(v) => changeLanguage((v ?? "en") as Language)}
                items={Object.fromEntries(
                  SUPPORTED_LANGUAGES.map((l) => [l, t(`languages.${l}`)]),
                )}
              >
                <SelectTrigger className="w-48 bg-black shrink-0">
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
            </div>
          </div>
        </section>

        {/* GROUP: Audio & Speech-to-Text */}
        <section>
          <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
            <Cpu size={16} className="text-muted" /> Audio &amp; Speech-to-Text
          </h2>
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">
          <div className="flex flex-col p-5 border-b border-border last:border-b-0">
            <Label className="mb-3">Input Microphone</Label>
            <Select
              value={selectedDevice || "default"}
              onValueChange={(v) => handleDeviceChange(v ?? "default")}
              items={micItems}
            >
              <SelectTrigger className="w-full bg-black">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="default">Default System Microphone</SelectItem>
                {devices.map((device) => (
                  <SelectItem key={device} value={device}>
                    {device}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div data-tour="language-select" className="flex flex-col p-5 border-b border-border last:border-b-0">
            <Label className="mb-3">Transcription Language</Label>
            <Select
              value={asrLanguage}
              onValueChange={(v) => handleAsrLanguageChange(v ?? "auto")}
              items={LANGUAGE_LABELS}
            >
              <SelectTrigger className="w-full bg-black">
                <SelectValue />
              </SelectTrigger>
              <SelectContent className="max-h-80">
                <SelectItem value="auto">Auto-detect</SelectItem>
                {LANGUAGE_GROUPS.map((group) => (
                  <SelectGroup key={group.label}>
                    <SelectLabel>{group.label}</SelectLabel>
                    {group.items.map(([code, label]) => (
                      <SelectItem key={code} value={code}>
                        {label}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                ))}
              </SelectContent>
            </Select>
            <p className="text-[11px] text-muted mt-2">
              Forces the model to output text in the selected language. Use
              "Auto-detect" for multilingual support.
            </p>
          </div>

          {!isMac && (
            <div className="flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0">
              <div className="min-w-0">
                <div className="text-fg font-medium mb-1">GPU Acceleration</div>
                <div className="text-xs text-muted leading-snug">
                  Use GPU (Vulkan on Linux/Windows) for faster transcription.
                  Falls back to CPU if no compatible GPU is available.
                </div>
              </div>
              <Switch checked={gpuEnabled} onCheckedChange={handleGpuToggle} />
            </div>
          )}
          </div>
        </section>

        {/* GROUP: Recording & Feedback */}
        <section data-tour="recording-section">
          <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
            <Mic size={16} className="text-muted" /> Recording &amp; Feedback
          </h2>
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">
            <div className="flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0">
              <div className="min-w-0">
                <div className="text-fg font-medium mb-1">Auto-start</div>
                <div className="text-xs text-muted">
                  Start Simplevoice automatically when you log in.
                </div>
              </div>
              <Switch
                checked={autostartEnabled}
                onCheckedChange={handleAutostartToggle}
              />
            </div>

          <div className="flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0">
            <div className="min-w-0">
              <div className="text-fg font-medium mb-1">
                Voice Activity Detection (VAD)
              </div>
              <div className="text-muted text-[13px]">
                Automatically stop recording when you stop speaking.
              </div>
            </div>
            <Switch checked={vadEnabled} onCheckedChange={handleVadToggle} />
          </div>

          <div className="flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0">
            <div className="min-w-0">
              <div className="text-fg font-medium mb-1">Sound Effects</div>
              <div className="text-muted text-[13px]">
                Play a premium audio cue when starting and stopping recording.
              </div>
            </div>
            <Switch checked={soundEnabled} onCheckedChange={handleSoundToggle} />
          </div>

          <div className="flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0">
            <div className="min-w-0">
              <div className="text-fg font-medium mb-1">Pause System Audio</div>
              <div className="text-muted text-[13px]">
                Automatically pause music or videos while recording and resume
                afterwards.
              </div>
            </div>
            <Switch
              checked={pauseAudioEnabled}
              onCheckedChange={handlePauseAudioToggle}
            />
          </div>

          {(isMac || platform === "linux" || platform === "windows") && (
            <div className="flex justify-between items-center gap-6 p-5">
              <div className="min-w-0">
                <div className="text-fg font-medium mb-1">
                  Recording Overlay Window
                </div>
                <div className="text-muted text-[13px]">
                  Display a floating wavebar on your screen reacting to voice.
                </div>
              </div>
              <Select
                value={recordingWindowMode}
                onValueChange={(v) => handleRecordingWindowModeChange(v ?? "always")}
                items={RECORDING_MODE_LABELS}
              >
                <SelectTrigger className="w-48 bg-black shrink-0">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="always">Always Show</SelectItem>
                  <SelectItem value="recording">Show During Recording</SelectItem>
                  <SelectItem value="never">Do Not Show</SelectItem>
                </SelectContent>
              </Select>
            </div>
          )}
          </div>
        </section>

        {/* GROUP: Keyboard Shortcuts */}
        <section data-tour="shortcuts-section">
          <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
            <Keyboard size={16} className="text-muted" /> Keyboard Shortcuts
          </h2>
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">

          <div className="flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0">
            <div className="flex-1 min-w-0">
              <div className="text-fg font-medium mb-1">
                Start / Stop Recording
              </div>
              <div className="text-muted text-[13px]">
                Global hotkey to toggle voice recording from anywhere.
              </div>
            </div>
            <button
              data-tour="record-shortcut"
              onClick={() => startRecordingShortcut("record")}
              className="font-mono text-sm px-3.5 py-1.5 bg-surface-active rounded-md border border-border text-foreground min-w-[150px] text-center hover:border-border-hover hover:bg-surface-hover active:scale-[0.985] transition-all select-none"
              title="Click to change shortcut"
            >
              {formatShortcutDisplay(shortcutText)}
            </button>
          </div>

          <div className="flex justify-between items-center gap-6 p-5">
            <div className="flex-1 min-w-0">
              <div className="text-fg font-medium mb-1">
                Copy Last Transcription
              </div>
              <div className="text-muted text-[13px]">
                Copy the most recent transcription result to clipboard.
              </div>
            </div>
            <button
              onClick={() => startRecordingShortcut("copy")}
              className="font-mono text-sm px-3.5 py-1.5 bg-surface-active rounded-md border border-border text-foreground min-w-[150px] text-center hover:border-border-hover hover:bg-surface-hover active:scale-[0.985] transition-all select-none"
              title="Click to change shortcut"
            >
              {formatShortcutDisplay(copyShortcutText)}
            </button>
          </div>

          {/* Linux Native / Wayland warning block */}
          {platform === "linux" && (
            <div className="p-5 pt-0 border-t border-border">
              {["niri", "hyprland", "sway", "i3", "unknown"].includes(desktopEnv) ? (
                <div className="mt-4 p-4 bg-success/10 border border-success/20 rounded-lg text-success text-xs leading-relaxed flex flex-col gap-1.5">
                  <div className="font-semibold flex items-center gap-1.5 text-sm">
                    <Check size={14} /> Native Global Hotkeys Active
                  </div>
                  <p className="text-success/85">
                    Your shortcuts are captured directly from your keyboard (evdev) and work globally on any
                    compositor — no configuration files are edited and no external tools are required.
                  </p>
                  <button
                    onClick={() => setShowManualWMInstructions(!showManualWMInstructions)}
                    className="text-left text-[11px] text-warning hover:text-warning/80 underline font-medium mt-1 cursor-pointer select-none transition-colors bg-transparent border-0 p-0"
                  >
                    {showManualWMInstructions ? "Hide troubleshooting" : "Hotkey not working? Show troubleshooting"}
                  </button>

                  {showManualWMInstructions && (
                    <div className="mt-3 font-medium border-t border-success/10 pt-3 flex flex-col gap-2 text-muted text-[11px]">
                      <p>
                        If the hotkey does nothing, your user probably cannot read input devices. Add yourself to
                        the <strong>input</strong> group and log back in:
                      </p>
                      <pre className="bg-black/50 p-2.5 rounded font-mono text-[11px] text-warning/90 overflow-x-auto">sudo usermod -aG input $USER</pre>
                      <p>
                        The hotkey is observed, not intercepted, so it also reaches the focused app — pick a
                        dedicated combination (for example one using Super) that nothing else uses.
                      </p>
                    </div>
                  )}
                </div>
              ) : ["gnome", "kde", "xfce", "cinnamon", "mate"].includes(desktopEnv) ? (
                <div className="mt-4 p-4 bg-success/10 border border-success/20 rounded-lg text-success text-xs leading-relaxed flex flex-col gap-1.5">
                  <div className="font-semibold flex items-center gap-1.5 text-sm">
                    <Check size={14} /> Native Linux Shortcut Integration Active
                  </div>
                  <p className="text-success/85">
                    Your shortcuts are registered directly in the <strong>{
                      desktopEnv === "gnome" ? "GNOME" :
                      desktopEnv === "kde" ? "KDE Plasma" :
                      desktopEnv === "xfce" ? "XFCE" :
                      desktopEnv === "cinnamon" ? "Cinnamon" : "MATE"
                    }</strong> keyboard configuration using built-in settings. They apply immediately!
                  </p>
                </div>
              ) : null}
            </div>
          )}

          {/* Fallback generic error warning if registration fails on other platforms */}
          {platform !== "linux" && (shortcutError || copyShortcutError) && (
            <div className="p-5 pt-0 border-t border-border">
              <Alert variant="destructive" className="mt-4 border-danger/20 bg-danger/5">
                <Shield />
                <AlertTitle>Shortcut Registration Error</AlertTitle>
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
              <Shield size={16} className="text-muted" /> System Permissions
            </h2>
            <div className="border border-border rounded-xl overflow-hidden bg-secondary">
              <div className="flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0">
                <div className="flex-1 min-w-0">
                  <div className="text-fg font-medium mb-1 flex items-center gap-2">
                    Accessibility
                    <span
                      className={`inline-block w-2 h-2 rounded-full ${
                        accessibilityGranted
                          ? "bg-success shadow-[0_0_6px_rgba(52,211,153,0.5)]"
                          : "bg-warning shadow-[0_0_6px_rgba(251,191,36,0.5)] animate-pulse"
                      }`}
                    />
                  </div>
                  <div className="text-muted text-[13px]">
                    Required for auto-paste (keyboard simulation via Cmd+V).
                    {!accessibilityGranted && (
                      <span className="text-warning font-medium">
                        {" "}
                        Not granted — auto-paste will not work.
                      </span>
                    )}
                  </div>
                </div>
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
                    Grant Access
                  </Button>
                ) : (
                  <span className="inline-flex items-center gap-1.5 px-2 text-xs font-medium text-success">
                    <Check size={14} /> Granted
                  </span>
                )}
              </div>

              <div className="flex justify-between items-center gap-6 p-5">
                <div className="flex-1 min-w-0">
                  <div className="text-fg font-medium mb-1 flex items-center gap-2">
                    Microphone
                    <span
                      className={`inline-block w-2 h-2 rounded-full ${
                        microphoneGranted
                          ? "bg-success shadow-[0_0_6px_rgba(52,211,153,0.5)]"
                          : "bg-warning shadow-[0_0_6px_rgba(251,191,36,0.5)] animate-pulse"
                      }`}
                    />
                  </div>
                  <div className="text-muted text-[13px]">
                    Required for audio capture.
                    {!microphoneGranted && (
                      <span className="text-warning font-medium">
                        {" "}
                        Not granted — recording will not work.
                      </span>
                    )}
                  </div>
                </div>
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
                    Grant Access
                  </Button>
                ) : (
                  <span className="inline-flex items-center gap-1.5 px-2 text-xs font-medium text-success">
                    <Check size={14} /> Granted
                  </span>
                )}
              </div>
            </div>
          </section>
        )}

        {/* GROUP: About */}
        <section>
          <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
            <Info size={16} className="text-muted" /> About
          </h2>
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">
            <div className="flex justify-between items-center gap-6 p-5">
              <div className="min-w-0">
                <div className="text-fg font-medium mb-1">Simplevoice</div>
                <div className="text-muted text-[13px]">
                  Version {appVersion || "…"}
                </div>
              </div>
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
                {checkingUpdate ? "Checking…" : "Check for updates"}
              </Button>
            </div>
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
                ? "Copy last transcription"
                : "Start / stop recording"}
            </div>
            <div className="flex items-center justify-center gap-2 h-16 w-full mb-6">
              {activeKeys.length === 0 ? (
                <div className="text-white/20 font-mono text-[11px] tracking-[0.2em] animate-pulse select-none">
                  {errorMessage ? "INVALID COMBINATION" : "PRESS KEYS"}
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
                  Shortcut Saved
                </span>
              ) : errorMessage ? (
                <span className="text-danger/90 font-mono text-[10px] leading-relaxed max-w-[280px] animate-in fade-in duration-200">
                  {errorMessage}
                </span>
              ) : (
                <span className="text-white/30 font-mono text-[10px] uppercase tracking-[0.2em] select-none">
                  Press keys to assign • Esc to cancel
                </span>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
