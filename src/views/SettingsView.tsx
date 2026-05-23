import { useEffect, useState, useRef } from "react";
import { ChevronDown, Cpu, Shield, ExternalLink, Keyboard, Check } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { enable, isEnabled, disable } from "@tauri-apps/plugin-autostart";

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

export function SettingsView() {
  const [vadEnabled, setVadEnabled] = useState(false);
  const [soundEnabled, setSoundEnabled] = useState(true);
  const [pauseAudioEnabled, setPauseAudioEnabled] = useState(false);
  const [asrLanguage, setAsrLanguage] = useState("auto");
  const [autostartEnabled, setAutostartEnabled] = useState(false);

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
  const [platform, setPlatform] = useState("unknown");
  const [desktopEnv, setDesktopEnv] = useState("none");
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

    const savedPauseAudio =
      localStorage.getItem("pause_audio_on_record") === "true";
    setPauseAudioEnabled(savedPauseAudio);

    const savedLang = localStorage.getItem("asr_language") || "auto";
    setAsrLanguage(savedLang);

    isEnabled().then(setAutostartEnabled);
  }, []);

  // Check system permissions and Wayland status on mount and periodically
  useEffect(() => {
    const checkPermissions = async () => {
      try {
        const status = await invoke<{
          accessibility: boolean;
          platform: string;
          is_wayland: boolean;
          desktop_env: string;
        }>("check_permissions_status");
        setAccessibilityGranted(status.accessibility);
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
          mainKey === "\u00a0" ||
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
  };

  const handlePauseAudioToggle = (checked: boolean) => {
    setPauseAudioEnabled(checked);
    localStorage.setItem("pause_audio_on_record", String(checked));
  };

  const handleAsrLanguageChange = (val: string) => {
    setAsrLanguage(val);
    localStorage.setItem("asr_language", val);
  };

  const handleAutostartToggle = async (checked: boolean) => {
    setAutostartEnabled(checked);
    if (checked) {
      await enable();
    } else {
      await disable();
    }
  };

  return (
    <div className="flex flex-col">
      <div className="mb-6">
        <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
          Preferences
        </h1>
      </div>

      <div className="w-full">
        {/* SECTION: Audio & STT */}
        <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
          <Cpu size={16} className="text-muted" /> Audio & Speech-to-Text
        </h2>
        <div className="border border-border rounded-xl overflow-hidden bg-secondary mb-10">
          <div className="flex flex-col p-6 border-b border-border">
            <label className="text-fg font-medium mb-3 block">
              Input Microphone
            </label>
            <div className="relative w-full">
              <select
                value={selectedDevice}
                onChange={(e) => handleDeviceChange(e.target.value)}
                className="input w-full bg-black border-border rounded-md pl-4 pr-10 py-3 appearance-none cursor-pointer hover:border-muted transition-colors text-sm font-medium"
              >
                <option value="default">Default System Microphone</option>
                {devices.map((device, idx) => (
                  <option key={idx} value={device}>
                    {device}
                  </option>
                ))}
              </select>
              <div className="absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none text-muted">
                <ChevronDown size={18} />
              </div>
            </div>
          </div>

          <div className="flex flex-col p-6 border-b border-border">
            <label className="text-fg font-medium mb-3 block">
              Transcription Language
            </label>
            <div className="relative w-full">
              <select
                value={asrLanguage}
                onChange={(e) => handleAsrLanguageChange(e.target.value)}
                className="input w-full bg-black border-border rounded-md pl-4 pr-10 py-3 appearance-none cursor-pointer hover:border-muted transition-colors text-sm font-medium"
              >
                <option value="auto">Auto-detect</option>
                <optgroup label="Popular">
                  <option value="en">English</option>
                  <option value="fr">French (Français)</option>
                  <option value="de">German (Deutsch)</option>
                  <option value="it">Italian (Italiano)</option>
                  <option value="pl">Polish (Polski)</option>
                  <option value="es">Spanish (Español)</option>
                </optgroup>
                <optgroup label="Europe">
                  <option value="bg">Bulgarian (Български)</option>
                  <option value="hr">Croatian (Hrvatski)</option>
                  <option value="cs">Czech (Čeština)</option>
                  <option value="da">Danish (Dansk)</option>
                  <option value="nl">Dutch (Nederlands)</option>
                  <option value="fi">Finnish (Suomi)</option>
                  <option value="el">Greek (Ελληνικά)</option>
                  <option value="hu">Hungarian (Magyar)</option>
                  <option value="no">Norwegian (Norsk)</option>
                  <option value="pt">Portuguese (Português)</option>
                  <option value="ro">Romanian (Română)</option>
                  <option value="ru">Russian (Русский)</option>
                  <option value="sr">Serbian (Српски)</option>
                  <option value="sk">Slovak (Slovenčina)</option>
                  <option value="sv">Swedish (Svenska)</option>
                  <option value="tr">Turkish (Türkçe)</option>
                  <option value="uk">Ukrainian (Українська)</option>
                </optgroup>
                <optgroup label="Asia & Middle East">
                  <option value="ar">Arabic (العربية)</option>
                  <option value="zh">Chinese (中文)</option>
                  <option value="he">Hebrew (עברית)</option>
                  <option value="hi">Hindi (हिन्दी)</option>
                  <option value="id">Indonesian (Bahasa Indonesia)</option>
                  <option value="ja">Japanese (日本語)</option>
                  <option value="ko">Korean (한국어)</option>
                  <option value="ms">Malay (Bahasa Melayu)</option>
                  <option value="fa">Persian (فارسی)</option>
                  <option value="th">Thai (ไทย)</option>
                  <option value="vi">Vietnamese (Tiếng Việt)</option>
                </optgroup>
                <optgroup label="Other">
                  <option value="af">Afrikaans</option>
                  <option value="sw">Swahili (Kiswahili)</option>
                  <option value="tl">Tagalog</option>
                </optgroup>
              </select>
              <div className="absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none text-muted">
                <ChevronDown size={18} />
              </div>
            </div>
            <p className="text-[11px] text-muted mt-2">
              Forces the model to output text in the selected language. Use
              "Auto-detect" for multilingual support.
            </p>
          </div>

          <div className="flex justify-between items-center p-6 border-b border-border">
            <div>
              <div className="text-fg font-medium mb-1">Auto-start</div>
              <div className="text-xs text-muted">
                Start SimpleVoice automatically when you log in.
              </div>
            </div>
            <label className="toggle cursor-pointer">
              <input
                type="checkbox"
                checked={autostartEnabled}
                onChange={(e) => handleAutostartToggle(e.target.checked)}
              />
              <span className="toggle-bg"></span>
            </label>
          </div>

          <div className="flex justify-between items-center p-6 border-b border-border">
            <div>
              <div className="text-fg font-medium mb-1">
                Voice Activity Detection (VAD)
              </div>
              <div className="text-muted text-[13px]">
                Automatically stop recording when you stop speaking.
              </div>
            </div>

            <label className="toggle cursor-pointer">
              <input
                type="checkbox"
                checked={vadEnabled}
                onChange={(e) => handleVadToggle(e.target.checked)}
              />
              <span className="toggle-bg"></span>
            </label>
          </div>

          <div className="flex justify-between items-center p-6 border-b border-border">
            <div>
              <div className="text-fg font-medium mb-1">Sound Effects</div>
              <div className="text-muted text-[13px]">
                Play a premium audio cue when starting and stopping recording.
              </div>
            </div>

            <label className="toggle cursor-pointer">
              <input
                type="checkbox"
                checked={soundEnabled}
                onChange={(e) => handleSoundToggle(e.target.checked)}
              />
              <span className="toggle-bg"></span>
            </label>
          </div>

          <div className="flex justify-between items-center p-6">
            <div>
              <div className="text-fg font-medium mb-1">Pause System Audio</div>
              <div className="text-muted text-[13px]">
                Automatically pause music or videos while recording and resume
                afterwards.
              </div>
            </div>

            <label className="toggle cursor-pointer">
              <input
                type="checkbox"
                checked={pauseAudioEnabled}
                onChange={(e) => handlePauseAudioToggle(e.target.checked)}
              />
              <span className="toggle-bg"></span>
            </label>
          </div>
          </div>

        {/* SECTION: Keyboard Shortcuts */}
        <div className="border border-border rounded-xl overflow-hidden bg-secondary mb-10">
          <h2 className="p-6 pb-4 text-base text-white font-medium flex items-center gap-2 border-b border-border">
            <Keyboard size={16} className="text-muted" /> Keyboard Shortcuts
          </h2>

          <div className="flex justify-between items-center p-6 border-b border-border">
            <div className="flex-1 pr-8">
              <div className="text-fg font-medium mb-1">Start / Stop Recording</div>
              <div className="text-muted text-[13px]">
                Global hotkey to toggle voice recording from anywhere
              </div>
            </div>
            <div
              onClick={() => startRecordingShortcut("record")}
              className="font-mono text-sm px-3.5 py-1.5 bg-surface-active rounded border border-border text-foreground min-w-[140px] text-center cursor-pointer hover:border-blue-400 hover:bg-blue-500/10 active:scale-[0.985] transition-all select-none"
              title="Click to change shortcut"
            >
              {formatShortcutDisplay(shortcutText)}
            </div>
          </div>

          <div className="flex justify-between items-center p-6">
            <div className="flex-1 pr-8">
              <div className="text-fg font-medium mb-1">Copy Last Transcription</div>
              <div className="text-muted text-[13px]">
                Copy the most recent transcription result to clipboard
              </div>
            </div>
            <div
              onClick={() => startRecordingShortcut("copy")}
              className="font-mono text-sm px-3.5 py-1.5 bg-surface-active rounded border border-border text-foreground min-w-[140px] text-center cursor-pointer hover:border-blue-400 hover:bg-blue-500/10 active:scale-[0.985] transition-all select-none"
              title="Click to change shortcut"
            >
              {formatShortcutDisplay(copyShortcutText)}
            </div>
          </div>

          {/* Linux Native / Wayland warning block */}
          {platform === "linux" && (
            <div className="p-6 pt-0 border-t border-border">
              {["niri", "hyprland", "sway", "i3"].includes(desktopEnv) ? (
                <div className="mt-4 p-4 bg-emerald-500/10 border border-emerald-500/20 rounded-lg text-emerald-400 text-xs leading-relaxed flex flex-col gap-1.5">
                  <div className="font-semibold flex items-center gap-1.5 text-sm">
                    <Check size={14} /> Automatic Window Manager Shortcuts Active
                  </div>
                  <p>
                    Your shortcuts were automatically written to your <strong>{desktopEnv.toUpperCase()}</strong> configuration file.
                    They work globally (across all native Wayland & X11 windows) and apply immediately!
                  </p>
                  <button
                    onClick={() => setShowManualWMInstructions(!showManualWMInstructions)}
                    className="text-left text-[11px] text-amber-400 hover:text-amber-300 underline font-medium mt-1 cursor-pointer select-none transition-colors bg-transparent border-0 p-0"
                  >
                    {showManualWMInstructions ? "Hide details / config path" : "Show config path & configuration details"}
                  </button>
                  
                  {showManualWMInstructions && (
                    <div className="mt-3 font-medium border-t border-emerald-500/10 pt-3 flex flex-col gap-3">
                      <p className="text-muted text-[11px]">
                        The app automatically appended custom bind entries to your configuration file. Here is the format:
                      </p>
                      
                      {desktopEnv === "niri" && (
                        <div className="flex flex-col gap-1">
                          <span className="font-semibold text-amber-300">Niri (~/.config/niri/config.kdl):</span>
                          <pre className="bg-black/50 p-2.5 rounded font-mono text-[11px] text-amber-200 overflow-x-auto">
{`binds {
    "Mod+Space" { spawn "simplevoice" "--toggle"; }
    "Mod+Shift+C" { spawn "simplevoice" "--copy-last"; }
}`}
                          </pre>
                        </div>
                      )}

                      {desktopEnv === "hyprland" && (
                        <div className="flex flex-col gap-1">
                          <span className="font-semibold text-amber-300">Hyprland (~/.config/hypr/hyprland.conf):</span>
                          <pre className="bg-black/50 p-2.5 rounded font-mono text-[11px] text-amber-200 overflow-x-auto">
{`bind = SUPER, Space, exec, simplevoice --toggle
bind = SUPER_SHIFT, C, exec, simplevoice --copy-last`}
                          </pre>
                        </div>
                      )}

                      {(desktopEnv === "sway" || desktopEnv === "i3") && (
                        <div className="flex flex-col gap-1">
                          <span className="font-semibold text-amber-300">Sway / i3 (~/.config/sway/config or ~/.config/i3/config):</span>
                          <pre className="bg-black/50 p-2.5 rounded font-mono text-[11px] text-amber-200 overflow-x-auto">
{`bindsym Mod4+Space exec simplevoice --toggle
bindsym Mod4+Shift+c exec simplevoice --copy-last`}
                          </pre>
                        </div>
                      )}
                    </div>
                  )}
                </div>
              ) : ["gnome", "kde", "xfce", "cinnamon", "mate"].includes(desktopEnv) ? (
                <div className="mt-4 p-4 bg-emerald-500/10 border border-emerald-500/20 rounded-lg text-emerald-400 text-xs leading-relaxed flex flex-col gap-1.5">
                  <div className="font-semibold flex items-center gap-1.5 text-sm">
                    <Check size={14} /> Native Linux Shortcut Integration Active
                  </div>
                  <p>
                    Your shortcuts are registered directly in the <strong>{
                      desktopEnv === "gnome" ? "GNOME" :
                      desktopEnv === "kde" ? "KDE Plasma" :
                      desktopEnv === "xfce" ? "XFCE" :
                      desktopEnv === "cinnamon" ? "Cinnamon" : "MATE"
                    }</strong> keyboard configuration using built-in settings. They apply immediately!
                  </p>
                </div>
              ) : (
                <div className="mt-4 p-4 bg-amber-500/10 border border-amber-500/20 rounded-lg text-amber-400 text-xs leading-relaxed flex flex-col gap-2">
                  <div className="font-semibold flex items-center gap-1.5 text-sm">
                    <Shield size={14} /> Linux {desktopEnv === "unknown" ? "Window Manager" : desktopEnv.toUpperCase()} / Shortcut Notice
                  </div>
                  <p>
                    Global hotkeys cannot be registered automatically under your desktop environment's security model.
                    For the best experience, add these custom binds to your config file:
                  </p>
                  
                  <div className="mt-1 font-medium border-t border-amber-500/10 pt-2 flex flex-col gap-3">
                    <div className="flex flex-col gap-1">
                      <span className="font-semibold text-amber-300">Niri (~/.config/niri/config.kdl):</span>
                      <pre className="bg-black/50 p-2.5 rounded font-mono text-[11px] text-amber-200 overflow-x-auto">
{`binds {
    "Mod+Space" { spawn "simplevoice" "--toggle"; }
    "Mod+Shift+C" { spawn "simplevoice" "--copy-last"; }
}`}
                      </pre>
                    </div>

                    <div className="flex flex-col gap-1">
                      <span className="font-semibold text-amber-300">Hyprland (~/.config/hypr/hyprland.conf):</span>
                      <pre className="bg-black/50 p-2.5 rounded font-mono text-[11px] text-amber-200 overflow-x-auto">
{`bind = SUPER, Space, exec, simplevoice --toggle
bind = SUPER_SHIFT, C, exec, simplevoice --copy-last`}
                      </pre>
                    </div>

                    <div className="flex flex-col gap-1">
                      <span className="font-semibold text-amber-300">Sway / i3 (~/.config/sway/config or ~/.config/i3/config):</span>
                      <pre className="bg-black/50 p-2.5 rounded font-mono text-[11px] text-amber-200 overflow-x-auto">
{`bindsym Mod4+Space exec simplevoice --toggle
bindsym Mod4+Shift+c exec simplevoice --copy-last`}
                      </pre>
                    </div>
                  </div>
                </div>
              )}
            </div>
          )}

          {/* Fallback generic error warning if registration fails on other platforms */}
          {platform !== "linux" && (shortcutError || copyShortcutError) && (
            <div className="p-6 pt-0 border-t border-border">
              <div className="mt-4 p-4 bg-red-500/10 border border-red-500/20 rounded-lg text-red-400 text-xs leading-relaxed flex flex-col gap-2">
                <div className="font-semibold flex items-center gap-1.5 text-sm">
                  <Shield size={14} /> Shortcut Registration Error
                </div>
                <p>{shortcutError || copyShortcutError}</p>
              </div>
            </div>
          )}
        </div>

        {/* SECTION: System Permissions */}
        {platform === "macos" && (

          <>
            <h2 className="mb-4 text-base text-white font-medium flex items-center gap-2">
              <Shield size={16} className="text-muted" /> System Permissions
            </h2>
            <div className="border border-border rounded-xl overflow-hidden bg-secondary mb-10">
              <div className="flex justify-between items-center p-6 border-b border-border">
                <div className="flex-1">
                  <div className="text-fg font-medium mb-1 flex items-center gap-2">
                    Accessibility
                    <span
                      className={`inline-block w-2 h-2 rounded-full ${
                        accessibilityGranted
                          ? "bg-emerald-400 shadow-[0_0_6px_rgba(52,211,153,0.5)]"
                          : "bg-amber-400 shadow-[0_0_6px_rgba(251,191,36,0.5)] animate-pulse"
                      }`}
                    />
                  </div>
                  <div className="text-muted text-[13px]">
                    Required for auto-paste (keyboard simulation via Cmd+V).
                    {!accessibilityGranted && (
                      <span className="text-amber-400 font-medium">
                        {" "}
                        Not granted — auto-paste will not work.
                      </span>
                    )}
                  </div>
                </div>
                {!accessibilityGranted ? (
                  <button
                    onClick={async () => {
                      try {
                        await invoke("request_accessibility_permission");
                      } catch (err) {
                        console.error("Failed to request accessibility:", err);
                      }
                    }}
                    className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium border cursor-pointer transition-all duration-200 bg-amber-500/10 text-amber-400 hover:bg-amber-500/20 hover:text-amber-300 border-amber-500/30"
                  >
                    Grant Access
                  </button>
                ) : (
                  <span className="inline-flex items-center px-3 py-1.5 rounded text-xs font-medium text-emerald-400">
                    ✓ Granted
                  </span>
                )}
              </div>

              <div className="flex justify-between items-center p-6">
                <div className="flex-1">
                  <div className="text-fg font-medium mb-1">Microphone</div>
                  <div className="text-muted text-[13px]">
                    Required for audio capture. Managed by macOS automatically
                    on first use.
                  </div>
                </div>
                <button
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
                  className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium border cursor-pointer transition-all duration-200 bg-surface-active text-muted hover:text-white hover:border-muted border-border"
                >
                  <ExternalLink size={12} />
                  Open Settings
                </button>
              </div>
            </div>
          </>
        )}
      </div>

      {isRecordingShortcut && (
        <div
          ref={overlayRef}
          onClick={(e) => {
            if (e.target === overlayRef.current && !isCompleted) {
              setIsRecordingShortcut(false);
            }
          }}
          className="fixed inset-0 z-50 flex flex-col items-center justify-center bg-black/90 backdrop-blur-xl transition-all duration-300 animate-in fade-in"
        >
          <div className="flex flex-col items-center justify-center text-center max-w-sm w-full mx-4">
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
                <span className="text-emerald-400 font-mono text-[10px] uppercase tracking-[0.25em] animate-in zoom-in-95 duration-200">
                  Shortcut Saved
                </span>
              ) : errorMessage ? (
                <span className="text-red-400/90 font-mono text-[10px] leading-relaxed max-w-[280px] animate-in fade-in duration-200">
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
