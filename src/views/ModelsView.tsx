import { useEffect, useState } from "react";
import { FolderOpen, RefreshCw, Check, ChevronDown } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface ModelStatus {
  active: string | null;
  loading: string | null;
}

interface LocalModel {
  name: string;
  filename: string;
  path: string;
  size_bytes: number;
  size_formatted: string;
  quality: number;
  speed: number;
  is_active: boolean;
}

export function ModelsView() {
  const [models, setModels] = useState<LocalModel[]>([]);
  const [modelsDir, setModelsDir] = useState<string>("");
  const [loadingPath, setLoadingPath] = useState<string | null>(null);
  const [scanning, setScanning] = useState<boolean>(false);
  const [asrEngine, setAsrEngine] = useState<"local" | "openai-cloud">("local");
  const [providerKey, setProviderKey] = useState<string>("••••••••••••••••");

  const [activeModelPath, setActiveModelPath] = useState<string | null>(null);
  const [loadingModelPath, setLoadingModelPath] = useState<string | null>(null);

  // Custom BYOK states
  const [asrProvider, setAsrProvider] = useState<
    "openai" | "openrouter" | "anthropic" | "gemini" | "custom"
  >("openai");
  const [showApiKey, setShowApiKey] = useState<boolean>(false);
  const [asrModel, setAsrModel] = useState<string>("whisper-1");
  const [asrCustomModel, setAsrCustomModel] = useState<string>("");
  const [asrBaseUrl, setAsrBaseUrl] = useState<string>(
    "https://api.openai.com/v1",
  );

  const loadModelsList = async () => {
    setScanning(true);
    try {
      const status = await invoke<ModelStatus>("get_model_status");
      setActiveModelPath(status.active);
      setLoadingModelPath(status.loading);

      const list = await invoke<LocalModel[]>("scan_models");
      setModels(list);
      const dir = await invoke<string>("get_models_dir");
      setModelsDir(dir);
    } catch (err) {
      console.error("Failed to load models list:", err);
    } finally {
      setScanning(false);
    }
  };

  const loadSecureKeysForProvider = async (provider: string) => {
    try {
      const hasKey = await invoke<boolean>("has_secure_api_key", { provider });
      if (hasKey) {
        setProviderKey("••••••••••••••••");
      } else {
        setProviderKey("");
      }
    } catch (err) {
      console.error(`Failed to check secure API key for ${provider}:`, err);
    }
  };

  const saveProviderKey = async (provider: string, val: string) => {
    try {
      if (val === "••••••••••••••••") return;
      await invoke("set_secure_api_key", { provider, key: val });
      window.dispatchEvent(new Event("api-keys-changed"));
    } catch (err) {
      console.error(`Failed to save secure key for ${provider}:`, err);
    }
  };

  useEffect(() => {
    loadModelsList();

    const syncEngine = () => {
      const savedEngine =
        (localStorage.getItem("asr_engine") as any) || "local";
      setAsrEngine(savedEngine);
      const savedProvider =
        (localStorage.getItem("asr_provider") as any) || "openai";
      setAsrProvider(savedProvider);
      setAsrModel(localStorage.getItem("asr_model") || "whisper-1");
      setAsrCustomModel(localStorage.getItem("asr_custom_model") || "");
      setAsrBaseUrl(
        localStorage.getItem("asr_base_url") || "https://api.openai.com/v1",
      );
      loadSecureKeysForProvider(savedProvider);
    };
    syncEngine();

    const handleKeyChange = () => {
      const currentProvider = localStorage.getItem("asr_provider") || "openai";
      loadSecureKeysForProvider(currentProvider);
    };

    window.addEventListener("asr-engine-changed", syncEngine);
    window.addEventListener("api-keys-changed", handleKeyChange);

    let unlistenStatus: (() => void) | null = null;
    listen("model-status-changed", () => {
      loadModelsList();
    }).then((fn) => {
      unlistenStatus = fn;
    });

    return () => {
      window.removeEventListener("asr-engine-changed", syncEngine);
      window.removeEventListener("api-keys-changed", handleKeyChange);
      if (unlistenStatus) unlistenStatus();
    };
  }, []);

  const handleProviderChange = (
    provider: "openai" | "openrouter" | "anthropic" | "gemini" | "custom",
  ) => {
    setAsrProvider(provider);
    localStorage.setItem("asr_provider", provider);

    if (provider === "openai") {
      setAsrModel("whisper-1");
      localStorage.setItem("asr_model", "whisper-1");
      setAsrBaseUrl("https://api.openai.com/v1");
      localStorage.setItem("asr_base_url", "https://api.openai.com/v1");
    } else if (provider === "openrouter") {
      setAsrModel("openai/whisper-large-v3");
      localStorage.setItem("asr_model", "openai/whisper-large-v3");
      setAsrBaseUrl("https://openrouter.ai/api/v1");
      localStorage.setItem("asr_base_url", "https://openrouter.ai/api/v1");
    } else if (provider === "anthropic") {
      setAsrModel("claude-3-5-haiku-20241022");
      localStorage.setItem("asr_model", "claude-3-5-haiku-20241022");
      setAsrBaseUrl("https://api.anthropic.com/v1");
      localStorage.setItem("asr_base_url", "https://api.anthropic.com/v1");
    } else if (provider === "gemini") {
      setAsrModel("gemini-1.5-flash");
      localStorage.setItem("asr_model", "gemini-1.5-flash");
      setAsrBaseUrl("https://generativelanguage.googleapis.com/v1beta");
      localStorage.setItem(
        "asr_base_url",
        "https://generativelanguage.googleapis.com/v1beta",
      );
    } else if (provider === "custom") {
      setAsrModel("custom");
      localStorage.setItem("asr_model", "custom");
    }

    loadSecureKeysForProvider(provider);
    window.dispatchEvent(new Event("asr-engine-changed"));
  };

  const handleModelChange = (model: string) => {
    setAsrModel(model);
    localStorage.setItem("asr_model", model);
    window.dispatchEvent(new Event("asr-engine-changed"));
  };

  const handleCustomModelChange = (customModel: string) => {
    setAsrCustomModel(customModel);
    localStorage.setItem("asr_custom_model", customModel);
    window.dispatchEvent(new Event("asr-engine-changed"));
  };

  const handleBaseUrlChange = (url: string) => {
    setAsrBaseUrl(url);
    localStorage.setItem("asr_base_url", url);
    window.dispatchEvent(new Event("asr-engine-changed"));
  };

  const handleSelectEngine = (engine: "local" | "openai-cloud") => {
    setAsrEngine(engine);
    localStorage.setItem("asr_engine", engine);
    window.dispatchEvent(new Event("asr-engine-changed"));
  };

  const handleLoadModel = async (path: string) => {
    setLoadingPath(path);
    try {
      await invoke("load_model", { modelPath: path });
      localStorage.setItem("active_local_model_path", path);
      await loadModelsList();
      window.dispatchEvent(new Event("asr-engine-changed"));
    } catch (err) {
      console.error("Failed to load model:", err);
    } finally {
      setLoadingPath(null);
    }
  };

  const handleOpenFolder = async () => {
    if (modelsDir) {
      try {
        await invoke("open_folder", { path: modelsDir });
      } catch (err) {
        console.error("Failed to open folder:", err);
      }
    }
  };

  return (
    <div className="flex flex-col w-full">
      <div className="flex flex-col sm:flex-row justify-between items-start sm:items-end gap-4 mb-6">
        <div>
          <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
            Models & Engines
          </h1>
          <p className="text-xs text-muted mt-1 leading-normal">
            Choose whether to use local Whisper/Parakeet model files or
            high-speed cloud speech recognition.
          </p>
        </div>
        {asrEngine === "local" && (
          <div className="flex gap-2 shrink-0 w-full sm:w-auto">
            <button
              onClick={handleOpenFolder}
              className="btn btn-outline btn-small flex items-center gap-1.5 justify-center flex-1 sm:flex-initial"
            >
              <FolderOpen size={14} />
              <span>Open Directory</span>
            </button>
            <button
              onClick={loadModelsList}
              disabled={scanning}
              className="btn btn-outline btn-small flex items-center gap-1.5 justify-center flex-1 sm:flex-initial"
            >
              <RefreshCw size={14} className={scanning ? "animate-spin" : ""} />
              <span>Scan Directory</span>
            </button>
          </div>
        )}
      </div>

      {/* Tabs */}
      <div className="flex border-b border-border mb-8 gap-2">
        <button
          onClick={() => handleSelectEngine("local")}
          className={`px-4 py-2.5 text-xs font-semibold uppercase tracking-wider transition-all border-b-2 relative -mb-[2px] cursor-pointer ${
            asrEngine === "local"
              ? "text-white border-white"
              : "text-muted border-transparent hover:text-white"
          }`}
        >
          Local Models
        </button>
        <button
          onClick={() => handleSelectEngine("openai-cloud")}
          className={`px-4 py-2.5 text-xs font-semibold uppercase tracking-wider transition-all border-b-2 relative -mb-[2px] cursor-pointer ${
            asrEngine === "openai-cloud"
              ? "text-white border-white"
              : "text-muted border-transparent hover:text-white"
          }`}
        >
          BYOK
        </button>
      </div>

      {asrEngine === "openai-cloud" && (
        <div
          className={`mb-6 p-4 rounded-xl border flex items-center gap-3 transition-all duration-300 ${
            providerKey && providerKey !== ""
              ? "bg-emerald-500/5 border-emerald-500/20 text-emerald-400"
              : "bg-amber-500/5 border-amber-500/20 text-amber-400"
          }`}
        >
          <div
            className={`w-2 h-2 rounded-full ${providerKey && providerKey !== "" ? "bg-emerald-400 animate-pulse" : "bg-amber-400 animate-bounce"}`}
          />
          <div className="flex-1 text-xs">
            {providerKey && providerKey !== "" ? (
              <span>
                <strong>Active Cloud Engine (BYOK):</strong> Configured for{" "}
                <strong>{asrProvider.toUpperCase()}</strong> (Model:{" "}
                <code>
                  {asrModel === "custom" ? asrCustomModel || "None" : asrModel}
                </code>
                ). Ready for cloud transcription.
              </span>
            ) : (
              <span>
                <strong>
                  Missing API Key for {asrProvider.toUpperCase()}:
                </strong>{" "}
                Please enter your API Key below to activate cloud transcription.
              </span>
            )}
          </div>
        </div>
      )}

      {asrEngine === "local" ? (
        models.length === 0 ? (
          <div className="flex flex-col items-center justify-center p-12 text-center border border-dashed border-border rounded-xl bg-secondary">
            <FolderOpen size={48} className="text-muted mb-4 opacity-50" />
            <h3 className="text-white font-medium mb-2">
              No local models found
            </h3>
            <p className="text-muted text-sm max-w-md mb-6 leading-relaxed">
              Place your Whisper model files (with{" "}
              <code className="text-white font-mono bg-black/30 px-1.5 py-0.5 rounded">
                .bin
              </code>{" "}
              or{" "}
              <code className="text-white font-mono bg-black/30 px-1.5 py-0.5 rounded">
                .gguf
              </code>{" "}
              extension) or NVIDIA Parakeet{" "}
              <code className="text-white font-mono bg-black/30 px-1.5 py-0.5 rounded">
                .onnx
              </code>{" "}
              files inside the application models folder.
            </p>
            <div className="flex flex-col sm:flex-row gap-3 items-center w-full justify-center">
              <button onClick={handleOpenFolder} className="btn btn-outline">
                Open Models Folder
              </button>
              <button
                onClick={loadModelsList}
                className="btn btn-primary"
                disabled={scanning}
              >
                {scanning ? "Scanning..." : "Scan Directory"}
              </button>
            </div>
            {modelsDir && (
              <div className="mt-4 text-[10px] text-muted-dark font-mono break-all max-w-xl bg-black/20 p-2 rounded border border-border/50 select-text">
                {modelsDir}
              </div>
            )}
          </div>
        ) : (
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">
            {models.map((model, idx) => {
              const isActive = model.path === activeModelPath;
              const isLoading =
                model.path === loadingModelPath || loadingPath === model.path;
              return (
                <div
                  key={idx}
                  className={`flex flex-col lg:flex-row items-start lg:items-center p-5 transition-colors hover:bg-surface-hover gap-6 ${
                    idx < models.length - 1 ? "border-b border-border" : ""
                  }`}
                >
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-3 mb-1">
                      <h3 className="m-0 font-medium text-white truncate text-sm">
                        {model.name}
                      </h3>
                      {isActive && (
                        <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[11px] font-mono font-medium text-emerald-400 bg-emerald-400/5 border border-emerald-400/20 shrink-0">
                          <Check size={10} />
                          Active
                        </span>
                      )}
                      {isLoading && (
                        <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[11px] font-mono font-medium text-sky-400 bg-sky-400/5 border border-sky-400/20 shrink-0 animate-pulse">
                          Loading...
                        </span>
                      )}
                    </div>
                    <div className="text-muted text-xs truncate max-w-md">
                      File:{" "}
                      <span className="font-mono text-[11px] text-fg/80">
                        {model.filename}
                      </span>
                    </div>
                  </div>

                  <div className="flex flex-col sm:flex-row items-start sm:items-center gap-6 w-full lg:w-auto">
                    <div className="flex flex-col gap-2 w-full sm:w-[150px]">
                      <div className="text-[10px] text-muted-dark font-semibold uppercase tracking-wider flex justify-between items-center">
                        <span>Quality</span>
                        <span className="text-white font-mono">
                          {model.quality}%
                        </span>
                      </div>
                      <div className="w-full h-1 bg-surface-active rounded-full overflow-hidden">
                        <div
                          className="h-full bg-white rounded-full transition-all duration-500"
                          style={{ width: `${model.quality}%` }}
                        />
                      </div>
                    </div>

                    <div className="flex flex-col gap-2 w-full sm:w-[150px]">
                      <div className="text-[10px] text-muted-dark font-semibold uppercase tracking-wider flex justify-between items-center">
                        <span>Speed</span>
                        <span className="text-white font-mono">
                          {model.speed}x
                        </span>
                      </div>
                      <div className="w-full h-1 bg-surface-active rounded-full overflow-hidden">
                        <div
                          className="h-full bg-white rounded-full transition-all duration-500"
                          style={{
                            width: `${Math.min(model.speed * 20, 100)}%`,
                          }}
                        />
                      </div>
                    </div>

                    <div className="flex items-center justify-between sm:justify-end gap-4 w-full sm:w-auto border-t sm:border-t-0 border-border/50 pt-4 sm:pt-0">
                      <div className="text-xs font-mono text-muted">
                        {model.size_formatted}
                      </div>
                      {isActive ? (
                        <button
                          className="btn btn-outline btn-small disabled opacity-50 cursor-not-allowed w-20"
                          disabled
                        >
                          Loaded
                        </button>
                      ) : (
                        <button
                          onClick={() => handleLoadModel(model.path)}
                          disabled={
                            isLoading ||
                            loadingModelPath !== null ||
                            loadingPath !== null
                          }
                          className="btn btn-primary btn-small w-20 flex items-center justify-center cursor-pointer"
                        >
                          {isLoading ? (
                            <span className="flex items-center gap-1.5 justify-center">
                              <span className="w-1 h-1 rounded-full bg-white animate-ping"></span>
                              <span>...</span>
                            </span>
                          ) : (
                            "Load"
                          )}
                        </button>
                      )}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )
      ) : (
        <div className="border border-border rounded-xl p-8 bg-secondary flex flex-col gap-6">
          <div className="flex flex-col md:flex-row items-start md:items-center justify-between gap-6 border-b border-border/50 pb-6">
            <div className="flex-1">
              <h3 className="m-0 font-medium text-white text-base mb-2">
                BYOK Config
              </h3>
              <p className="text-muted text-[13px] max-w-xl leading-relaxed">
                Configure your cloud-based Speech-to-Text provider. All API
                requests are sent directly from your client machine to the
                designated endpoint.
              </p>
            </div>
          </div>

          <div className="max-w-xl space-y-6">
            <div className="flex flex-col">
              <label className="text-fg font-medium text-xs mb-2">
                Provider Preset
              </label>
              <div className="relative w-full">
                <select
                  value={asrProvider}
                  onChange={(e) => handleProviderChange(e.target.value as any)}
                  className="input w-full bg-black border-border rounded-md pl-4 pr-10 py-2.5 appearance-none cursor-pointer hover:border-muted transition-colors text-xs font-medium"
                >
                  <option value="openai">OpenAI</option>
                  <option value="openrouter">OpenRouter</option>
                  <option value="anthropic">Anthropic Claude</option>
                  <option value="gemini">Google Gemini</option>
                  <option value="custom">Custom...</option>
                </select>
                <div className="absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none text-muted">
                  <ChevronDown size={14} />
                </div>
              </div>
            </div>

            <div className="flex flex-col">
              <label className="text-fg font-medium text-xs mb-2">
                API Key ({asrProvider.toUpperCase()})
              </label>
              <div className="flex gap-2">
                <input
                  type={showApiKey ? "text" : "password"}
                  value={providerKey}
                  onChange={(e) => {
                    setProviderKey(e.target.value);
                    saveProviderKey(asrProvider, e.target.value);
                  }}
                  placeholder={
                    providerKey === "••••••••••••••••"
                      ? ""
                      : `Enter API Key for ${asrProvider.toUpperCase()}...`
                  }
                  className="input flex-1 bg-black border-border rounded-md px-4 py-2.5 text-xs focus:border-muted transition-colors"
                />
                <button
                  type="button"
                  onClick={() => setShowApiKey(!showApiKey)}
                  className="btn btn-outline min-w-[70px] flex items-center justify-center text-xs font-medium cursor-pointer"
                >
                  {showApiKey ? "Hide" : "Show"}
                </button>
              </div>
            </div>

            <div className="flex flex-col">
              <label className="text-fg font-medium text-xs mb-2">Model</label>
              <div className="relative w-full">
                <select
                  value={asrModel}
                  onChange={(e) => handleModelChange(e.target.value)}
                  className="input w-full bg-black border-border rounded-md pl-4 pr-10 py-2.5 appearance-none cursor-pointer hover:border-muted transition-colors text-xs font-medium"
                >
                  {asrProvider === "openai" && (
                    <>
                      <option value="whisper-1">whisper-1</option>
                      <option value="gpt-4o-mini">gpt-4o-mini</option>
                      <option value="gpt-4o">gpt-4o</option>
                    </>
                  )}
                  {asrProvider === "openrouter" && (
                    <>
                      <option value="openai/whisper-large-v3">
                        openai/whisper-large-v3
                      </option>
                      <option value="meta-llama/llama-3.2-11b-vision-instruct:free">
                        meta-llama/llama-3.2-11b-vision-instruct:free
                      </option>
                      <option value="deepseek/deepseek-chat">
                        deepseek/deepseek-chat
                      </option>
                      <option value="google/gemini-2.0-flash-exp:free">
                        google/gemini-2.0-flash-exp:free
                      </option>
                    </>
                  )}
                  {asrProvider === "anthropic" && (
                    <>
                      <option value="claude-3-5-haiku-20241022">
                        claude-3-5-haiku-20241022
                      </option>
                      <option value="claude-3-5-sonnet-20241022">
                        claude-3-5-sonnet-20241022
                      </option>
                      <option value="claude-3-opus-20240229">
                        claude-3-opus-20240229
                      </option>
                    </>
                  )}
                  {asrProvider === "gemini" && (
                    <>
                      <option value="gemini-1.5-flash">gemini-1.5-flash</option>
                      <option value="gemini-1.5-pro">gemini-1.5-pro</option>
                      <option value="gemini-2.0-flash-exp">
                        gemini-2.0-flash-exp
                      </option>
                    </>
                  )}
                  <option value="custom">Custom (type below)...</option>
                </select>
                <div className="absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none text-muted">
                  <ChevronDown size={14} />
                </div>
              </div>
              <p className="text-[11px] text-muted-dark mt-1.5">
                These are suggested models for this provider. Your API endpoint
                might support other models.
              </p>
            </div>

            {(asrModel === "custom" ||
              (asrModel !== "whisper-1" &&
                asrModel !== "gpt-4o-mini" &&
                asrModel !== "gpt-4o" &&
                asrModel !== "openai/whisper-large-v3" &&
                asrModel !== "meta-llama/llama-3.2-11b-vision-instruct:free" &&
                asrModel !== "deepseek/deepseek-chat" &&
                asrModel !== "google/gemini-2.0-flash-exp:free" &&
                asrModel !== "claude-3-5-haiku-20241022" &&
                asrModel !== "claude-3-5-sonnet-20241022" &&
                asrModel !== "claude-3-opus-20240229" &&
                asrModel !== "gemini-1.5-flash" &&
                asrModel !== "gemini-1.5-pro" &&
                asrModel !== "gemini-2.0-flash-exp")) && (
              <div className="flex flex-col">
                <label className="text-fg font-medium text-xs mb-2">
                  Custom Model ID
                </label>
                <input
                  type="text"
                  value={asrModel === "custom" ? asrCustomModel : asrModel}
                  onChange={(e) => {
                    if (asrModel === "custom") {
                      handleCustomModelChange(e.target.value);
                    } else {
                      handleModelChange(e.target.value);
                    }
                  }}
                  placeholder="e.g. openrouter/owl-alpha"
                  className="input w-full bg-black border-border rounded-md px-4 py-2.5 text-xs focus:border-muted transition-colors font-mono"
                />
              </div>
            )}

            <div className="flex flex-col">
              <label className="text-fg font-medium text-xs mb-2">
                Base URL
              </label>
              <input
                type="text"
                value={asrBaseUrl}
                onChange={(e) => handleBaseUrlChange(e.target.value)}
                placeholder="e.g. https://api.openai.com/v1"
                className="input w-full bg-black border-border rounded-md px-4 py-2.5 text-xs focus:border-muted transition-colors font-mono"
              />
              <p className="text-[11px] text-muted-dark mt-1.5">
                The base URL of the API server (e.g.
                https://openrouter.ai/api/v1). Leave empty for standard OpenAI.
              </p>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
