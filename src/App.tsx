import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import "./App.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { localizeError as localizeBackendError } from "@/lib/localizeError";

import { AlertTriangle } from "lucide-react";

import { TitleBar } from "./components/layout/TitleBar";
import { Updater } from "./components/Updater";
import { OnboardingOverlay } from "./components/onboarding/OnboardingOverlay";
import { Sidebar } from "./components/layout/Sidebar";
import { UsageView } from "./views/UsageView";
import { ModelsView } from "./views/ModelsView";
import { TranscriptionsView } from "./views/TranscriptionsView";
import { SettingsView } from "./views/SettingsView";
import { WaveBar } from "@/components/brand/WaveBar";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogMedia,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";

type ViewId = "usage" | "models" | "transcriptions" | "settings";

function App() {
  const { t } = useTranslation();
  // The recording/transcription event handlers below are registered once in a
  // []-deps effect, so they capture this render's localizeError. Route it
  // through a ref to the latest `t` (react-i18next rebinds `t` per language) so
  // backend error keys are localized in the *current* UI language, not whatever
  // language was active when the listeners were first attached.
  const tRef = useRef(t);
  useEffect(() => {
    tRef.current = t;
  }, [t]);
  const localizeError = (msg: string) => localizeBackendError(tRef.current, msg);
  const [activeView, setActiveView] = useState<ViewId>("usage");
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => {
    return localStorage.getItem("sidebar_collapsed") === "true";
  });
  const [isRecording, setIsRecording] = useState(false);
  const [isTranscribing, setIsTranscribing] = useState(false);
  const [transcriptionProgress, setTranscriptionProgress] = useState<{
    done: number;
    total: number;
  } | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [errorActionView, setErrorActionView] = useState<ViewId>("models");
  // WAV path of the in-flight live session, consumed by the 'transcription-final' handler.
  const liveWavPathRef = useRef<string | null>(null);
  // Serializes incremental paste calls so typed characters never interleave or reorder.
  const pasteChainRef = useRef<Promise<void>>(Promise.resolve());

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
          // Keep the user's explicit choice when present, or when the list came
          // back empty (a transient enumeration glitch). Only fall back to the
          // system default when the device is genuinely gone from a populated list.
          if (list.includes(saved) || list.length === 0) {
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

    // Register the record/toggle shortcut on startup so the global hotkey works
    // before the Settings view is ever opened (required on Linux, where evdev
    // grabs are not persisted across launches like a compositor config bind).
    const savedRecordShortcut =
      localStorage.getItem("global_record_shortcut") ||
      "CommandOrControl+Shift+Space";
    if (savedRecordShortcut) {
      invoke("register_shortcut", {
        shortcutStr: savedRecordShortcut,
      }).catch((err) => {
        console.error("Failed to register record shortcut on mount:", err);
      });
    }

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
      setTranscriptionProgress(null);
      pasteChainRef.current = Promise.resolve();
      liveWavPathRef.current = null;
    };

    const handleStopped = async (event: any) => {
      const wavPath = event?.payload;
      setIsRecording(false);

      // Live mode: text is streamed to the overlay; the final text arrives via
      // 'transcription-final' (handled below). Skip the one-shot batch transcribe.
      const liveActive =
        localStorage.getItem("live_transcription_enabled") === "true" &&
        (localStorage.getItem("asr_engine") || "local") === "local";
      if (liveActive) {
        liveWavPathRef.current =
          wavPath && wavPath !== "Recording stopped" ? wavPath : null;
        return;
      }

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

            // Clipboard, auto-paste, the done sound, last-transcription and clearing
            // the transcribing indicator are all handled inside the Rust
            // `transcribe_audio` command now. The main window is `visible: false`, so
            // macOS can defer this command's response to the occluded webview until an
            // unrelated event wakes it; doing the user-facing work on the backend makes
            // it independent of that delivery. Only history persistence and the in-app
            // list refresh stay here (non-critical if they arrive late).
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

            window.dispatchEvent(
              new CustomEvent("transcription-added", {
                detail: { source: "recording" },
              }),
            );
            console.log("Transcription successful:", text);
          }
        }
      } catch (err: any) {
        console.error("Failed to transcribe recording:", err);
        const msg = typeof err === "string" ? err : err?.message || String(err);
        setErrorMessage(localizeError(msg));
      } finally {
        setIsTranscribing(false);
        setTranscriptionProgress(null);
        invoke("set_transcribing", { active: false }).catch(() => {});
      }
    };

    // Live mode: the streamed final text arrives here (no batch transcribe ran).
    // Paste it once and save to history, reusing the stashed WAV path.
    const handleFinal = async (event: any) => {
      const liveActive =
        localStorage.getItem("live_transcription_enabled") === "true" &&
        (localStorage.getItem("asr_engine") || "local") === "local";
      if (!liveActive) return;

      // Consume the stashed WAV path now so it never leaks into the next session.
      const wavPath = liveWavPathRef.current;
      liveWavPathRef.current = null;

      const text: string = event?.payload?.text || "";
      setIsTranscribing(false);
      setTranscriptionProgress(null);
      invoke("set_transcribing", { active: false }).catch(() => {});
      if (!text.trim()) return;

      const autopaste = localStorage.getItem("live_autopaste") !== "false";

      try {
        await invoke("play_done_sound");
      } catch (e) {
        console.warn("Failed to play sound:", e);
      }

      if (!autopaste) {
        // Not live-typing: type the whole text once at the end. (We use type_text,
        // not paste_text, because in live mode the clipboard is never populated.)
        invoke("type_text", { text }).catch((err) =>
          console.error("Typing failed:", err),
        );
      }

      try {
        await invoke("set_last_transcription", { text });
      } catch (e) {
        console.error("Failed to set last transcription:", e);
      }

      if (wavPath) {
        let modelName = "Whisper Local";
        try {
          const activeModelPath = await invoke<string | null>("get_active_model");
          if (activeModelPath) {
            const parts = activeModelPath.split(/[\/\\]/);
            modelName = parts[parts.length - 1];
          }
        } catch (e) {
          console.warn("Failed to resolve active model name:", e);
        }
        try {
          await invoke("save_transcription_data", { wavPath, text, model: modelName });
        } catch (saveErr) {
          console.error("Failed to save live transcription data:", saveErr);
        }
      }

      window.dispatchEvent(
        new CustomEvent("transcription-added", { detail: { source: "recording" } }),
      );
    };

    // Incremental live typing: type each committed delta into the active app, in
    // order, one paste at a time (so characters never interleave or reorder).
    const handleCommitted = (event: any) => {
      const live =
        localStorage.getItem("live_transcription_enabled") === "true" &&
        (localStorage.getItem("asr_engine") || "local") === "local";
      const autopaste = localStorage.getItem("live_autopaste") !== "false";
      if (!live || !autopaste) return;
      const delta: string = event?.payload?.delta || "";
      if (!delta) return;
      pasteChainRef.current = pasteChainRef.current
        .catch(() => {})
        .then(() => invoke("type_text", { text: delta }).then(() => {}))
        .catch((err) => console.error("Live typing failed:", err));
    };

    const unlistenStarted = listen("recording-started", handleStarted);
    const unlistenStopped = listen("recording-stopped", handleStopped);
    const unlistenCommitted = listen("transcription-committed", handleCommitted);
    const unlistenFinal = listen("transcription-final", handleFinal);
    const unlistenProgress = listen<{ done: number; total: number }>(
      "transcription-progress",
      (event) => setTranscriptionProgress(event.payload),
    );
    const unlistenFailed = listen<string>(
      "recording-failed-to-start",
      (event) => {
        const target: ViewId =
          event.payload === "errors.mic_unavailable" ? "settings" : "models";
        setErrorMessage(localizeError(event.payload));
        setErrorActionView(target);
        setActiveView(target);
      },
    );

    return () => {
      unlistenStarted.then((f) => f());
      unlistenStopped.then((f) => f());
      unlistenCommitted.then((f) => f());
      unlistenFinal.then((f) => f());
      unlistenProgress.then((f) => f());
      unlistenFailed.then((f) => f());
    };
  }, []);

  const getTitleName = (id: ViewId) => {
    return id.charAt(0).toUpperCase() + id.slice(1);
  };

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden bg-black relative">
        <TitleBar
          activeViewName={getTitleName(activeView)}
          toggleSidebar={toggleSidebar}
        />
        <AlertDialog
          open={!!errorMessage}
          onOpenChange={(open) => {
            if (!open) setErrorMessage(null);
          }}
        >
          <AlertDialogContent>
            <AlertDialogHeader>
              <AlertDialogMedia className="bg-danger/10 text-danger">
                <AlertTriangle />
              </AlertDialogMedia>
              <AlertDialogTitle>{t("errors.title")}</AlertDialogTitle>
              <AlertDialogDescription>{errorMessage}</AlertDialogDescription>
            </AlertDialogHeader>
            <AlertDialogFooter>
              <AlertDialogCancel>{t("common.dismiss")}</AlertDialogCancel>
              <AlertDialogAction
                onClick={() => {
                  setActiveView(errorActionView);
                  setErrorMessage(null);
                }}
              >
                {t(
                  errorActionView === "settings"
                    ? "errors.openSettings"
                    : "errors.openModels",
                )}
              </AlertDialogAction>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialog>
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
          <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm animate-[fadeIn_0.2s_ease-out]">
            <div className="flex flex-col items-center justify-center bg-popover/95 border border-border rounded-2xl p-8 max-w-sm w-full mx-4 shadow-[0_24px_64px_-16px_rgba(0,0,0,0.85)] backdrop-blur-md text-center">
              {isRecording ? (
                <>
                  <div className="relative mb-6">
                    {/* Pulsing outer ring */}
                    <div className="absolute inset-[-12px] rounded-full bg-red-500/10 animate-ping"></div>
                    <div className="absolute inset-[-6px] rounded-full bg-red-500/20 animate-pulse"></div>
                    {/* Recording circle */}
                    <div className="w-16 h-16 rounded-full bg-red-500 flex items-center justify-center shadow-lg shadow-red-500/30">
                      <div className="w-6 h-6 bg-white rounded-[5px] animate-pulse"></div>
                    </div>
                  </div>
                  <h2 className="text-white text-lg font-medium mb-1 tracking-tight">
                    {t("hud.recording")}
                  </h2>
                  <p className="text-muted text-sm leading-normal">
                    {t("hud.recordingHint")}
                  </p>
                </>
              ) : (
                <>
                  <WaveBar animated className="w-16 text-white mb-6" />
                  <h2 className="text-white text-lg font-medium mb-1 tracking-tight">
                    {t("hud.transcribing")}
                  </h2>
                  <p className="text-muted text-sm leading-normal">
                    {t("hud.transcribingHint")}
                  </p>
                  {transcriptionProgress && transcriptionProgress.total > 1 && (
                    <div className="w-full mt-4">
                      <div className="h-1.5 w-full rounded-full bg-white/10 overflow-hidden">
                        <div
                          className="h-full rounded-full bg-white/80 transition-all duration-300"
                          style={{
                            width: `${Math.round((transcriptionProgress.done / transcriptionProgress.total) * 100)}%`,
                          }}
                        />
                      </div>
                      <p className="text-muted text-xs mt-2 tabular-nums">
                        {Math.round((transcriptionProgress.done / transcriptionProgress.total) * 100)}%
                      </p>
                    </div>
                  )}
                </>
              )}
            </div>
          </div>
        )}
        <Updater />
        <OnboardingOverlay />
      </div>
  );
}

export default App;
