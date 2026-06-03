import { useEffect, useState } from "react";
import {
  FolderOpen,
  RefreshCw,
  Download,
  Loader2,
  Eye,
  EyeOff,
  X,
  AlertTriangle,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

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
    name: "Whisper Tiny (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-tiny.bin"],
    description: "Ultra-fast multilingual model by OpenAI. Tiny footprint, great for quick notes.",
    format: "ggml_bin",
    size_formatted: "74 MB"
  },
  {
    name: "Whisper Tiny English (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-tiny.en.bin"],
    description: "English-only Tiny. Even faster and more accurate when you only dictate in English.",
    format: "ggml_bin",
    size_formatted: "74 MB"
  },
  {
    name: "Whisper Base (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-base.bin"],
    description: "A step up from Tiny: better accuracy while staying light and fast. Multilingual.",
    format: "ggml_bin",
    size_formatted: "141 MB"
  },
  {
    name: "Whisper Small (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-small.bin"],
    description: "Balanced multilingual model. Reliable accuracy for everyday dictation.",
    format: "ggml_bin",
    size_formatted: "465 MB"
  },
  {
    name: "Whisper Small English (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-small.en.bin"],
    description: "English-only Small. Higher accuracy for English at the same speed.",
    format: "ggml_bin",
    size_formatted: "465 MB"
  },
  {
    name: "Parakeet TDT v2 (ONNX)",
    repo_id: "csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8",
    files: [
      "encoder.int8.onnx",
      "decoder.int8.onnx",
      "joiner.int8.onnx",
      "tokens.txt"
    ],
    description: "Previous-generation NVIDIA Parakeet (English). Very fast INT8 ASR on CPU.",
    format: "onnx",
    size_formatted: "631 MB"
  },
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
    size_formatted: "639 MB"
  },
  {
    name: "Whisper Medium (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-medium.bin"],
    description: "Larger multilingual model. Clearly better accuracy; needs more RAM and time.",
    format: "ggml_bin",
    size_formatted: "1.4 GB"
  },
  {
    name: "Whisper Large v3 Turbo (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-large-v3-turbo.bin"],
    description: "Newest large model tuned for speed. Near large-v3 quality, much faster.",
    format: "ggml_bin",
    size_formatted: "1.5 GB"
  },
  {
    name: "Whisper Large v2 (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-large-v2.bin"],
    description: "Proven previous flagship. Excellent multilingual accuracy.",
    format: "ggml_bin",
    size_formatted: "2.9 GB"
  },
  {
    name: "Whisper Large v3 (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-large-v3.bin"],
    description: "Most accurate multilingual Whisper. Top quality, highest resource use.",
    format: "ggml_bin",
    size_formatted: "2.9 GB"
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

