import { useEffect, useState, useRef } from "react";
import { ChevronDown, Sparkles, Cpu, Keyboard } from "lucide-react";
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
  const [soundEnabled, setSoundEnabled] = useState(true);
  const [devices, setDevices] = useState<string[]>([]);
  const [selectedDevice, setSelectedDevice] = useState<string>("");
  const [isRecordingShortcut, setIsRecordingShortcut] = useState(false);
  const [shortcutText, setShortcutText] = useState("CommandOrControl+Shift+Space");
  const [activeKeys, setActiveKeys] = useState<string[]>([]);
  const [isCompleted, setIsCompleted] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const overlayRef = useRef<HTMLDivElement>(null);
  const isCompletedRef = useRef(false);

  // Multi-engine & BYOK API settings states
  const [openaiKey, setOpenaiKey] = useState("");
  const [anthropicKey, setAnthropicKey] = useState("");
  const [geminiKey, setGeminiKey] = useState("");
  const [refinerEnabled, setRefinerEnabled] = useState(false);
  const [refinerProvider, setRefinerProvider] = useState<"openai" | "anthropic" | "gemini">("openai");
  const [refinerModel, setRefinerModel] = useState("gpt-4o-mini");
  const [refinerPrompt, setRefinerPrompt] = useState(
    "You are an ASR post-processor. Correct grammar, add punctuation, format code snippets in markdown if appropriate. Do NOT add any conversational filler. Only return the corrected text."
  );

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

    const savedSound = localStorage.getItem("sound_feedback_enabled") !== "false";
    setSoundEnabled(savedSound);

    const syncSettings = () => {
      setRefinerEnabled(localStorage.getItem("refiner_enabled") === "true");
      setRefinerProvider((localStorage.getItem("refiner_provider") as any) || "openai");
      setRefinerModel(localStorage.getItem("refiner_model") || "gpt-4o-mini");
      setRefinerPrompt(
        localStorage.getItem("refiner_prompt") ||
          "You are an ASR post-processor. Correct grammar, add punctuation, format code snippets in markdown if appropriate. Do NOT add any conversational filler. Only return the corrected text."
      );
    };
    syncSettings();

    const loadSecureKeys = async () => {
      try {
        const hasOpenai = await invoke<boolean>("has_secure_api_key", { provider: "openai" });
        if (hasOpenai) setOpenaiKey("••••••••••••••••");
        else setOpenaiKey("");
        
        const hasAnthropic = await invoke<boolean>("has_secure_api_key", { provider: "anthropic" });
        if (hasAnthropic) setAnthropicKey("••••••••••••••••");
        else setAnthropicKey("");
        
        const hasGemini = await invoke<boolean>("has_secure_api_key", { provider: "gemini" });
        if (hasGemini) setGeminiKey("••••••••••••••••");
        else setGeminiKey("");
      } catch (err) {
        console.error("Failed to query keyring:", err);
      }
    };
    loadSecureKeys();

    window.addEventListener("api-keys-changed", loadSecureKeys);
    return () => {
      window.removeEventListener("api-keys-changed", loadSecureKeys);
    };
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

  const saveSecureKey = async (provider: string, val: string) => {
    try {
      if (val === "••••••••••••••••") return;
      await invoke("set_secure_api_key", { provider, key: val });
      window.dispatchEvent(new Event("api-keys-changed"));
    } catch (err) {
      console.error(`Failed to save secure key for ${provider}:`, err);
    }
  };



  const updateRefinerToggle = (val: boolean) => {
    setRefinerEnabled(val);
    localStorage.setItem("refiner_enabled", String(val));
  };

  const updateRefinerProvider = (val: "openai" | "anthropic" | "gemini") => {
    setRefinerProvider(val);
    localStorage.setItem("refiner_provider", val);
    
    // Set sensible default models
    let defaultModel = "gpt-4o-mini";
    if (val === "anthropic") {
      defaultModel = "claude-3-5-haiku-20241022";
    } else if (val === "gemini") {
      defaultModel = "gemini-1.5-flash";
    }
    setRefinerModel(defaultModel);
    localStorage.setItem("refiner_model", defaultModel);
  };

  const updateRefinerModel = (val: string) => {
    setRefinerModel(val);
    localStorage.setItem("refiner_model", val);
  };

  const updateRefinerPrompt = (val: string) => {
    setRefinerPrompt(val);
    localStorage.setItem("refiner_prompt", val);
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

          <div className="flex justify-between items-center p-6 border-b border-border">
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

          <div className="flex justify-between items-center p-6">
            <div>
              <div className="text-fg font-medium mb-1">
                Sound Effects
              </div>
              <div className="text-muted text-[13px]">
                Play a premium audio cue when starting and stopping recording.
              </div>
            </div>
            <label className="toggle">
              <input
                type="checkbox"
                checked={soundEnabled}
                onChange={(e) => handleSoundToggle(e.target.checked)}
              />
              <span className="toggle-bg"></span>
            </label>
          </div>
        </div>

        {/* SECTION: LLM Post-Processing */}
        <h2 className="mb-4 text-base text-white font-medium flex items-center gap-2">
          <Sparkles size={16} className="text-muted" /> Smart LLM Refiner (Post-Processing)
        </h2>
        <div className="border border-border rounded-xl overflow-hidden bg-secondary mb-10">
          <div className="flex justify-between items-center p-6 border-b border-border">
            <div>
              <div className="text-fg font-medium mb-1">
                Enable Text Refiner
              </div>
              <div className="text-muted text-[13px]">
                Post-process raw audio transcripts using a cloud LLM to format, fix spelling, or edit code.
              </div>
            </div>
            <label className="toggle">
              <input
                type="checkbox"
                checked={refinerEnabled}
                onChange={(e) => updateRefinerToggle(e.target.checked)}
              />
              <span className="toggle-bg"></span>
            </label>
          </div>

          {refinerEnabled && (
            <>
              <div className="flex flex-col sm:flex-row p-6 gap-6 border-b border-border bg-black/10">
                <div className="flex-1">
                  <label className="text-fg font-medium mb-2.5 block text-xs">
                    LLM Provider
                  </label>
                  <div className="relative w-full">
                    <select
                      value={refinerProvider}
                      onChange={(e) => updateRefinerProvider(e.target.value as any)}
                      className="input w-full bg-black border-border rounded-md pl-4 pr-10 py-2.5 appearance-none cursor-pointer hover:border-muted transition-colors text-xs font-medium"
                    >
                      <option value="openai">OpenAI</option>
                      <option value="anthropic">Anthropic Claude</option>
                      <option value="gemini">Google Gemini</option>
                    </select>
                    <div className="absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none text-muted">
                      <ChevronDown size={14} />
                    </div>
                  </div>
                </div>

                <div className="flex-1">
                  <label className="text-fg font-medium mb-2.5 block text-xs">
                    Model
                  </label>
                  <div className="relative w-full">
                    <select
                      value={refinerModel}
                      onChange={(e) => updateRefinerModel(e.target.value)}
                      className="input w-full bg-black border-border rounded-md pl-4 pr-10 py-2.5 appearance-none cursor-pointer hover:border-muted transition-colors text-xs font-medium"
                    >
                      {refinerProvider === "openai" && (
                        <>
                          <option value="gpt-4o-mini">gpt-4o-mini (Recommended)</option>
                          <option value="gpt-4o">gpt-4o</option>
                        </>
                      )}
                      {refinerProvider === "anthropic" && (
                        <>
                          <option value="claude-3-5-haiku-20241022">claude-3-5-haiku (Recommended)</option>
                          <option value="claude-3-5-sonnet-20241022">claude-3-5-sonnet</option>
                        </>
                      )}
                      {refinerProvider === "gemini" && (
                        <>
                          <option value="gemini-1.5-flash">gemini-1.5-flash (Recommended)</option>
                          <option value="gemini-1.5-pro">gemini-1.5-pro</option>
                        </>
                      )}
                    </select>
                    <div className="absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none text-muted">
                      <ChevronDown size={14} />
                    </div>
                  </div>
                </div>
              </div>

              {/* Contextual API Key Input for selected Provider */}
              <div className="flex flex-col p-6 border-b border-border bg-black/5">
                <label className="text-fg font-medium mb-2 block text-xs">
                  {refinerProvider === "openai" ? "OpenAI API Key" : refinerProvider === "anthropic" ? "Anthropic API Key" : "Google Gemini API Key"}
                </label>
                {refinerProvider === "openai" && (
                  <input
                    type="password"
                    value={openaiKey}
                    onChange={(e) => {
                      setOpenaiKey(e.target.value);
                      saveSecureKey("openai", e.target.value);
                    }}
                    placeholder={openaiKey === "••••••••••••••••" ? "" : "sk-..."}
                    className="input w-full bg-black border-border rounded-md px-4 py-2.5 text-xs focus:border-muted transition-colors"
                  />
                )}
                {refinerProvider === "anthropic" && (
                  <input
                    type="password"
                    value={anthropicKey}
                    onChange={(e) => {
                      setAnthropicKey(e.target.value);
                      saveSecureKey("anthropic", e.target.value);
                    }}
                    placeholder={anthropicKey === "••••••••••••••••" ? "" : "sk-ant-..."}
                    className="input w-full bg-black border-border rounded-md px-4 py-2.5 text-xs focus:border-muted transition-colors"
                  />
                )}
                {refinerProvider === "gemini" && (
                  <input
                    type="password"
                    value={geminiKey}
                    onChange={(e) => {
                      setGeminiKey(e.target.value);
                      saveSecureKey("gemini", e.target.value);
                    }}
                    placeholder={geminiKey === "••••••••••••••••" ? "" : "AIzaSy..."}
                    className="input w-full bg-black border-border rounded-md px-4 py-2.5 text-xs focus:border-muted transition-colors"
                  />
                )}
                <p className="text-[10px] text-muted mt-2">
                  This key is stored securely in your operating system's native keychain.
                </p>
              </div>

              <div className="flex flex-col p-6 bg-black/15">
                <label className="text-fg font-medium mb-2.5 block text-xs">
                  Refining System Prompt Instructions
                </label>
                <textarea
                  value={refinerPrompt}
                  onChange={(e) => updateRefinerPrompt(e.target.value)}
                  className="input w-full bg-black border-border rounded-md p-4 h-24 resize-none text-xs font-normal focus:border-muted transition-colors leading-relaxed"
                  placeholder="Define instructions for transcription formatting..."
                />
              </div>
            </>
          )}
        </div>

        {/* SECTION: Shortcuts */}
        <h2 className="mb-4 text-base text-white font-medium flex items-center gap-2">
          <Keyboard size={16} className="text-muted" /> Shortcuts
        </h2>
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
