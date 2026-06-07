import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  FolderOpen,
  RefreshCw,
  Download,
  Loader2,
  Eye,
  EyeOff,
  X,
  AlertTriangle,
  Cloud,
  Pause,
  Play,
  PlugZap,
  Trash2,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
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
  descriptionKey: string;
  format: string;
  size_formatted: string;
}

const RECOMMENDED_MODELS: RecommendedModel[] = [
  {
    name: "Whisper Tiny (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-tiny.bin"],
    descriptionKey: "models.desc.whisperTiny",
    format: "ggml_bin",
    size_formatted: "74 MB"
  },
  {
    name: "Whisper Tiny English (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-tiny.en.bin"],
    descriptionKey: "models.desc.whisperTinyEn",
    format: "ggml_bin",
    size_formatted: "74 MB"
  },
  {
    name: "Whisper Base (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-base.bin"],
    descriptionKey: "models.desc.whisperBase",
    format: "ggml_bin",
    size_formatted: "141 MB"
  },
  {
    name: "Whisper Small (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-small.bin"],
    descriptionKey: "models.desc.whisperSmall",
    format: "ggml_bin",
    size_formatted: "465 MB"
  },
  {
    name: "Whisper Small English (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-small.en.bin"],
    descriptionKey: "models.desc.whisperSmallEn",
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
    descriptionKey: "models.desc.parakeetV2",
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
    descriptionKey: "models.desc.parakeetV3",
    format: "onnx",
    size_formatted: "639 MB"
  },
  {
    name: "Whisper Medium (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-medium.bin"],
    descriptionKey: "models.desc.whisperMedium",
    format: "ggml_bin",
    size_formatted: "1.4 GB"
  },
  {
    name: "Whisper Large v3 Turbo (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-large-v3-turbo.bin"],
    descriptionKey: "models.desc.whisperLargeV3Turbo",
    format: "ggml_bin",
    size_formatted: "1.5 GB"
  },
  {
    name: "Whisper Large v2 (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-large-v2.bin"],
    descriptionKey: "models.desc.whisperLargeV2",
    format: "ggml_bin",
    size_formatted: "2.9 GB"
  },
  {
    name: "Whisper Large v3 (GGML)",
    repo_id: "ggerganov/whisper.cpp",
    files: ["ggml-large-v3.bin"],
    descriptionKey: "models.desc.whisperLargeV3",
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

// Unique identifier for a recommended model. repo_id alone is NOT unique —
// every whisper.cpp GGML model shares "ggerganov/whisper.cpp" — so include the
// file list to distinguish them.
const modelKey = (model: RecommendedModel) =>
  `${model.repo_id}::${model.files.join("|")}`;

// Curated fallback shown when the provider's live model list is unavailable
// (no key yet, fetch error, or an empty response).
const FALLBACK_CLOUD_MODELS: Record<string, string[]> = {
  openai: ["whisper-1", "gpt-4o-transcribe", "gpt-4o-mini-transcribe"],
  openrouter: ["openai/whisper-large-v3"],
  gemini: ["gemini-1.5-flash", "gemini-1.5-pro", "gemini-2.0-flash-exp"],
  custom: [],
};

export function ModelsView() {
  const { t } = useTranslation();

  const PROVIDER_LABELS: Record<string, string> = {
    openai: "OpenAI",
    openrouter: "OpenRouter",
    gemini: "Google Gemini",
    custom: t("models.custom"),
  };

  const [models, setModels] = useState<LocalModel[]>([]);
  const [modelsDir, setModelsDir] = useState<string>("");
  const [loadingPath, setLoadingPath] = useState<string | null>(null);
  const [scanning, setScanning] = useState<boolean>(false);
  const [asrEngine, setAsrEngine] = useState<"local" | "openai-cloud">("local");
  const [providerKey, setProviderKey] = useState<string>("••••••••••••••••");

  const [activeModelPath, setActiveModelPath] = useState<string | null>(null);
  const [loadingModelPath, setLoadingModelPath] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ path: string; name: string } | null>(null);
  const [deletingPath, setDeletingPath] = useState<string | null>(null);

  // Conversion states
  const [convertingPath, setConvertingPath] = useState<string | null>(null);
  const [conversionStatus, setConversionStatus] = useState<string>("");
  const [conversionError, setConversionError] = useState<{ path: string; message: string } | null>(null);

  // Downloader states. Keyed by a unique per-model key (modelKey) because
  // several recommended models share the same repo_id (all whisper.cpp GGML).
  const [downloadingKey, setDownloadingKey] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<number>(0);
  const [downloadStatus, setDownloadStatus] = useState<string>("");
  const [downloadPaused, setDownloadPaused] = useState<boolean>(false);
  const [downloadError, setDownloadError] = useState<{ key: string; message: string } | null>(null);
  // Mirror of downloadingKey for the (mount-time, stale-closure-free) progress
  // listener, so it can ignore events from any download other than the active one.
  const downloadingKeyRef = useRef<string | null>(null);
  const setActiveDownloadKey = (key: string | null) => {
    downloadingKeyRef.current = key;
    setDownloadingKey(key);
  };

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

  // Cloud model-list + connection-test state
  const [cloudModels, setCloudModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState<boolean>(false);
  const [modelsFetchError, setModelsFetchError] = useState<string | null>(null);
  const [testing, setTesting] = useState<boolean>(false);

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
      download_id: string;
      repo_id: string;
      file: string;
      progress: number;
      current_file_index: number;
      total_files: number;
    }>("download-progress", (event) => {
      const { download_id, progress, file, current_file_index, total_files } =
        event.payload;
      // Ignore stragglers from a download that is no longer the active one.
      if (download_id !== downloadingKeyRef.current) return;
      setDownloadProgress(progress);
      setDownloadStatus(
        t("models.downloading", {
          index: current_file_index,
          total: total_files,
          file,
          pct: Math.round(progress),
        }),
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

  // Auto-fetch the live model list when on the Cloud tab and a key is present.
  // Debounced so typing a key doesn't fire a request per keystroke.
  useEffect(() => {
    if (asrEngine !== "openai-cloud") return;
    if (!providerKey) return; // no key -> keep the curated fallback
    if (asrProvider === "anthropic") return; // can't list/transcribe; keep fallback
    const handle = setTimeout(() => {
      fetchCloudModels().catch(() => {});
    }, 600);
    return () => clearTimeout(handle);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asrEngine, asrProvider, asrBaseUrl, providerKey]);

  const handleProviderChange = (
    provider: "openai" | "openrouter" | "anthropic" | "gemini" | "custom",
  ) => {
    setModelsFetchError(null);
    setCloudModels([]);
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

  const fetchCloudModels = async (): Promise<string[]> => {
    setModelsLoading(true);
    setModelsFetchError(null);
    try {
      const list = await invoke<string[]>("list_cloud_models", {
        provider: asrProvider,
        baseUrl: asrBaseUrl,
      });
      setCloudModels(list);
      return list;
    } catch (err: any) {
      setCloudModels([]);
      setModelsFetchError(err?.toString() || t("models.modelsFetchFailed"));
      throw err;
    } finally {
      setModelsLoading(false);
    }
  };

  const handleTestConnection = async () => {
    setTesting(true);
    try {
      await fetchCloudModels();
      toast.success(t("models.testOk"));
    } catch (err: any) {
      toast.error(t("models.testFailed"), { description: err?.toString() });
    } finally {
      setTesting(false);
    }
  };

  const handleRefreshModels = () => {
    fetchCloudModels().catch(() => {});
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
      toast.error(t("models.loadFailed"), { description: err?.toString() });
    } finally {
      setLoadingPath(null);
    }
  };

  const requestDeleteModel = (path: string, name: string) => {
    setDeleteTarget({ path, name });
  };

  const handleDeleteModel = async () => {
    if (!deleteTarget) return;
    const { path } = deleteTarget;
    setDeletingPath(path);
    try {
      await invoke("delete_model", { path });
      if (localStorage.getItem("active_local_model_path") === path) {
        localStorage.removeItem("active_local_model_path");
        window.dispatchEvent(new Event("asr-engine-changed"));
      }
      await loadModelsList();
      toast.success(t("models.deleted"));
    } catch (err: any) {
      toast.error(t("models.deleteFailed"), { description: err?.toString() });
    } finally {
      setDeletingPath(null);
      setDeleteTarget(null);
    }
  };

  const handleConvertModel = async (path: string) => {
    setConvertingPath(path);
    setConversionStatus(t("models.starting"));
    setConversionError(null);
    try {
      await invoke("convert_model", { modelPath: path });
      await loadModelsList();
    } catch (err: any) {
      console.error("Failed to convert model:", err);
      setConversionError({ path, message: err?.toString() || t("models.unknownError") });
    } finally {
      setConvertingPath(null);
      setConversionStatus("");
    }
  };

  const runDownload = async (model: RecommendedModel) => {
    const key = modelKey(model);
    setActiveDownloadKey(key);
    setDownloadPaused(false);
    setDownloadError(null);
    try {
      const outcome = await invoke<string>("download_model", {
        repoId: model.repo_id,
        files: model.files,
        downloadId: key,
      });
      if (outcome === "paused") {
        // Keep the row in its paused state (progress + resume/cancel controls).
        setDownloadPaused(true);
        setDownloadStatus(t("models.paused"));
        return;
      }
      // "completed" or "cancelled" -> tear down the active-download UI.
      setActiveDownloadKey(null);
      setDownloadPaused(false);
      setDownloadProgress(0);
      setDownloadStatus("");
      if (outcome === "completed") {
        await loadModelsList();
      }
    } catch (err: any) {
      console.error("Failed to download model:", err);
      setDownloadError({
        key,
        message: err?.toString() || t("models.unknownError"),
      });
      setActiveDownloadKey(null);
      setDownloadPaused(false);
      setDownloadProgress(0);
      setDownloadStatus("");
    }
  };

  const handleDownloadModel = (model: RecommendedModel) => {
    setDownloadProgress(0);
    setDownloadStatus(t("models.startingDownload"));
    runDownload(model);
  };

  // Resume keeps the current progress; the backend continues from the partial file.
  const handleResumeDownload = (model: RecommendedModel) => {
    setDownloadStatus(t("models.startingDownload"));
    runDownload(model);
  };

  const handlePauseDownload = async () => {
    if (!downloadingKey) return;
    try {
      await invoke("pause_download", { downloadId: downloadingKey });
    } catch (err) {
      console.error("Failed to pause download:", err);
    }
  };

  const handleCancelDownload = async (model: RecommendedModel) => {
    if (!downloadingKey) return;
    if (downloadPaused) {
      // Paused downloads have no running task to signal — remove partial data
      // directly and tear down the UI ourselves.
      try {
        await invoke("discard_download", {
          repoId: model.repo_id,
          files: model.files,
          downloadId: downloadingKey,
        });
      } catch (err) {
        console.error("Failed to discard download:", err);
      }
      setActiveDownloadKey(null);
      setDownloadPaused(false);
      setDownloadProgress(0);
      setDownloadStatus("");
      return;
    }
    // Active download: signal the loop, which cleans up and resolves runDownload.
    try {
      await invoke("cancel_download", { downloadId: downloadingKey });
    } catch (err) {
      console.error("Failed to cancel download:", err);
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
  const modelOptions =
    cloudModels.length > 0 ? cloudModels : FALLBACK_CLOUD_MODELS[asrProvider] || [];
  const isCustomModel =
    asrModel === "custom" ||
    (!modelOptions.includes(asrModel) && !KNOWN_MODELS.has(asrModel));

  // Render a single model row (shared between local and recommended)
  const renderModelRow = (
    key: string,
    name: string,
    formatLabel: string,
    size: string,
    action: React.ReactNode,
    subtitle?: React.ReactNode,
    isLast?: boolean,
    onDelete?: () => void,
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
      {onDelete && (
        <button
          onClick={onDelete}
          disabled={
            loadingModelPath !== null ||
            loadingPath !== null ||
            convertingPath !== null ||
            deletingPath !== null
          }
          className="shrink-0 text-muted hover:text-danger transition-colors cursor-pointer p-1 disabled:opacity-40 disabled:cursor-not-allowed"
          title={t("models.delete")}
          aria-label={t("models.delete")}
        >
          <Trash2 size={15} />
        </button>
      )}
      <div className="shrink-0 w-24 flex justify-end">{action}</div>
    </div>
  );

  return (
    <div className="flex flex-col w-full">
      <AlertDialog
        open={!!deleteTarget}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("models.deleteConfirmTitle")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("models.deleteConfirmBody", { name: deleteTarget?.name ?? "" })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleDeleteModel}
              disabled={deletingPath !== null}
            >
              {t("models.deleteConfirm")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
          {t("nav.models")}
        </h1>
        {asrEngine === "local" && (
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={handleOpenFolder}
              title={t("models.openFolderTooltip")}
            >
              <FolderOpen size={13} />
              <span className="hidden sm:inline">{t("models.folder")}</span>
            </Button>
            <Button
              variant="outline"
              size="icon-sm"
              onClick={loadModelsList}
              disabled={scanning}
              title={t("models.rescanTooltip")}
            >
              <RefreshCw size={13} className={scanning ? "animate-spin" : ""} />
            </Button>
          </div>
        )}
      </div>

      <div data-tour="engine-tabs" className="w-full">
      <Tabs
        value={asrEngine}
        onValueChange={(v) => handleSelectEngine(v as "local" | "openai-cloud")}
        className="w-full"
      >
        <TabsList variant="line" className="mb-6 border-b border-border w-full justify-start">
          <TabsTrigger value="local" className="flex-none px-4">
            {t("models.tabLocal")}
          </TabsTrigger>
          <TabsTrigger value="openai-cloud" className="flex-none px-4">
            {t("models.tabCloud")}
          </TabsTrigger>
        </TabsList>

        {/* Global error alerts */}
        {conversionError && (
          <Alert variant="destructive" className="mb-4 border-danger/20 bg-danger/5">
            <AlertTriangle />
            <AlertTitle>{t("models.conversionFailed")}</AlertTitle>
            <AlertDescription>{conversionError.message}</AlertDescription>
            <button
              onClick={() => setConversionError(null)}
              className="absolute top-3 right-3 text-danger/60 hover:text-danger cursor-pointer"
              aria-label={t("common.dismiss")}
            >
              <X size={14} />
            </button>
          </Alert>
        )}
        {downloadError && (
          <Alert variant="destructive" className="mb-4 border-danger/20 bg-danger/5">
            <AlertTriangle />
            <AlertTitle>{t("models.downloadFailed")}</AlertTitle>
            <AlertDescription>{downloadError.message}</AlertDescription>
            <button
              onClick={() => setDownloadError(null)}
              className="absolute top-3 right-3 text-danger/60 hover:text-danger cursor-pointer"
              aria-label={t("common.dismiss")}
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
                  {t("models.emptyTitle")}
                </p>
                <p className="text-muted-foreground text-xs mb-1">
                  {t("models.emptyHint")}
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
                        {conversionStatus || t("models.converting")}
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
                        {t("models.convert")}
                      </Button>
                    );
                  }
                } else if (isActive) {
                  action = (
                    <Button variant="outline" size="sm" disabled className="w-full opacity-60">
                      {t("models.selected")}
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
                      {isLoading ? <Loader2 size={12} className="animate-spin" /> : t("models.load")}
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
                  () => requestDeleteModel(model.path, model.name),
                );
              })
            )}

            {/* Recommended models — integrated as continuation */}
            {RECOMMENDED_MODELS.some(r => !isModelDownloaded(r)) && (
              <>
                {models.length > 0 && (
                  <div className="px-5 py-2.5 bg-black/30 border-y border-border/50">
                    <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                      {t("models.availableForDownload")}
                    </span>
                  </div>
                )}
                {RECOMMENDED_MODELS.filter(r => !isModelDownloaded(r)).map((rec, idx, arr) => {
                  const isDownloading = downloadingKey === modelKey(rec);

                  let action: React.ReactNode;
                  if (isDownloading) {
                    action = (
                      <div className="flex flex-col items-end gap-1 min-w-[96px] w-24">
                        <span className="text-[10px] font-mono text-info">
                          {Math.round(downloadProgress)}%
                        </span>
                        <Progress
                          value={downloadProgress}
                          className={`w-full [&_[data-slot=progress-track]]:h-1 [&_[data-slot=progress-indicator]]:bg-info ${
                            downloadPaused ? "opacity-40" : ""
                          }`}
                        />
                        <div className="flex items-center gap-2 mt-0.5">
                          {downloadPaused ? (
                            <button
                              onClick={() => handleResumeDownload(rec)}
                              title={t("models.resume")}
                              aria-label={t("models.resume")}
                              className="text-muted hover:text-info transition-colors cursor-pointer"
                            >
                              <Play size={13} />
                            </button>
                          ) : (
                            <button
                              onClick={handlePauseDownload}
                              title={t("models.pause")}
                              aria-label={t("models.pause")}
                              className="text-muted hover:text-info transition-colors cursor-pointer"
                            >
                              <Pause size={13} />
                            </button>
                          )}
                          <button
                            onClick={() => handleCancelDownload(rec)}
                            title={t("models.cancelDownload")}
                            aria-label={t("models.cancelDownload")}
                            className="text-muted hover:text-danger transition-colors cursor-pointer"
                          >
                            <X size={13} />
                          </button>
                        </div>
                      </div>
                    );
                  } else {
                    action = (
                      <Button
                        variant="outline"
                        size="sm"
                        onClick={() => handleDownloadModel(rec)}
                        disabled={downloadingKey !== null || loadingModelPath !== null || loadingPath !== null}
                        className="w-full"
                      >
                        <Download size={11} />
                        {t("models.get")}
                      </Button>
                    );
                  }

                  return renderModelRow(
                    modelKey(rec),
                    rec.name,
                    FORMAT_LABELS[rec.format] || rec.format.toUpperCase(),
                    rec.size_formatted,
                    action,
                    <p className="text-[11px] text-muted leading-snug m-0 max-w-md">{t(rec.descriptionKey)}</p>,
                    idx === arr.length - 1,
                  );
                })}
              </>
            )}
          </div>

          {/* Download status bar */}
          {downloadingKey && downloadStatus && (
            <div className="px-4 py-2 rounded-lg border border-info/15 bg-info/5 text-info text-[11px] font-mono">
              {downloadStatus}
            </div>
          )}
        </TabsContent>

        {/* BYOK Cloud Configuration */}
        <TabsContent value="openai-cloud" className="flex flex-col">
          <div className="flex items-center justify-between gap-4 mb-4">
            <h2 className="m-0 text-base text-white font-medium flex items-center gap-2">
              <Cloud size={16} className="text-muted" /> {t("models.cloudProvider")}
            </h2>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={handleTestConnection}
              disabled={testing || !providerKey}
            >
              {testing ? (
                <Loader2 size={13} className="animate-spin" />
              ) : (
                <PlugZap size={13} />
              )}
              {testing ? t("models.testing") : t("models.test")}
            </Button>
          </div>
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">
            <div className="flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0">
              <div className="flex-1 min-w-0">
                <div className="text-fg font-medium mb-1">{t("models.providerLabel")}</div>
                <div className="text-muted text-[13px]">
                  {t("models.providerDesc")}
                </div>
              </div>
              <Select
                value={asrProvider}
                onValueChange={(v) => handleProviderChange(v as typeof asrProvider)}
                items={PROVIDER_LABELS}
              >
                <SelectTrigger className="w-72 bg-black shrink-0">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="openai">OpenAI</SelectItem>
                  <SelectItem value="openrouter">OpenRouter</SelectItem>
                  <SelectItem value="gemini">Google Gemini</SelectItem>
                  <SelectItem value="custom">{t("models.custom")}</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0">
              <div className="flex-1 min-w-0">
                <div className="text-fg font-medium mb-1">{t("models.apiKeyLabel")}</div>
                <div className="text-muted text-[13px]">
                  {t("models.apiKeyDesc")}
                </div>
              </div>
              <div className="flex gap-2 w-72 shrink-0">
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
                      : t("models.apiKeyPlaceholder", {
                          provider: asrProvider.toUpperCase(),
                        })
                  }
                  className="flex-1 bg-black font-mono"
                />
                <Button
                  type="button"
                  variant="outline"
                  size="icon"
                  onClick={() => setShowApiKey(!showApiKey)}
                  title={showApiKey ? t("models.hideKey") : t("models.showKey")}
                >
                  {showApiKey ? <EyeOff size={15} /> : <Eye size={15} />}
                </Button>
              </div>
            </div>

            <div className="flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0">
              <div className="flex-1 min-w-0">
                <div className="text-fg font-medium mb-1">{t("models.modelLabel")}</div>
                <div className="text-muted text-[13px]">
                  {t("models.modelDesc")}
                </div>
              </div>
              <div className="flex flex-col gap-1 w-72 shrink-0">
                <div className="flex items-center gap-2">
                  <Select
                    value={asrModel}
                    onValueChange={(v) => handleModelChange(v as string)}
                    disabled={modelsLoading}
                  >
                    <SelectTrigger className="flex-1 bg-black">
                      <SelectValue>
                        {(v: string) =>
                          v === "custom" ? t("models.customModel") : v
                        }
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      {modelOptions.map((m) => (
                        <SelectItem key={m} value={m}>
                          {m}
                        </SelectItem>
                      ))}
                      {asrModel &&
                        asrModel !== "custom" &&
                        !modelOptions.includes(asrModel) && (
                          <SelectItem key={asrModel} value={asrModel}>
                            {asrModel}
                          </SelectItem>
                        )}
                      <SelectItem value="custom">
                        {t("models.customTypeBelow")}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                  <Button
                    type="button"
                    variant="outline"
                    size="icon"
                    onClick={handleRefreshModels}
                    disabled={modelsLoading || !providerKey}
                    title={t("models.refreshModels")}
                  >
                    <RefreshCw
                      size={14}
                      className={modelsLoading ? "animate-spin" : ""}
                    />
                  </Button>
                </div>
                {!modelsLoading && modelsFetchError && (
                  <span className="text-[11px] text-danger truncate" title={modelsFetchError}>
                    {modelsFetchError}
                  </span>
                )}
                {!modelsLoading && !modelsFetchError && cloudModels.length === 0 && providerKey && (
                  <span className="text-[11px] text-muted">
                    {t("models.usingFallbackModels")}
                  </span>
                )}
              </div>
            </div>

            {isCustomModel && (
              <div className="flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0">
                <div className="flex-1 min-w-0">
                  <div className="text-fg font-medium mb-1">{t("models.customModelIdLabel")}</div>
                  <div className="text-muted text-[13px]">
                    {t("models.customModelIdDesc")}
                  </div>
                </div>
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
                  placeholder={t("models.customModelIdPlaceholder")}
                  className="w-72 shrink-0 bg-black font-mono"
                />
              </div>
            )}

            <div className="flex justify-between items-center gap-6 p-5 border-b border-border last:border-b-0">
              <div className="flex-1 min-w-0">
                <div className="text-fg font-medium mb-1">{t("models.baseUrlLabel")}</div>
                <div className="text-muted text-[13px]">
                  {t("models.baseUrlDesc")}
                </div>
              </div>
              <Input
                type="text"
                value={asrBaseUrl}
                onChange={(e) => handleBaseUrlChange(e.target.value)}
                placeholder={t("models.baseUrlPlaceholder")}
                className="w-72 shrink-0 bg-black font-mono"
              />
            </div>
          </div>
        </TabsContent>
      </Tabs>
      </div>
    </div>
  );
}