const PROVIDER_LABELS: Record<string, string> = {
  openai: "OpenAI",
  openrouter: "OpenRouter",
  gemini: "Google Gemini",
  custom: "Custom…",
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
          <Badge
            variant="outline"
            className="rounded-md bg-surface-active text-muted font-mono text-[10px] shrink-0"
          >
            {formatLabel}
          </Badge>
        </div>
        {subtitle && <div className="mt-0.5">{subtitle}</div>}
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
            <Button
              variant="outline"
              size="sm"
              onClick={handleOpenFolder}
              title="Open models folder"
            >
              <FolderOpen size={13} />
              <span className="hidden sm:inline">Folder</span>
            </Button>
            <Button
              variant="outline"
              size="icon-sm"
              onClick={loadModelsList}
              disabled={scanning}
              title="Rescan models directory"
            >
              <RefreshCw size={13} className={scanning ? "animate-spin" : ""} />
            </Button>
          </div>
        )}
      </div>

      <Tabs
        value={asrEngine}
        onValueChange={(v) => handleSelectEngine(v as "local" | "openai-cloud")}
        className="w-full"
      >
        <TabsList variant="line" className="mb-6 border-b border-border w-full justify-start">
          <TabsTrigger value="local" className="flex-none px-4">
            Local
          </TabsTrigger>
          <TabsTrigger value="openai-cloud" className="flex-none px-4">
            Cloud (BYOK)
          </TabsTrigger>
        </TabsList>

        {/* Global error alerts */}
        {conversionError && (
          <Alert variant="destructive" className="mb-4 border-danger/20 bg-danger/5">
            <AlertTriangle />
            <AlertTitle>Conversion failed</AlertTitle>
            <AlertDescription>{conversionError.message}</AlertDescription>
            <button
              onClick={() => setConversionError(null)}
              className="absolute top-3 right-3 text-danger/60 hover:text-danger cursor-pointer"
              aria-label="Dismiss"
            >
              <X size={14} />
            </button>
          </Alert>
        )}
        {downloadError && (
          <Alert variant="destructive" className="mb-4 border-danger/20 bg-danger/5">
            <AlertTriangle />
            <AlertTitle>Download failed</AlertTitle>
            <AlertDescription>{downloadError.message}</AlertDescription>
            <button
              onClick={() => setDownloadError(null)}
              className="absolute top-3 right-3 text-danger/60 hover:text-danger cursor-pointer"
              aria-label="Dismiss"
            >
              <X size={14} />
            </button>
          </Alert>
        )}

        <TabsContent value="local" className="flex flex-col gap-6">
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
                      <span className="text-[10px] font-mono text-warning animate-pulse truncate">
                        {conversionStatus || "Converting..."}
                      </span>
                    );
                  } else {
                    action = (
                      <Button
                        size="sm"
                        onClick={() => handleConvertModel(model.path)}
                        disabled={convertingPath !== null || loadingModelPath !== null || loadingPath !== null}
                        className="w-full bg-warning text-black hover:bg-warning/90"
                      >
                        Convert
                      </Button>
                    );
                  }
                } else if (isActive) {
                  action = (
                    <Button variant="outline" size="sm" disabled className="w-full opacity-60">
                      Selected
                    </Button>
                  );
                } else {
                  action = (
                    <Button
                      size="sm"
                      onClick={() => handleLoadModel(model.path)}
                      disabled={isLoading || loadingModelPath !== null || loadingPath !== null || convertingPath !== null}
                      className="w-full"
                    >
                      {isLoading ? <Loader2 size={12} className="animate-spin" /> : "Load"}
                    </Button>
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
                      <div className="flex flex-col items-end gap-1 min-w-[96px] w-24">
                        <span className="text-[10px] font-mono text-info">
                          {Math.round(downloadProgress)}%
                        </span>
                        <Progress
                          value={downloadProgress}
                          className="w-full [&_[data-slot=progress-track]]:h-1 [&_[data-slot=progress-indicator]]:bg-info"
                        />
                      </div>
                    );
                  } else {
                    action = (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => handleDownloadModel(rec)}
                        disabled={downloadingRepo !== null || loadingModelPath !== null || loadingPath !== null}
                        className="w-full"
                      >
                        <Download size={11} />
                        Get
                      </Button>
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
            <div className="px-4 py-2 rounded-lg border border-info/15 bg-info/5 text-info text-[11px] font-mono">
              {downloadStatus}
            </div>
          )}
        </TabsContent>

        {/* BYOK Cloud Configuration */}
        <TabsContent value="openai-cloud" className="flex flex-col gap-5 max-w-xl">
          <div className="flex flex-col gap-2">
            <Label>Provider</Label>
            <Select
              value={asrProvider}
              onValueChange={(v) => handleProviderChange(v as typeof asrProvider)}
              items={PROVIDER_LABELS}
            >
              <SelectTrigger className="w-full bg-black">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="openai">OpenAI</SelectItem>
                <SelectItem value="openrouter">OpenRouter</SelectItem>
                <SelectItem value="gemini">Google Gemini</SelectItem>
                <SelectItem value="custom">Custom…</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="flex flex-col gap-2">
            <Label>API Key</Label>
            <div className="flex gap-2">
              <Input
                type={showApiKey ? "text" : "password"}
                value={providerKey}
                onChange={(e) => {
                  setProviderKey(e.target.value);
                  saveProviderKey(asrProvider, e.target.value);
                }}
                placeholder={
                  providerKey === "••••••••••••••••"
                    ? ""
                    : `Enter API key for ${asrProvider.toUpperCase()}…`
                }
                className="flex-1 bg-black font-mono"
              />
              <Button
                type="button"
                variant="outline"
                size="icon"
                onClick={() => setShowApiKey(!showApiKey)}
                title={showApiKey ? "Hide key" : "Show key"}
              >
                {showApiKey ? <EyeOff size={15} /> : <Eye size={15} />}
              </Button>
            </div>
            <p className="text-[11px] text-muted-foreground">
              Stored securely in your OS keyring — never written to disk.
            </p>
          </div>

          <div className="flex flex-col gap-2">
            <Label>Model</Label>
            <Select
              value={asrModel}
              onValueChange={(v) => handleModelChange(v as string)}
            >
              <SelectTrigger className="w-full bg-black">
                <SelectValue>
                  {(v: string) => (v === "custom" ? "Custom model…" : v)}
                </SelectValue>
              </SelectTrigger>
              <SelectContent>
                {asrProvider === "openai" && (
                  <>
                    <SelectItem value="whisper-1">whisper-1</SelectItem>
                    <SelectItem value="gpt-4o-mini">gpt-4o-mini</SelectItem>
                    <SelectItem value="gpt-4o">gpt-4o</SelectItem>
                  </>
                )}
                {asrProvider === "openrouter" && (
                  <>
                    <SelectItem value="openai/whisper-large-v3">
                      openai/whisper-large-v3
                    </SelectItem>
                    <SelectItem value="meta-llama/llama-3.2-11b-vision-instruct:free">
                      meta-llama/llama-3.2-11b-vision-instruct:free
                    </SelectItem>
                    <SelectItem value="deepseek/deepseek-chat">
                      deepseek/deepseek-chat
                    </SelectItem>
                    <SelectItem value="google/gemini-2.0-flash-exp:free">
                      google/gemini-2.0-flash-exp:free
                    </SelectItem>
                  </>
                )}
                {asrProvider === "gemini" && (
                  <>
                    <SelectItem value="gemini-1.5-flash">gemini-1.5-flash</SelectItem>
                    <SelectItem value="gemini-1.5-pro">gemini-1.5-pro</SelectItem>
                    <SelectItem value="gemini-2.0-flash-exp">
                      gemini-2.0-flash-exp
                    </SelectItem>
                  </>
                )}
                <SelectItem value="custom">Custom (type below)…</SelectItem>
              </SelectContent>
            </Select>
          </div>

          {isCustomModel && (
            <div className="flex flex-col gap-2">
              <Label>Custom Model ID</Label>
              <Input
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
                className="bg-black font-mono"
              />
            </div>
          )}

          <div className="flex flex-col gap-2">
            <Label>Base URL</Label>
            <Input
              type="text"
              value={asrBaseUrl}
              onChange={(e) => handleBaseUrlChange(e.target.value)}
              placeholder="e.g. https://api.openai.com/v1"
              className="bg-black font-mono"
            />
            <p className="text-[11px] text-muted-foreground">
              Leave empty for the standard OpenAI endpoint.
            </p>
          </div>
        </TabsContent>
      </Tabs>
    </div>
  );
}
