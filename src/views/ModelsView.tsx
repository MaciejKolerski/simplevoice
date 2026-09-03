import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { localizeError as localizeBackendError } from "@/lib/localizeError";
import {
  FolderOpen,
  RefreshCw,
  Download,
  Loader2,
  Check,
  Eye,
  EyeOff,
  X,
  AlertTriangle,
  Cloud,
  Pause,
  Play,
  Trash2,
  ExternalLink,
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
import { SettingRow } from "@/components/ui/setting-row";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  fallbackCloudProviders,
  fetchCloudProviders,
  getCloudProviderSettings,
  isCloudProviderId,
  setCloudProviderSetting,
  type CloudCredentialKind,
  type CloudModelInfo,
  type CloudProviderId,
  type CloudProviderInfo,
  type CloudProviderSettings,
} from "@/lib/byok";

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
  /** Highlight as a recommended default (best accuracy/speed tradeoff). */
  recommended?: boolean;
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
    size_formatted: "639 MB",
    recommended: true
  },
  {
    name: "Zipformer GigaSpeech (EN)",
    repo_id: "csukuangfj/sherpa-onnx-zipformer-gigaspeech-2023-12-12",
    files: [
      "encoder-epoch-30-avg-1.int8.onnx",
      "decoder-epoch-30-avg-1.int8.onnx",
      "joiner-epoch-30-avg-1.int8.onnx",
      "tokens.txt",
      "bpe.model"
    ],
    descriptionKey: "models.desc.zipformerGigaspeechEn",
    format: "onnx",
    size_formatted: "73 MB"
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
    size_formatted: "1.5 GB",
    recommended: true
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

const CREDENTIAL_LABEL_KEYS: Record<CloudCredentialKind, string> = {
  apiKey: "models.apiKeyLabel",
  apiToken: "models.apiTokenLabel",
  subscriptionKey: "models.subscriptionKeyLabel",
  serviceAccountJson: "models.serviceAccountJsonLabel",
  secretAccessKey: "models.secretAccessKeyLabel",
};

const PROVIDER_SETTING_KEYS: Record<
  string,
  { label: string; description: string; placeholder: string }
> = {
  accountId: {
    label: "models.accountIdLabel",
    description: "models.accountIdDesc",
    placeholder: "models.accountIdPlaceholder",
  },
  accessKeyId: {
    label: "models.accessKeyIdLabel",
    description: "models.accessKeyIdDesc",
    placeholder: "models.accessKeyIdPlaceholder",
  },
  region: {
    label: "models.regionLabel",
    description: "models.regionDesc",
    placeholder: "models.regionPlaceholder",
  },
};

// Unique identifier for a recommended model. repo_id alone is NOT unique —
// every whisper.cpp GGML model shares "ggerganov/whisper.cpp" — so include the
// file list to distinguish them.
const modelKey = (model: RecommendedModel) =>
  `${model.repo_id}::${model.files.join("|")}`;

export function ModelsView() {
  const { t } = useTranslation();
  const [cloudProviders, setCloudProviders] = useState<CloudProviderInfo[]>(
    fallbackCloudProviders,
  );
  const providerLabels = Object.fromEntries(
    cloudProviders.map((provider) => [provider.id, provider.name]),
  );

  const [models, setModels] = useState<LocalModel[]>([]);
  const [modelsDir, setModelsDir] = useState<string>("");
  const [loadingPath, setLoadingPath] = useState<string | null>(null);
  const [scanning, setScanning] = useState<boolean>(false);
  const [asrEngine, setAsrEngine] = useState<"local" | "openai-cloud">("local");
  // The stored key never round-trips into the UI: `hasStoredKey` is presence
  // only, `keyDraft` is the pending input. A masked value in the field would be
  // indistinguishable from a real one and get committed as the key.
  const [hasStoredKey, setHasStoredKey] = useState<boolean>(false);
  const [keyDraft, setKeyDraft] = useState<string>("");
  const [removeKeyOpen, setRemoveKeyOpen] = useState<boolean>(false);

  const [activeModelPath, setActiveModelPath] = useState<string | null>(null);
  const [loadingModelPath, setLoadingModelPath] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<{ path: string; name: string } | null>(null);
  const [deletingPath, setDeletingPath] = useState<string | null>(null);

  const [convertingPath, setConvertingPath] = useState<string | null>(null);
  const [conversionStatus, setConversionStatus] = useState<string>("");
  const [conversionError, setConversionError] = useState<{ path: string; message: string } | null>(null);

  // Key download state by a unique per-model key because
  // several recommended models share the same repo_id (all whisper.cpp GGML).
  const [downloadingKey, setDownloadingKey] = useState<string | null>(null);
  const [downloadProgress, setDownloadProgress] = useState<number>(0);
  const [downloadStatus, setDownloadStatus] = useState<string>("");
  const [downloadPaused, setDownloadPaused] = useState<boolean>(false);
  const [downloadError, setDownloadError] = useState<{ key: string; message: string } | null>(null);
  // Mirror of downloadingKey for the (mount-time, stale-closure-free) progress
  // listener, so it can ignore events from any download other than the active one.
  const downloadingKeyRef = useRef<string | null>(null);
  // The download-progress listener below is registered once at mount; `t` from
  // react-i18next is bound to the render that created it, so reading it directly
  // there would freeze the status text at the startup language. Mirror the
  // latest `t` into a ref, the same way downloadingKey is mirrored above.
  const tRef = useRef(t);
  useEffect(() => {
    tRef.current = t;
  }, [t]);
  // Backend errors arrive as errors.* keys; localize them in the current UI
  // language (tRef stays current even for long-lived handlers).
  const localizeError = (err: unknown) => localizeBackendError(tRef.current, err);
  const setActiveDownloadKey = (key: string | null) => {
    downloadingKeyRef.current = key;
    setDownloadingKey(key);
  };

  const [asrProvider, setAsrProvider] = useState<CloudProviderId>("openai");
  const [providerSettings, setProviderSettings] = useState<CloudProviderSettings>(() =>
    getCloudProviderSettings("openai"),
  );
  const [showApiKey, setShowApiKey] = useState<boolean>(false);
  const [asrModel, setAsrModel] = useState<string>("gpt-4o-mini-transcribe");

  const activeCloudProvider =
    cloudProviders.find((provider) => provider.id === asrProvider) ??
    fallbackCloudProviders().find((provider) => provider.id === asrProvider)!;
  const providerConfigurationReady = activeCloudProvider.requiredSettings.every((name) =>
    Boolean(providerSettings[name]?.trim()),
  );
  const credentialLabel = t(CREDENTIAL_LABEL_KEYS[activeCloudProvider.credentialKind]);

  const [cloudModels, setCloudModels] = useState<CloudModelInfo[]>([]);
  const [modelsLoading, setModelsLoading] = useState<boolean>(false);
  const [modelsFetchError, setModelsFetchError] = useState<string | null>(null);
  const modelRequestRef = useRef(0);
  const keyRequestRef = useRef(0);

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

  const loadSecureKeysForProvider = async (provider: CloudProviderId) => {
    const requestId = ++keyRequestRef.current;
    try {
      const hasKey = await invoke<boolean>("has_secure_api_key", { provider });
      if (requestId !== keyRequestRef.current) return;
      setHasStoredKey(hasKey);
      setKeyDraft("");
    } catch (err) {
      if (requestId !== keyRequestRef.current) return;
      setHasStoredKey(false);
      console.error(`Failed to check secure API key for ${provider}:`, err);
    }
  };

  // Committed on blur, never per keystroke, so a half-typed key never reaches
  // the system keyring.
  const commitProviderKey = async () => {
    const key = keyDraft.trim();
    if (!key) return;
    const provider = asrProvider;
    const replacesExistingKey = hasStoredKey;
    try {
      await invoke("set_secure_api_key", { provider, key });
      setKeyDraft("");
      setShowApiKey(false);
      if (localStorage.getItem("asr_provider") === provider) {
        setHasStoredKey(true);
        if (replacesExistingKey && providerConfigurationReady) {
          fetchCloudModels(provider).catch(() => {});
        }
      }
      toast.success(t("models.keySaved"));
      window.dispatchEvent(new Event("api-keys-changed"));
    } catch (err: any) {
      console.error(`Failed to save secure key for ${provider}:`, err);
      toast.error(t("models.keySaveFailed"), { description: localizeError(err) });
    }
  };

  const removeProviderKey = async () => {
    setRemoveKeyOpen(false);
    const provider = asrProvider;
    try {
      await invoke("delete_secure_api_key", { provider });
      if (localStorage.getItem("asr_provider") === provider) {
        setHasStoredKey(false);
        setKeyDraft("");
        setShowApiKey(false);
        setCloudModels([]);
        setModelsFetchError(null);
      }
      toast.success(t("models.keyRemoved"));
      window.dispatchEvent(new Event("api-keys-changed"));
    } catch (err: any) {
      console.error(`Failed to remove secure key for ${provider}:`, err);
      toast.error(t("models.keyRemoveFailed"), { description: localizeError(err) });
    }
  };

  useEffect(() => {
    loadModelsList();
    fetchCloudProviders()
      .then(setCloudProviders)
      .catch((err) => console.warn("Could not refresh the AI SDK provider catalog:", err));

    const syncEngine = () => {
      const savedEngine =
        (localStorage.getItem("asr_engine") as any) || "local";
      setAsrEngine(savedEngine);
      const storedProvider = localStorage.getItem("asr_provider");
      const savedProvider: CloudProviderId = isCloudProviderId(storedProvider)
        ? storedProvider
        : "openai";
      setAsrProvider(savedProvider);
      setProviderSettings(getCloudProviderSettings(savedProvider));
      const defaultModel = fallbackCloudProviders().find(
        (provider) => provider.id === savedProvider,
      )!.defaultModel;
      const savedModel =
        storedProvider === savedProvider
          ? localStorage.getItem("asr_model") || defaultModel
          : defaultModel;
      if (storedProvider !== savedProvider) {
        localStorage.setItem("asr_provider", savedProvider);
        localStorage.setItem("asr_model", savedModel);
      }
      setAsrModel(savedModel);
      loadSecureKeysForProvider(savedProvider);
    };
    syncEngine();

    const handleKeyChange = () => {
      const storedProvider = localStorage.getItem("asr_provider");
      const currentProvider = isCloudProviderId(storedProvider) ? storedProvider : "openai";
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
        tRef.current("models.downloading", {
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

  // Fetch only after a complete key has been committed to the system keyring.
  useEffect(() => {
    if (asrEngine !== "openai-cloud") return;
    if (!hasStoredKey || !providerConfigurationReady) return;
    fetchCloudModels().catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [asrEngine, asrProvider, hasStoredKey]);

  const handleProviderChange = (provider: CloudProviderId) => {
    modelRequestRef.current += 1;
    setModelsFetchError(null);
    setCloudModels([]);
    setModelsLoading(false);
    setHasStoredKey(false);
    setKeyDraft("");
    setShowApiKey(false);
    setAsrProvider(provider);
    setProviderSettings(getCloudProviderSettings(provider));
    localStorage.setItem("asr_provider", provider);
    const defaultModel =
      cloudProviders.find((item) => item.id === provider)?.defaultModel ||
      fallbackCloudProviders().find((item) => item.id === provider)!.defaultModel;
    setAsrModel(defaultModel);
    localStorage.setItem("asr_model", defaultModel);
    loadSecureKeysForProvider(provider);
    window.dispatchEvent(new Event("asr-engine-changed"));
  };

  const handleModelChange = (model: string) => {
    setAsrModel(model);
    localStorage.setItem("asr_model", model);
    window.dispatchEvent(new Event("asr-engine-changed"));
  };

  const handleProviderSettingChange = (name: string, value: string) => {
    setProviderSettings((current) => ({ ...current, [name]: value }));
  };

  const commitProviderSetting = (name: string) => {
    const settings = setCloudProviderSetting(
      asrProvider,
      name,
      providerSettings[name] ?? "",
    );
    setProviderSettings(settings);
    modelRequestRef.current += 1;
    setCloudModels([]);
    setModelsFetchError(null);
    const ready = activeCloudProvider.requiredSettings.every((field) =>
      Boolean(settings[field]?.trim()),
    );
    if (hasStoredKey && ready) fetchCloudModels(asrProvider).catch(() => {});
    window.dispatchEvent(new Event("asr-engine-changed"));
  };

  const handleSelectEngine = (engine: "local" | "openai-cloud") => {
    setAsrEngine(engine);
    localStorage.setItem("asr_engine", engine);
    window.dispatchEvent(new Event("asr-engine-changed"));
  };

  const fetchCloudModels = async (
    provider: CloudProviderId = asrProvider,
  ): Promise<CloudModelInfo[]> => {
    const requestId = ++modelRequestRef.current;
    setModelsLoading(true);
    setModelsFetchError(null);
    try {
      const list = await invoke<CloudModelInfo[]>("list_cloud_models", {
        provider,
        settings: getCloudProviderSettings(provider),
      });
      if (
        requestId !== modelRequestRef.current ||
        localStorage.getItem("asr_provider") !== provider
      ) {
        return list;
      }
      setCloudModels(list);
      const selectedModel = localStorage.getItem("asr_model") || asrModel;
      if (!list.some((model) => model.id === selectedModel)) {
        const preferredModel =
          cloudProviders.find((item) => item.id === provider)?.defaultModel;
        const nextModel =
          list.find((model) => model.id === preferredModel)?.id || list[0]?.id;
        if (nextModel) handleModelChange(nextModel);
      }
      return list;
    } catch (err: any) {
      if (
        requestId !== modelRequestRef.current ||
        localStorage.getItem("asr_provider") !== provider
      ) {
        return [];
      }
      setCloudModels([]);
      setModelsFetchError(localizeError(err) || t("models.modelsFetchFailed"));
      throw err;
    } finally {
      if (requestId === modelRequestRef.current) setModelsLoading(false);
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
      toast.error(t("models.loadFailed"), { description: localizeError(err) });
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
      toast.error(t("models.deleteFailed"), { description: localizeError(err) });
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
      setConversionError({ path, message: localizeError(err) || t("models.unknownError") });
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
        message: localizeError(err) || t("models.unknownError"),
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

  const handleOpenProviderDashboard = async () => {
    try {
      await invoke("open_folder", { path: activeCloudProvider.dashboardUrl });
    } catch (err) {
      console.error("Failed to open provider dashboard:", err);
    }
  };

  const selectedCloudModel = cloudModels.some((model) => model.id === asrModel)
    ? asrModel
    : null;

  const renderModelRow = (
    key: string,
    name: string,
    formatLabel: string,
    size: string,
    action: React.ReactNode,
    subtitle?: React.ReactNode,
    isLast?: boolean,
    onDelete?: () => void,
    recommended?: boolean,
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
          {recommended && (
            <Badge
              variant="outline"
              className="rounded-md border-primary/40 bg-primary/10 text-primary text-[10px] shrink-0"
            >
              {t("models.recommended")}
            </Badge>
          )}
        </div>
        {subtitle && <div className="mt-0.5">{subtitle}</div>}
      </div>
      <span className="text-xs font-mono text-muted shrink-0">{size}</span>
      {onDelete && (
        <Tooltip>
          <TooltipTrigger
            render={
              <button
                onClick={onDelete}
                disabled={
                  loadingModelPath !== null ||
                  loadingPath !== null ||
                  convertingPath !== null ||
                  deletingPath !== null
                }
                className="shrink-0 flex size-8 items-center justify-center text-muted hover:text-danger transition-colors cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed rounded-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/60"
                aria-label={t("models.delete")}
              >
                <Trash2 size={15} />
              </button>
            }
          />
          <TooltipContent>{t("models.delete")}</TooltipContent>
        </Tooltip>
      )}
      <div className="shrink-0 w-24 flex justify-end">{action}</div>
    </div>
  );

  return (
    <div className="flex flex-col w-full animate-[fadeIn_0.3s_ease-out]">
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
      <AlertDialog open={removeKeyOpen} onOpenChange={setRemoveKeyOpen}>
        <AlertDialogContent size="sm">
          <AlertDialogHeader>
            <AlertDialogTitle>{t("models.removeKeyConfirmTitle")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("models.removeKeyConfirmBody", {
                provider: providerLabels[asrProvider] ?? asrProvider,
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{t("common.cancel")}</AlertDialogCancel>
            <AlertDialogAction
              onClick={removeProviderKey}
              className="bg-danger text-white hover:bg-danger/90"
            >
              {t("models.removeKey")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <div className="flex items-center justify-between mb-6">
        <h1 className="m-0 text-2xl font-medium text-white tracking-tight">
          {t("nav.models")}
        </h1>
        {asrEngine === "local" && (
          <div className="flex items-center gap-2">
            <Tooltip>
              <TooltipTrigger
                render={
                  <Button variant="outline" size="sm" onClick={handleOpenFolder}>
                    <FolderOpen size={13} />
                    {t("models.folder")}
                  </Button>
                }
              />
              <TooltipContent>{t("models.openFolderTooltip")}</TooltipContent>
            </Tooltip>
            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    variant="outline"
                    size="icon-sm"
                    onClick={loadModelsList}
                    disabled={scanning}
                    aria-label={t("models.rescanTooltip")}
                  >
                    <RefreshCw size={13} className={scanning ? "animate-spin" : ""} />
                  </Button>
                }
              />
              <TooltipContent>{t("models.rescanTooltip")}</TooltipContent>
            </Tooltip>
          </div>
        )}
      </div>

      <div data-tour="engine-tabs" className="w-full">
      <Tabs
        value={asrEngine}
        onValueChange={(v) => handleSelectEngine(v as "local" | "openai-cloud")}
        className="w-full"
      >
        <TabsList
          variant="line"
          className="mb-6 border-b border-border w-full justify-start overflow-x-auto no-scrollbar"
        >
          <TabsTrigger value="local" className="flex-none px-4">
            {t("models.tabLocal")}
          </TabsTrigger>
          <TabsTrigger value="openai-cloud" className="flex-none px-4">
            {t("models.tabCloud")}
          </TabsTrigger>
        </TabsList>

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
                        <div className="flex items-center gap-1 mt-0.5">
                          <Tooltip>
                            <TooltipTrigger
                              render={
                                <button
                                  onClick={
                                    downloadPaused
                                      ? () => handleResumeDownload(rec)
                                      : handlePauseDownload
                                  }
                                  aria-label={
                                    downloadPaused ? t("models.resume") : t("models.pause")
                                  }
                                  className="flex size-6 items-center justify-center rounded-md text-muted hover:text-info transition-colors cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/60"
                                >
                                  {downloadPaused ? <Play size={13} /> : <Pause size={13} />}
                                </button>
                              }
                            />
                            <TooltipContent>
                              {downloadPaused ? t("models.resume") : t("models.pause")}
                            </TooltipContent>
                          </Tooltip>
                          <Tooltip>
                            <TooltipTrigger
                              render={
                                <button
                                  onClick={() => handleCancelDownload(rec)}
                                  aria-label={t("models.cancelDownload")}
                                  className="flex size-6 items-center justify-center rounded-md text-muted hover:text-danger transition-colors cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/60"
                                >
                                  <X size={13} />
                                </button>
                              }
                            />
                            <TooltipContent>{t("models.cancelDownload")}</TooltipContent>
                          </Tooltip>
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
                    undefined,
                    rec.recommended,
                  );
                })}
              </>
            )}
          </div>

          {downloadingKey && downloadStatus && (
            <div className="px-4 py-2 rounded-lg border border-info/15 bg-info/5 text-info text-[11px] font-mono">
              {downloadStatus}
            </div>
          )}
        </TabsContent>

        <TabsContent value="openai-cloud" className="flex flex-col">
          <div className="flex items-center justify-between gap-4 mb-4">
            <h2 className="m-0 text-base text-white font-medium flex items-center gap-2">
              <Cloud size={16} className="text-muted" /> {t("models.cloudProvider")}
            </h2>
            <Badge
              variant="outline"
              className="rounded-md border-primary/30 bg-primary/5 text-primary font-mono text-[10px]"
            >
              Vercel AI SDK
            </Badge>
          </div>
          <div className="border border-border rounded-xl overflow-hidden bg-secondary">
            <SettingRow
              className="flex-wrap"
              title={t("models.providerLabel")}
              description={t("models.providerDesc")}
            >
              <div className="flex w-72 max-w-full shrink gap-2">
                <Select
                  value={asrProvider}
                  onValueChange={(value) => handleProviderChange(value as CloudProviderId)}
                  items={providerLabels}
                >
                  <SelectTrigger className="min-w-0 flex-1 bg-black">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {cloudProviders.map((provider) => (
                      <SelectItem key={provider.id} value={provider.id}>
                        {provider.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Tooltip>
                  <TooltipTrigger
                    render={
                      <Button
                        type="button"
                        variant="outline"
                        size="icon"
                        onClick={handleOpenProviderDashboard}
                        aria-label={t("models.openProviderDashboard", {
                          provider: activeCloudProvider.name,
                        })}
                      >
                        <ExternalLink size={15} />
                      </Button>
                    }
                  />
                  <TooltipContent>
                    {t("models.openProviderDashboard", {
                      provider: activeCloudProvider.name,
                    })}
                  </TooltipContent>
                </Tooltip>
              </div>
            </SettingRow>

            {activeCloudProvider.requiredSettings.map((name) => {
              const field = PROVIDER_SETTING_KEYS[name];
              if (!field) return null;
              const placeholder =
                name === "region"
                  ? asrProvider === "aws"
                    ? "eu-central-1"
                    : "westeurope"
                  : t(field.placeholder);
              return (
                <SettingRow
                  key={name}
                  className="flex-wrap"
                  title={t(field.label)}
                  description={t(field.description)}
                >
                  <Input
                    id={`byok-${asrProvider}-${name}`}
                    value={providerSettings[name] ?? ""}
                    onChange={(event) =>
                      handleProviderSettingChange(name, event.target.value)
                    }
                    onBlur={() => commitProviderSetting(name)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") event.currentTarget.blur();
                    }}
                    placeholder={placeholder}
                    aria-required="true"
                    autoComplete="off"
                    spellCheck={false}
                    className="w-72 max-w-full shrink bg-black font-mono"
                  />
                </SettingRow>
              );
            })}

            <SettingRow
              className="flex-wrap"
              title={credentialLabel}
              description={
                activeCloudProvider.credentialKind === "serviceAccountJson"
                  ? t("models.serviceAccountJsonDesc")
                  : activeCloudProvider.credentialKind === "secretAccessKey"
                    ? t("models.secretAccessKeyDesc")
                    : t("models.credentialDesc")
              }
            >
              <div className="flex flex-col gap-1.5 w-72 max-w-full shrink">
                <div className="flex gap-2">
                  <Input
                    type={showApiKey ? "text" : "password"}
                    value={keyDraft}
                    onChange={(e) => setKeyDraft(e.target.value)}
                    onBlur={commitProviderKey}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        e.currentTarget.blur();
                      }
                    }}
                    placeholder={
                      hasStoredKey
                        ? t("models.credentialReplace")
                        : t("models.credentialPlaceholder", {
                            credential: credentialLabel,
                            provider: providerLabels[asrProvider] ?? asrProvider,
                          })
                    }
                    aria-label={credentialLabel}
                    autoComplete="off"
                    spellCheck={false}
                    className="flex-1 bg-black font-mono"
                  />
                  <Tooltip>
                    <TooltipTrigger
                      render={
                        <Button
                          type="button"
                          variant="outline"
                          size="icon"
                          onClick={() => setShowApiKey(!showApiKey)}
                          aria-label={showApiKey ? t("models.hideKey") : t("models.showKey")}
                        >
                          {showApiKey ? <EyeOff size={15} /> : <Eye size={15} />}
                        </Button>
                      }
                    />
                    <TooltipContent>
                      {showApiKey ? t("models.hideKey") : t("models.showKey")}
                    </TooltipContent>
                  </Tooltip>
                  {hasStoredKey && (
                    <Tooltip>
                      <TooltipTrigger
                        render={
                          <Button
                            type="button"
                            variant="outline"
                            size="icon"
                            onClick={() => setRemoveKeyOpen(true)}
                            aria-label={t("models.removeKey")}
                            className="text-danger hover:text-danger hover:border-danger/40 hover:bg-danger/5"
                          >
                            <Trash2 size={15} />
                          </Button>
                        }
                      />
                      <TooltipContent>{t("models.removeKey")}</TooltipContent>
                    </Tooltip>
                  )}
                </div>
                {hasStoredKey && !keyDraft && (
                  <span className="inline-flex items-center gap-1.5 text-[11px] text-success">
                    <Check size={12} className="shrink-0" />
                    {t("models.keyStoredHint")}
                  </span>
                )}
              </div>
            </SettingRow>

            <SettingRow
              className="flex-wrap"
              title={t("models.modelLabel")}
              description={t("models.modelDesc")}
            >
              <div className="flex flex-col gap-1 w-72 max-w-full shrink">
                <div className="flex items-center gap-2">
                  <Select
                    value={selectedCloudModel}
                    onValueChange={(value) => handleModelChange(value as string)}
                    disabled={
                      !hasStoredKey ||
                      !providerConfigurationReady ||
                      modelsLoading ||
                      cloudModels.length === 0
                    }
                  >
                    <SelectTrigger
                      className="flex-1 bg-black"
                      aria-busy={modelsLoading}
                      aria-invalid={Boolean(modelsFetchError)}
                      aria-describedby={
                        modelsFetchError
                          ? "cloud-model-error"
                          : modelsLoading
                            ? "cloud-model-loading"
                            : undefined
                      }
                    >
                      <SelectValue>
                        {(value: string | null) =>
                          value
                            ? cloudModels.find((model) => model.id === value)?.name || value
                            : modelsLoading
                              ? t("models.loadingModels")
                              : t("models.modelPlaceholder")
                        }
                      </SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      {cloudModels.map((model) => (
                        <SelectItem key={model.id} value={model.id}>
                          <span className="flex min-w-0 flex-col items-start">
                            <span className="truncate">{model.name}</span>
                            {model.name !== model.id && (
                              <span className="max-w-64 truncate font-mono text-[10px] text-muted">
                                {model.id}
                              </span>
                            )}
                          </span>
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <Tooltip>
                    <TooltipTrigger
                      render={
                        <Button
                          type="button"
                          variant="outline"
                          size="icon"
                          onClick={handleRefreshModels}
                          disabled={
                            modelsLoading || !hasStoredKey || !providerConfigurationReady
                          }
                          aria-label={t("models.refreshModels")}
                        >
                          <RefreshCw
                            size={14}
                            className={modelsLoading ? "animate-spin" : ""}
                          />
                        </Button>
                      }
                    />
                    <TooltipContent>{t("models.refreshModels")}</TooltipContent>
                  </Tooltip>
                </div>
                {!modelsLoading && modelsFetchError && (
                  <span
                    id="cloud-model-error"
                    role="alert"
                    className="text-[11px] leading-snug text-danger"
                    title={modelsFetchError}
                  >
                    {modelsFetchError}
                  </span>
                )}
                {modelsLoading && (
                  <span id="cloud-model-loading" role="status" className="text-[11px] text-muted">
                    {t("models.loadingModels")}
                  </span>
                )}
                {!hasStoredKey && (
                  <span className="text-[11px] text-muted">
                    {t("models.modelNeedsKey")}
                  </span>
                )}
                {hasStoredKey && !providerConfigurationReady && (
                  <span className="text-[11px] text-muted">
                    {t("models.modelNeedsConfiguration")}
                  </span>
                )}
                {!modelsLoading && !modelsFetchError && cloudModels.length > 0 && (
                  <span className="inline-flex items-center gap-1.5 text-[11px] text-success">
                    <Check size={12} className="shrink-0" />
                    {t("models.modelsAvailable", { count: cloudModels.length })}
                  </span>
                )}
              </div>
            </SettingRow>
          </div>
        </TabsContent>
      </Tabs>
      </div>
    </div>
  );
}
