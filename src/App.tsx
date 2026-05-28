import { useEffect, useState } from "react";
import "./App.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { TitleBar } from "./components/layout/TitleBar";
import { Updater } from "./components/Updater";
import { Sidebar } from "./components/layout/Sidebar";
import { UsageView } from "./views/UsageView";
import { ModelsView } from "./views/ModelsView";
import { TranscriptionsView } from "./views/TranscriptionsView";
import { SettingsView } from "./views/SettingsView";
import { ConfigProvider } from "./context/ConfigContext";

type ViewId = "usage" | "models" | "transcriptions" | "settings";

function App() {
  const [activeView, setActiveView] = useState<ViewId>("usage");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => {
    return localStorage.getItem("sidebar_collapsed") === "true";
  });
  const [isRecording, setIsRecording] = useState(false);
  const [isTranscribing, setIsTranscribing] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const toggleSidebar = () => {
    const nextVal = !sidebarCollapsed;
    setSidebarCollapsed(nextVal);
    localStorage.setItem("sidebar_collapsed", String(nextVal));
  };

  const syncActiveConfig = async () => {
    try {
      const engine = localStorage.getItem("asr_engine") || "local";
      const provider = localStorage.getItem("asr_provider") || "openai";
      await invoke("update_active_config", { engine, provider });
    } catch (err) {
      console.error("Failed to sync active engine configuration:", err);
    }
  };

  useEffect(() => {
    const initDevice = async () => {
      try {
        const saved = localStorage.getItem("selected_audio_device");
        if (saved) {
          const list = await invoke<string[]>("list_audio_devices");
          if (list.includes(saved)) {
            await invoke("set_selected_device", { device: saved });
            return;
          }
        }
        await invoke("set_selected_device", { device: null });
      } catch (err) {
        console.error("Failed to initialize audio device:", err);
      }
    };

    const initModel = async () => {
      try {
        const asrEngine = localStorage.getItem("asr_engine") || "local";
        if (asrEngine === "local") {
          const savedModelPath = localStorage.getItem(
            "active_local_model_path",
          );
          if (savedModelPath) {
            await invoke("load_model", { modelPath: savedModelPath });
            window.dispatchEvent(new Event("asr-engine-changed"));
          }
        }
      } catch (err) {
        console.error("Failed to restore active local model on startup:", err);
      }
    };

    initDevice();
    initModel();
    syncActiveConfig();

    // Register the copy-last shortcut if one was saved
    const savedCopyShortcut =
      localStorage.getItem("global_copy_shortcut") ||
      "CommandOrControl+Shift+C";
    if (savedCopyShortcut) {
      invoke("register_copy_shortcut", {
        shortcutStr: savedCopyShortcut,
      }).catch((err) => {
        console.error("Failed to register copy-last shortcut on mount:", err);
      });
    }
  }, []);

  useEffect(() => {
    const unlisten = listen<string>("navigate", (event) => {
      setActiveView(event.payload as ViewId);
    });

    const handleCustomNav = (e: Event) => {
      const view = (e as CustomEvent).detail as ViewId;
      setActiveView(view);
    };
    window.addEventListener("navigate-to-view", handleCustomNav);

    window.addEventListener("asr-engine-changed", syncActiveConfig);
    window.addEventListener("api-keys-changed", syncActiveConfig);

    return () => {
      unlisten.then((f) => f());
      window.removeEventListener("navigate-to-view", handleCustomNav);
      window.removeEventListener("asr-engine-changed", syncActiveConfig);
      window.removeEventListener("api-keys-changed", syncActiveConfig);
    };
  }, []);

  useEffect(() => {
    const handleStarted = () => {
      setIsRecording(true);
      setErrorMessage(null);
    };

    const handleStopped = async (event: any) => {
      const wavPath = event?.payload;
      setIsRecording(false);
      setIsTranscribing(true);
      invoke("set_transcribing", { active: true }).catch(() => {});

      try {
        const hasSamples = await invoke<boolean>("has_last_recording_samples");
        if (hasSamples) {
          // Read settings from localStorage
          const asrEngine = localStorage.getItem("asr_engine") || "local";

          let modelName = "Whisper Local";
          if (asrEngine === "openai-cloud") {
            modelName = "OpenAI Cloud";
          } else {
            const activeModelPath = await invoke<string | null>(
              "get_active_model",
            );
            if (activeModelPath) {
              const parts = activeModelPath.split(/[\/\\]/);
              modelName = parts[parts.length - 1];
            }
          }

          const asrProvider = localStorage.getItem("asr_provider") || "openai";
          const asrModel = localStorage.getItem("asr_model") || "whisper-1";
          const asrCustomModel = localStorage.getItem("asr_custom_model") || "";
          const asrBaseUrl = localStorage.getItem("asr_base_url") || "";
          const asrLanguage = localStorage.getItem("asr_language") || "auto";
          const modelToUse = asrModel === "custom" ? asrCustomModel : asrModel;

          // Step 1: Transcribe Audio
          let text = await invoke<string>("transcribe_audio", {
            samples: null,
            engine: asrEngine,
            provider: asrProvider,
            model: modelToUse || null,
            baseUrl: asrBaseUrl || null,
            language: asrLanguage === "auto" ? null : asrLanguage,
          });

          if (text && text.trim().length > 0) {
            console.log(`[FRONTEND] Transcription successful, text length: ${text.length}`);

            // Clipboard is now set in Rust backend (arboard). Play sound + auto-paste.
            try {
              await invoke("play_done_sound");
            } catch (e) {
              console.warn("Failed to play sound:", e);
            }

            try {
              // Pass text so wtype can type it directly on Wayland (more reliable than Ctrl+V)
              invoke("paste_text", { text }).catch((err) =>
                console.error("Paste failed:", err),
              );
            } catch (e) {
              console.error("Paste invocation failed:", e);
            }

            if (wavPath && wavPath !== "Recording stopped") {
              try {
                await invoke("save_transcription_data", {
                  wavPath,
                  text,
                  model: modelName,
                });
              } catch (saveErr) {
                console.error(
                  "Failed to save transcription directory data:",
                  saveErr,
                );
              }
            }

            // Store the last transcription in the Rust backend for global shortcut access
            try {
              await invoke("set_last_transcription", { text });
            } catch (e) {
              console.error("Failed to set last transcription:", e);
            }

            window.dispatchEvent(new Event("transcription-added"));
            console.log("Transcription successful:", text);
          }
        }
      } catch (err: any) {
        console.error("Failed to transcribe recording:", err);
        const msg = typeof err === "string" ? err : err?.message || String(err);
        setErrorMessage(msg);
      } finally {
        setIsTranscribing(false);
        invoke("set_transcribing", { active: false }).catch(() => {});
      }
    };

    const unlistenStarted = listen("recording-started", handleStarted);
    const unlistenStopped = listen("recording-stopped", handleStopped);
    const unlistenFailed = listen<string>(
      "recording-failed-to-start",
      (event) => {
        setErrorMessage(event.payload);
        setActiveView("models");
      },
    );

    return () => {
      unlistenStarted.then((f) => f());
      unlistenStopped.then((f) => f());
      unlistenFailed.then((f) => f());
    };
  }, []);

  const getTitleName = (id: ViewId) => {
    return id.charAt(0).toUpperCase() + id.slice(1);
  };

  return (
    <ConfigProvider>
      <div className="flex flex-col h-screen w-screen overflow-hidden bg-black relative">
        <TitleBar
          activeViewName={getTitleName(activeView)}
          toggleSidebar={toggleSidebar}
        />
        {errorMessage && (
          <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm transition-all duration-300 animate-[fadeIn_0.2s_ease-out]">
            <div className="flex flex-col items-center justify-center bg-[#1c1c1e]/95 border border-border/90 rounded-2xl p-8 max-w-sm w-full mx-4 shadow-[0_20px_50px_rgba(0,0,0,0.7)] backdrop-blur-md text-center transform scale-100 transition-all duration-300">
              <div className="w-12 h-12 rounded-full bg-red-500/10 flex items-center justify-center mb-4 text-red-500">
                <svg
                  xmlns="http://www.w3.org/2000/svg"
                  fill="none"
                  viewBox="0 0 24 24"
                  strokeWidth={2}
                  stroke="currentColor"
                  className="w-6 h-6"
                >
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    d="M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z"
                  />
                </svg>
              </div>
              <h3 className="text-white text-lg font-semibold mb-2">
                Configuration Required
              </h3>
              <p className="text-muted text-xs leading-relaxed mb-6">
                {errorMessage}
              </p>
              <button
                onClick={() => setErrorMessage(null)}
                className="w-full btn btn-primary py-2.5 rounded-lg text-xs font-semibold cursor-pointer"
              >
                Configure Now
              </button>
            </div>
          </div>
        )}
        <div className="flex flex-1 overflow-hidden">
          <Sidebar
            collapsed={sidebarCollapsed}
            activeView={activeView}
            setActiveView={(v) => setActiveView(v as ViewId)}
          />

          <main className="main-content">
            <div className={`view ${activeView === "usage" ? "active" : ""}`}>
              <UsageView />
            </div>
            <div className={`view ${activeView === "models" ? "active" : ""}`}>
              <ModelsView />
            </div>
            <div
              className={`view ${activeView === "transcriptions" ? "active" : ""}`}
            >
              <TranscriptionsView />
            </div>
            <div className={`view ${activeView === "settings" ? "active" : ""}`}>
              <SettingsView />
            </div>
          </main>
        </div>

        {/* Global Recording / Transcribing HUD Overlay */}
        {(isRecording || isTranscribing) && (
          <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm transition-all duration-300 animate-[fadeIn_0.2s_ease-out]">
            <div className="flex flex-col items-center justify-center bg-[#1c1c1e]/90 border border-border/85 rounded-2xl p-8 max-w-sm w-full mx-4 shadow-[0_12px_40px_rgba(0,0,0,0.5)] backdrop-blur-md text-center transform scale-100 transition-all duration-300">
              {isRecording ? (
                <>
                  <div className="relative mb-6">
                    {/* Pulsing outer ring */}
                    <div className="absolute inset-[-12px] rounded-full bg-red-500/10 animate-ping"></div>
                    <div className="absolute inset-[-6px] rounded-full bg-red-500/20 animate-pulse"></div>
                    {/* Recording circle */}
                    <div className="w-16 h-16 rounded-full bg-red-500 flex items-center justify-center shadow-lg shadow-red-500/30">
                      <div className="w-6 h-6 bg-white rounded-sm animate-pulse"></div>
                    </div>
                  </div>
                  <h2 className="text-white text-lg font-medium mb-1 tracking-tight">
                    Recording Audio
                  </h2>
                  <p className="text-muted text-sm leading-normal">
                    Speak now... Press shortcut or wait for silence to stop.
                  </p>
                </>
              ) : (
                <>
                  <div className="relative mb-6">
                    {/* Rotating loader */}
                    <div className="w-16 h-16 rounded-full border-4 border-border/40 border-t-white animate-spin"></div>
                  </div>
                  <h2 className="text-white text-lg font-medium mb-1 tracking-tight">
                    Transcribing
                  </h2>
                  <p className="text-muted text-sm leading-normal">
                    Processing audio locally using Whisper...
                  </p>
                </>
              )}
            </div>
          </div>
        )}
        <Updater />
      </div>
    </ConfigProvider>
  );
}

export default App;
