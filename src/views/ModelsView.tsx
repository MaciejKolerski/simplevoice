import { useEffect, useState } from "react";
import { FolderOpen, RefreshCw, ChevronDown, Download, Loader2 } from "lucide-react";
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
  format: string;
  architecture: string | null;
  hf_model_id: string | null;
  needs_conversion: boolean;
}

interface RecommendedModel {
  name: string;
  repo_id: string;
  files: string[];
  description: string;
  format: string;
  size_formatted: string;
}

const RECOMMENDED_MODELS: RecommendedModel[] = [
  {
    name: "Parakeet TDT v3 (ONNX)",
    repo_id: "csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8",
    files: [
      "encoder.int8.onnx",
      "decoder.int8.onnx",
      "joiner.int8.onnx",
      "tokens.txt"
    ],
    description: "State-of-the-art multilingual ASR by NVIDIA. INT8 quantized, runs natively on CPU.",
    format: "onnx",
    size_formatted: "600 MB"
  },
  {
    name: "Whisper Tiny (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-tiny.bin"],
    description: "Ultra-fast, tiny model by OpenAI. Low memory footprint, great for quick transcription.",
    format: "gguf",
    size_formatted: "75 MB"
  }
];

const FORMAT_LABELS: Record<string, string> = {
  ggml_bin: "GGML",
  gguf: "GGUF",
  hf_safetensors: "Safetensors",
  hf_pytorch: "PyTorch",
  onnx: "ONNX",
  nemo: "NeMo",
};

