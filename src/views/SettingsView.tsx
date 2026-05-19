import { useEffect, useState, useRef } from "react";
import { ChevronDown, Trash2 } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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
  return key;
}

export function SettingsView() {
  const [vadEnabled, setVadEnabled] = useState(false);
  const [devices, setDevices] = useState<string[]>([]);
  const [selectedDevice, setSelectedDevice] = useState<string>("");
  const [clearing, setClearing] = useState(false);
  const [storageMessage, setStorageMessage] = useState<string | null>(null);
  const [showConfirmModal, setShowConfirmModal] = useState(false);
  const [isRecordingShortcut, setIsRecordingShortcut] = useState(false);
  const [shortcutText, setShortcutText] = useState("CommandOrControl+Shift+Space");
  const [activeKeys, setActiveKeys] = useState<string[]>([]);
  const [isCompleted, setIsCompleted] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  const isCompletedRef = useRef(false);

  useEffect(() => {
    isCompletedRef.current = isCompleted;
  }, [isCompleted]);

  useEffect(() => {
    const saved = localStorage.getItem("global_record_shortcut") || "CommandOrControl+Shift+Space";
    setShortcutText(saved);
    invoke("register_shortcut", { shortcutStr: saved }).catch((err) => {
      console.error("Failed to register shortcut on mount:", err);
    });

    const savedVad = localStorage.getItem("vad_enabled") === "true";
    setVadEnabled(savedVad);
    invoke("set_vad_enabled", { enabled: savedVad }).catch((err) => {
      console.error("Failed to set VAD state on mount:", err);
    });
  }, []);

  useEffect(() => {
    if (!isRecordingShortcut) return;

    setActiveKeys([]);
    setIsCompleted(false);
    setErrorMessage(null);

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

      const keys: string[] = [];
      if (e.metaKey || e.ctrlKey) {
        keys.push("CommandOrControl");
      } else {
        if (e.ctrlKey) keys.push("Control");
        if (e.metaKey) keys.push("Command");
      }
      if (e.altKey) keys.push("Alt");
      if (e.shiftKey) keys.push("Shift");

      const isModifier = ["Control", "Shift", "Alt", "Meta"].includes(e.key);
      if (!isModifier) {
        let mainKey = e.key;
        if (mainKey === " ") {
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
        
        invoke("register_shortcut", { shortcutStr: shortcutStr })
          .then(() => {
            localStorage.setItem("global_record_shortcut", shortcutStr);
            setShortcutText(shortcutStr);
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
                : "Global shortcuts require a modifier (Cmd, Shift, Alt, etc.) or a Function key (F1-F12)."
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

      const keys: string[] = [];
      if (e.metaKey || e.ctrlKey) {
        keys.push("CommandOrControl");
      } else {
        if (e.ctrlKey) keys.push("Control");
        if (e.metaKey) keys.push("Command");
      }
      if (e.altKey) keys.push("Alt");
      if (e.shiftKey) keys.push("Shift");

      setActiveKeys(keys);
    };

    window.addEventListener("keydown", handleKeyDown, true);
    window.addEventListener("keyup", handleKeyUp, true);
    return () => {
      window.removeEventListener("keydown", handleKeyDown, true);
      window.removeEventListener("keyup", handleKeyUp, true);
    };
  }, [isRecordingShortcut]);

  const handleClearCache = async () => {
    setClearing(true);
    setStorageMessage(null);
    try {
      const msg = await invoke<string>("clear_app_files");
      setStorageMessage(msg);
      setTimeout(() => setStorageMessage(null), 5000);
    } catch (err) {
      console.error(err);
      setStorageMessage(`Error: ${err}`);
      setTimeout(() => setStorageMessage(null), 5000);
    } finally {
      setClearing(false);
    }
  };

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

  return (
    <div className="flex flex-col">
      <div className="mb-6">
        <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
          Preferences
        </h1>
      </div>

      <div className="w-full">
        <h2 className="mt-0 mb-4 text-base text-white font-medium">
          Audio Processing
        </h2>
        <div className="border border-border rounded-xl overflow-hidden bg-secondary mb-10">
          <div className="flex flex-col p-6 border-b border-border">
            <label className="text-fg font-medium mb-3 block">
              Input Device
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
          <div className="flex justify-between items-center p-6">
            <div>
              <div className="text-fg font-medium mb-1">
                Voice Activity Detection (VAD)
              </div>
              <div className="text-muted text-[13px]">
                Automatically stop recording when you stop speaking.
              </div>
            </div>
            <label className="toggle">
              <input
                type="checkbox"
                checked={vadEnabled}
                onChange={(e) => handleVadToggle(e.target.checked)}
              />
              <span className="toggle-bg"></span>
            </label>
          </div>
        </div>

        <h2 className="mb-4 text-base text-white font-medium">Shortcuts</h2>
        <div className="border border-border rounded-xl overflow-hidden bg-secondary mb-10">
          <div className="flex justify-between items-center p-6">
            <div>
              <div className="text-fg font-medium mb-1">
                Global Record Toggle
              </div>
              <div className="text-muted text-[13px]">
                Start/stop recording from anywhere. Click to change.
              </div>
            </div>
            <button
              onClick={() => setIsRecordingShortcut(true)}
              className="inline-flex items-center px-3 py-1.5 rounded text-xs font-mono font-medium border cursor-pointer transition-all duration-200 bg-surface-active text-muted hover:text-white hover:border-muted border-border"
            >
              {formatShortcutDisplay(shortcutText)}
            </button>
          </div>
        </div>

        <h2 className="mb-4 text-base text-white font-medium">Storage</h2>
        <div className="border border-border rounded-xl overflow-hidden bg-secondary mb-10">
          <div className="flex flex-col p-6">
            <div className="flex justify-between items-center w-full">
              <div>
                <div className="text-fg font-medium mb-1">Clear Cache & Recordings</div>
                <div className="text-muted text-[13px]">
                  Remove all temporary recordings and cached audio files from disk.
                </div>
              </div>
              <button
                onClick={() => setShowConfirmModal(true)}
                disabled={clearing}
                className="inline-flex items-center justify-center gap-2 border border-red-500/20 bg-red-500/10 hover:bg-red-500/20 text-red-400 px-3 sm:px-4 py-2 text-xs font-medium rounded-md transition-all duration-200 cursor-pointer disabled:opacity-50 hover:translate-y-[-1px] active:translate-y-0 shrink-0 whitespace-nowrap"
              >
                <Trash2 size={13} className="shrink-0" />
                <span className="hidden sm:inline">
                  {clearing ? "Clearing..." : "Clear Files"}
                </span>
              </button>
            </div>
            {storageMessage && (
              <div className="text-xs text-emerald-400 font-medium mt-3">
                {storageMessage}
              </div>
            )}
          </div>
        </div>
      </div>

      {showConfirmModal && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm transition-all duration-300">
          <div className="bg-secondary border border-border rounded-xl p-6 max-w-sm w-full mx-4 shadow-2xl animate-in fade-in zoom-in-95 duration-200">
            <h3 className="text-lg font-medium text-white mb-2">
              Clear Cache & Recordings?
            </h3>
            <p className="text-muted text-[13px] mb-6 leading-relaxed">
              This will permanently delete all temporary `.wav` files and empty the active in-memory recording buffer. This action cannot be undone.
            </p>
            <div className="flex justify-end gap-3">
              <button
                onClick={() => setShowConfirmModal(false)}
                className="btn btn-outline px-4 py-2 text-xs rounded-md"
              >
                Cancel
              </button>
              <button
                onClick={() => {
                  setShowConfirmModal(false);
                  handleClearCache();
                }}
                className="btn bg-red-600 hover:bg-red-500 text-white border-0 px-4 py-2 text-xs font-semibold rounded-md"
              >
                Confirm Delete
              </button>
            </div>
          </div>
        </div>
      )}

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
            
            {/* Keys Display Row */}
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
                    <kbd className={`inline-flex items-center justify-center px-3.5 py-2 rounded-lg text-xs font-mono font-bold shadow-xl transition-all duration-200 animate-in fade-in slide-in-from-bottom-2 ${
                      isCompleted 
                        ? "bg-white text-black border-white scale-105 shadow-[0_0_15px_rgba(255,255,255,0.25)]" 
                        : "bg-white/10 text-white/90 border border-white/10"
                    }`}>
                      {formatKeycapLabel(key)}
                    </kbd>
                  </span>
                ))
              )}
            </div>

            {/* Instruction/Status Text */}
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