export function ModelsView() {
  const [models, setModels] = useState<LocalModel[]>([]);
  const [modelsDir, setModelsDir] = useState<string>("");
  const [loadingPath, setLoadingPath] = useState<string | null>(null);
  const [scanning, setScanning] = useState<boolean>(false);
  const [asrEngine, setAsrEngine] = useState<"local" | "openai-cloud">("local");
  const [providerKey, setProviderKey] = useState<string>("••••••••••••••••");

  const [activeModelPath, setActiveModelPath] = useState<string | null>(null);
  const [loadingModelPath, setLoadingModelPath] = useState<string | null>(null);

  // Conversion states
  const [convertingPath, setConvertingPath] = useState<string | null>(null);
  const [conversionStatus, setConversionStatus] = useState<string>("");
  const [conversionError, setConversionError] = useState<{ path: string; message: string } | null>(null);

  // Downloader states
  const [downloadingRepo, setDownloadingRepo] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<number>(0);
  const [downloadStatus, setDownloadStatus] = useState<string>("");
  const [downloadError, setDownloadError] = useState<{ repoId: string; message: string } | null>(null);

  // BYOK states
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
      setProviderKey(hasKey ? "••••••••••••••••" : "");
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

    let unlistenConversion: (() => void) | null = null;
    listen<string>("conversion-progress", (event) => {
      setConversionStatus(event.payload);
    }).then((fn) => {
      unlistenConversion = fn;
    });

    let unlistenDownload: (() => void) | null = null;
    listen<{
      repo_id: string;
      file: string;
      progress: number;
      current_file_index: number;
      total_files: number;
    }>("download-progress", (event) => {
      const { progress, file, current_file_index, total_files } = event.payload;
      setDownloadProgress(progress);
      setDownloadStatus(
        `Downloading ${current_file_index}/${total_files}: ${file} (${Math.round(progress)}%)`
      );
    }).then((fn) => {
      unlistenDownload = fn;
    });

    return () => {
      window.removeEventListener("asr-engine-changed", syncEngine);
      window.removeEventListener("api-keys-changed", handleKeyChange);
      if (unlistenStatus) unlistenStatus();
      if (unlistenConversion) unlistenConversion();
      if (unlistenDownload) unlistenDownload();
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

  const handleConvertModel = async (path: string) => {
    setConvertingPath(path);
    setConversionStatus("Starting...");
    setConversionError(null);
    try {
      await invoke("convert_model", { modelPath: path });
      await loadModelsList();
    } catch (err: any) {
      console.error("Failed to convert model:", err);
      setConversionError({ path, message: err?.toString() || "Unknown error occurred" });
    } finally {
      setConvertingPath(null);
      setConversionStatus("");
    }
  };

  const handleDownloadModel = async (model: RecommendedModel) => {
    setDownloadingRepo(model.repo_id);
    setDownloadProgress(0);
    setDownloadStatus("Starting download...");
    setDownloadError(null);
    try {
      await invoke("download_model", {
        repoId: model.repo_id,
        files: model.files,
      });
      await loadModelsList();
    } catch (err: any) {
      console.error("Failed to download model:", err);
      setDownloadError({
        repoId: model.repo_id,
        message: err?.toString() || "Unknown error occurred",
      });
    } finally {
      setDownloadingRepo(null);
      setDownloadStatus("");
    }
  };

  const isModelDownloaded = (model: RecommendedModel) => {
    if (model.files.length === 1) {
      const filename = model.files[0];
      return models.some((m) => m.filename === filename || m.path.endsWith(filename));
    }
    const folderName = model.repo_id.replace("/", "--");
    return models.some((m) => m.path.includes(folderName));
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

  const KNOWN_MODELS = new Set([
    "whisper-1", "gpt-4o-mini", "gpt-4o",
    "openai/whisper-large-v3", "meta-llama/llama-3.2-11b-vision-instruct:free",
    "deepseek/deepseek-chat", "google/gemini-2.0-flash-exp:free",
    "claude-3-5-haiku-20241022", "claude-3-5-sonnet-20241022", "claude-3-opus-20240229",
    "gemini-1.5-flash", "gemini-1.5-pro", "gemini-2.0-flash-exp",
  ]);
  const isCustomModel = asrModel === "custom" || !KNOWN_MODELS.has(asrModel);

  // Render a single model row (shared between local and recommended)
  const renderModelRow = (
    key: string,
    name: string,
    formatLabel: string,
    size: string,
    action: React.ReactNode,
    subtitle?: React.ReactNode,
    isLast?: boolean,
  ) => (
    <div
      key={key}
      className={`flex items-center gap-4 px-5 py-3.5 transition-colors hover:bg-surface-hover ${
        !isLast ? "border-b border-border/50" : ""
      }`}
    >
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2.5">
          <span className="text-sm font-medium text-white truncate">{name}</span>
          <span className="inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-mono font-medium text-muted bg-surface-active border border-border shrink-0">
            {formatLabel}
          </span>
        </div>
        {subtitle && (
          <div className="mt-0.5">{subtitle}</div>
        )}
      </div>
      <span className="text-xs font-mono text-muted shrink-0">{size}</span>
      <div className="shrink-0 w-24 flex justify-end">{action}</div>
    </div>
  );

  return (
    <div className="flex flex-col w-full">
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
          Models
        </h1>
        {asrEngine === "local" && (
          <div className="flex items-center gap-2">
            <button
              onClick={handleOpenFolder}
              className="btn btn-outline btn-small flex items-center gap-1.5 cursor-pointer"
              title="Open models folder"
            >
              <FolderOpen size={13} />
              <span className="hidden sm:inline">Folder</span>
            </button>
            <button
              onClick={loadModelsList}
              disabled={scanning}
              className="btn btn-outline btn-small flex items-center gap-1.5 cursor-pointer"
              title="Rescan models directory"
            >
              <RefreshCw size={13} className={scanning ? "animate-spin" : ""} />
            </button>
          </div>
        )}
      </div>

      {/* Tabs */}
      <div className="flex border-b border-border mb-6 gap-1">
        <button
          onClick={() => handleSelectEngine("local")}
          className={`px-4 py-2 text-xs font-semibold uppercase tracking-wider transition-all border-b-2 relative -mb-[2px] cursor-pointer ${
            asrEngine === "local"
              ? "text-white border-white"
              : "text-muted border-transparent hover:text-white"
          }`}
        >
          Local
        </button>
        <button
          onClick={() => handleSelectEngine("openai-cloud")}
          className={`px-4 py-2 text-xs font-semibold uppercase tracking-wider transition-all border-b-2 relative -mb-[2px] cursor-pointer ${
            asrEngine === "openai-cloud"
              ? "text-white border-white"
              : "text-muted border-transparent hover:text-white"
          }`}
        >
          Cloud (BYOK)
        </button>
      </div>

      {/* Global error alerts */}
      {conversionError && (
        <div className="mb-4 px-4 py-3 rounded-lg border border-rose-500/20 bg-rose-500/5 text-rose-400 text-xs flex items-start gap-2">
          <span className="shrink-0 mt-px">✕</span>
          <div>
            <span className="font-medium">Conversion failed:</span>{" "}
            {conversionError.message}
          </div>
          <button
            onClick={() => setConversionError(null)}
            className="ml-auto text-rose-400/60 hover:text-rose-400 cursor-pointer shrink-0"
          >
            ✕
          </button>
        </div>
      )}
      {downloadError && (
        <div className="mb-4 px-4 py-3 rounded-lg border border-rose-500/20 bg-rose-500/5 text-rose-400 text-xs flex items-start gap-2">
          <span className="shrink-0 mt-px">✕</span>
          <div>
            <span className="font-medium">Download failed:</span>{" "}
            {downloadError.message}
          </div>
          <button
            onClick={() => setDownloadError(null)}
            className="ml-auto text-rose-400/60 hover:text-rose-400 cursor-pointer shrink-0"
          >
            ✕
          </button>
        </div>
      )}

      {asrEngine === "local" ? (
        <div className="flex flex-col gap-6">
          {/* Installed models */}
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">
            {models.length === 0 ? (
              <div className="flex flex-col items-center justify-center py-16 px-6 text-center">
                <p className="text-muted text-sm mb-4">
                  No local models installed yet.
                </p>
                <p className="text-muted-foreground text-xs mb-1">
                  Download a recommended model below, or place model files in:
                </p>
                {modelsDir && (
                  <button
                    onClick={handleOpenFolder}
                    className="text-[11px] font-mono text-muted hover:text-white transition-colors cursor-pointer mt-1"
                  >
                    {modelsDir} →
                  </button>
                )}
              </div>
            ) : (
              models.map((model, idx) => {
                const isActive = model.path === activeModelPath;
                const isLoading =
                  model.path === loadingModelPath || loadingPath === model.path;
                const formatLabel = FORMAT_LABELS[model.format] || model.format.toUpperCase();

                let action: React.ReactNode;
                if (model.needs_conversion) {
                  if (convertingPath === model.path) {
                    action = (
                      <span className="text-[10px] font-mono text-amber-400 animate-pulse truncate">
                        {conversionStatus || "Converting..."}
                      </span>
                    );
                  } else {
                    action = (
                      <button
                        onClick={() => handleConvertModel(model.path)}
                        disabled={convertingPath !== null || loadingModelPath !== null || loadingPath !== null}
                        className="btn btn-small text-[11px] bg-amber-500 hover:bg-amber-600 border-amber-500 text-black font-medium cursor-pointer w-full h-[30px]"
                      >
                        Convert
                      </button>
                    );
                  }
                } else if (isActive) {
                  action = (
                    <button
                      disabled
                      className="btn btn-outline btn-small w-full h-[30px] opacity-50 cursor-not-allowed"
                    >
                      Selected
                    </button>
                  );
                } else {
                  action = (
                    <button
                      onClick={() => handleLoadModel(model.path)}
                      disabled={isLoading || loadingModelPath !== null || loadingPath !== null || convertingPath !== null}
                      className="btn btn-primary btn-small w-full h-[30px] flex items-center justify-center cursor-pointer"
                    >
                      {isLoading ? (
                        <Loader2 size={12} className="animate-spin" />
                      ) : (
                        "Load"
                      )}
                    </button>
                  );
                }

                return renderModelRow(
                  model.path,
                  model.name,
                  formatLabel,
                  model.size_formatted,
                  action,
                  undefined,
                  idx === models.length - 1 && RECOMMENDED_MODELS.every(r => isModelDownloaded(r)),
                );
              })
            )}

            {/* Recommended models — integrated as continuation */}
            {RECOMMENDED_MODELS.some(r => !isModelDownloaded(r)) && (
              <>
                {models.length > 0 && (
                  <div className="px-5 py-2.5 bg-black/30 border-y border-border/50">
                    <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                      Available for Download
                    </span>
                  </div>
                )}
                {RECOMMENDED_MODELS.filter(r => !isModelDownloaded(r)).map((rec, idx, arr) => {
                  const isDownloading = downloadingRepo === rec.repo_id;

                  let action: React.ReactNode;
                  if (isDownloading) {
                    action = (
                      <div className="flex flex-col items-end gap-1 min-w-[96px]">
                        <span className="text-[10px] font-mono text-sky-400 animate-pulse">
                          {Math.round(downloadProgress)}%
                        </span>
                        <div className="w-full h-1 bg-surface-active rounded-full overflow-hidden">
                          <div
                            className="h-full bg-sky-400 rounded-full transition-all duration-300"
                            style={{ width: `${downloadProgress}%` }}
                          />
                        </div>
                      </div>
                    );
                  } else {
                    action = (
                      <button
                        onClick={() => handleDownloadModel(rec)}
                        disabled={downloadingRepo !== null || loadingModelPath !== null || loadingPath !== null}
                        className="btn btn-outline btn-small w-full flex items-center justify-center gap-1.5 cursor-pointer"
                      >
                        <Download size={11} />
                        <span>Get</span>
                      </button>
                    );
                  }

                  return renderModelRow(
                    rec.repo_id,
                    rec.name,
                    FORMAT_LABELS[rec.format] || rec.format.toUpperCase(),
                    rec.size_formatted,
                    action,
                    <p className="text-[11px] text-muted leading-snug m-0 max-w-md">{rec.description}</p>,
                    idx === arr.length - 1,
                  );
                })}
              </>
            )}
          </div>

          {/* Download status bar */}
          {downloadingRepo && downloadStatus && (
            <div className="px-4 py-2 rounded-lg border border-sky-500/15 bg-sky-500/5 text-sky-400 text-[11px] font-mono">
              {downloadStatus}
            </div>
          )}
        </div>
      ) : (
        /* BYOK Cloud Configuration */
        <div className="flex flex-col gap-5 max-w-xl">
          <div className="flex flex-col">
            <label className="text-fg font-medium text-xs mb-2">
              Provider
            </label>
            <div className="relative w-full">
              <select
                value={asrProvider}
                onChange={(e) => handleProviderChange(e.target.value as any)}
                className="input w-full bg-black border-border rounded-md pl-4 pr-10 py-2.5 appearance-none cursor-pointer hover:border-muted transition-colors text-xs font-medium"
              >
                <option value="openai">OpenAI</option>
                <option value="openrouter">OpenRouter</option>
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
              API Key
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
                className="btn btn-outline btn-small text-xs font-medium cursor-pointer whitespace-nowrap"
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
          </div>

          {isCustomModel && (
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
            <p className="text-[11px] text-muted-foreground mt-1.5">
              Leave empty for standard OpenAI endpoint.
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
