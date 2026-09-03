import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import type { JSONValue, TranscriptionModel } from "ai";

export type CloudProviderId =
  | "openai"
  | "groq"
  | "deepgram"
  | "assemblyai"
  | "speechmatics"
  | "gladia"
  | "revai"
  | "elevenlabs"
  | "together"
  | "fireworks"
  | "deepinfra"
  | "lemonfox"
  | "cloudflare"
  | "replicate"
  | "huggingface"
  | "azure"
  | "google-cloud"
  | "google-ai-studio"
  | "aws";

export type CloudCredentialKind =
  | "apiKey"
  | "apiToken"
  | "subscriptionKey"
  | "serviceAccountJson"
  | "secretAccessKey";

export interface CloudProviderInfo {
  id: CloudProviderId;
  name: string;
  sdkPackage: string;
  sdkVersion: string | null;
  defaultModel: string;
  credentialKind: CloudCredentialKind;
  requiredSettings: string[];
  dashboardUrl: string;
}

export interface CloudModelInfo {
  id: string;
  name: string;
}

interface CloudTranscriptionPlan {
  sessionId: string;
  chunkCount: number;
}

interface CloudChunkResult {
  text?: string;
  error?: string;
}

interface ProxyResponseMetadata {
  status: number;
  statusText: string;
  headers: Array<[string, string]>;
}

interface ProviderTranscriptionResult {
  text: string;
  language?: string;
  durationInSeconds?: number;
  segments?: Array<{
    text: string;
    startSecond: number;
    endSecond: number;
  }>;
}

export type CloudProviderSettings = Record<string, string>;

const providerInfo = (
  id: CloudProviderId,
  name: string,
  sdkPackage: string,
  defaultModel: string,
  credentialKind: CloudCredentialKind = "apiKey",
  requiredSettings: string[] = [],
  dashboardUrl = "",
): CloudProviderInfo => ({
  id,
  name,
  sdkPackage,
  sdkVersion: null,
  defaultModel,
  credentialKind,
  requiredSettings,
  dashboardUrl,
});

const FALLBACK_PROVIDERS: CloudProviderInfo[] = [
  providerInfo(
    "openai",
    "OpenAI",
    "@ai-sdk/openai",
    "gpt-4o-mini-transcribe",
    "apiKey",
    [],
    "https://platform.openai.com/api-keys",
  ),
  providerInfo(
    "groq",
    "Groq",
    "@ai-sdk/groq",
    "whisper-large-v3-turbo",
    "apiKey",
    [],
    "https://console.groq.com/keys",
  ),
  providerInfo(
    "deepgram",
    "Deepgram",
    "@ai-sdk/deepgram",
    "nova-3",
    "apiKey",
    [],
    "https://console.deepgram.com",
  ),
  providerInfo(
    "assemblyai",
    "AssemblyAI",
    "@ai-sdk/assemblyai",
    "universal-3-5-pro",
    "apiKey",
    [],
    "https://www.assemblyai.com/dashboard",
  ),
  providerInfo(
    "speechmatics",
    "Speechmatics",
    "ai",
    "enhanced",
    "apiKey",
    [],
    "https://portal.speechmatics.com",
  ),
  providerInfo(
    "gladia",
    "Gladia",
    "@ai-sdk/gladia",
    "default",
    "apiKey",
    [],
    "https://app.gladia.io",
  ),
  providerInfo(
    "revai",
    "Rev AI",
    "@ai-sdk/revai",
    "machine",
    "apiKey",
    [],
    "https://www.rev.ai/auth/login",
  ),
  providerInfo(
    "elevenlabs",
    "ElevenLabs",
    "@ai-sdk/elevenlabs",
    "scribe_v2",
    "apiKey",
    [],
    "https://elevenlabs.io/app/developers/api-keys",
  ),
  providerInfo(
    "together",
    "Together AI",
    "@ai-sdk/openai",
    "openai/whisper-large-v3",
    "apiKey",
    [],
    "https://api.together.ai/settings/api-keys",
  ),
  providerInfo(
    "fireworks",
    "Fireworks AI",
    "@ai-sdk/openai",
    "whisper-v3-turbo",
    "apiKey",
    [],
    "https://app.fireworks.ai/settings/users/api-keys",
  ),
  providerInfo(
    "deepinfra",
    "DeepInfra",
    "ai",
    "openai/whisper-large-v3",
    "apiKey",
    [],
    "https://deepinfra.com/dash/api_keys",
  ),
  providerInfo(
    "lemonfox",
    "Lemonfox.ai",
    "@ai-sdk/openai",
    "whisper-1",
    "apiKey",
    [],
    "https://www.lemonfox.ai/dashboard",
  ),
  providerInfo(
    "cloudflare",
    "Cloudflare Workers AI",
    "workers-ai-provider",
    "@cf/openai/whisper-large-v3-turbo",
    "apiToken",
    ["accountId"],
    "https://dash.cloudflare.com",
  ),
  providerInfo(
    "replicate",
    "Replicate",
    "ai",
    "openai/whisper",
    "apiToken",
    [],
    "https://replicate.com/account/api-tokens",
  ),
  providerInfo(
    "huggingface",
    "Hugging Face",
    "ai",
    "openai/whisper-large-v3",
    "apiToken",
    [],
    "https://huggingface.co/settings/tokens",
  ),
  providerInfo(
    "azure",
    "Microsoft Azure AI Speech",
    "ai",
    "standard",
    "subscriptionKey",
    ["region"],
    "https://portal.azure.com",
  ),
  providerInfo(
    "google-cloud",
    "Google Cloud Speech-to-Text",
    "ai",
    "latest_long",
    "serviceAccountJson",
    [],
    "https://console.cloud.google.com/apis/credentials",
  ),
  providerInfo(
    "google-ai-studio",
    "Google AI Studio",
    "ai",
    "gemini-3.5-transcribe",
    "apiKey",
    [],
    "https://aistudio.google.com/api-keys",
  ),
  providerInfo(
    "aws",
    "Amazon Transcribe",
    "ai",
    "standard",
    "secretAccessKey",
    ["accessKeyId", "region"],
    "https://console.aws.amazon.com/iam/home#/security_credentials",
  ),
];

const PROVIDER_IDS = new Set(FALLBACK_PROVIDERS.map((provider) => provider.id));
const KEYRING_PLACEHOLDER = "simplevoice-keyring-managed";
const MAX_CONCURRENCY = 3;
const loadOpenAI = () =>
  import("@ai-sdk/openai").then((module) => module.createOpenAI);
const loadGroq = () => import("@ai-sdk/groq").then((module) => module.createGroq);
const loadDeepgram = () =>
  import("@ai-sdk/deepgram").then((module) => module.createDeepgram);
const loadElevenLabs = () =>
  import("@ai-sdk/elevenlabs").then((module) => module.createElevenLabs);
const loadAssemblyAI = () =>
  import("@ai-sdk/assemblyai").then((module) => module.createAssemblyAI);
const loadGladia = () => import("@ai-sdk/gladia").then((module) => module.createGladia);
const loadRevai = () => import("@ai-sdk/revai").then((module) => module.createRevai);
const loadWorkersAI = () =>
  import("workers-ai-provider").then((module) => module.createWorkersAI);

const providerSettingsKey = (provider: CloudProviderId) =>
  `byok_provider_settings:${provider}`;

export function isCloudProviderId(value: string | null): value is CloudProviderId {
  return value !== null && PROVIDER_IDS.has(value as CloudProviderId);
}

export function fallbackCloudProviders(): CloudProviderInfo[] {
  return FALLBACK_PROVIDERS.map((provider) => ({
    ...provider,
    requiredSettings: [...provider.requiredSettings],
  }));
}

export function getCloudProviderSettings(
  provider: CloudProviderId,
): CloudProviderSettings {
  try {
    const stored = JSON.parse(localStorage.getItem(providerSettingsKey(provider)) || "{}");
    if (!stored || typeof stored !== "object" || Array.isArray(stored)) return {};
    return Object.fromEntries(
      Object.entries(stored)
        .filter((entry): entry is [string, string] => typeof entry[1] === "string")
        .map(([key, value]) => [key, value.slice(0, 256)]),
    );
  } catch {
    return {};
  }
}

export function setCloudProviderSetting(
  provider: CloudProviderId,
  name: string,
  value: string,
): CloudProviderSettings {
  const settings = getCloudProviderSettings(provider);
  const normalized = value.trim().slice(0, 256);
  if (normalized) settings[name] = normalized;
  else delete settings[name];
  localStorage.setItem(providerSettingsKey(provider), JSON.stringify(settings));
  return settings;
}

export function hasRequiredCloudProviderSettings(
  provider: CloudProviderInfo,
): boolean {
  const settings = getCloudProviderSettings(provider.id);
  return provider.requiredSettings.every((name) => Boolean(settings[name]?.trim()));
}

export function defaultCloudModel(provider: CloudProviderId): string {
  return FALLBACK_PROVIDERS.find((item) => item.id === provider)!.defaultModel;
}

export async function fetchCloudProviders(): Promise<CloudProviderInfo[]> {
  const providers = await invoke<CloudProviderInfo[]>("list_cloud_providers");
  if (!Array.isArray(providers)) return fallbackCloudProviders();
  const safeProviders = providers.filter((provider) => isCloudProviderId(provider.id));
  return safeProviders.length > 0 ? safeProviders : fallbackCloudProviders();
}

export async function preloadCloudTranscription(provider: CloudProviderId): Promise<void> {
  switch (provider) {
    case "openai":
      await loadOpenAI();
      return;
    case "groq":
      await loadGroq();
      return;
    case "deepgram":
      await loadDeepgram();
      return;
    case "assemblyai":
      await loadAssemblyAI();
      return;
    case "gladia":
      await loadGladia();
      return;
    case "revai":
      await loadRevai();
      return;
    case "elevenlabs":
      await loadElevenLabs();
      return;
    case "together":
    case "fireworks":
    case "lemonfox":
      await loadOpenAI();
      return;
    case "cloudflare":
      await loadWorkersAI();
      return;
    default:
      return;
  }
}

function encodeMetadata(value: unknown): string {
  const bytes = new TextEncoder().encode(JSON.stringify(value));
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

function asUint8Array(value: ArrayBuffer | number[]): Uint8Array {
  return value instanceof ArrayBuffer ? new Uint8Array(value) : new Uint8Array(value);
}

function decodeProxyResponse(value: ArrayBuffer | number[]): Response {
  const envelope = asUint8Array(value);
  if (envelope.byteLength < 4) throw new Error("errors.cloud_response_parse");
  const view = new DataView(envelope.buffer, envelope.byteOffset, envelope.byteLength);
  const metadataLength = view.getUint32(0, false);
  const bodyOffset = 4 + metadataLength;
  if (bodyOffset > envelope.byteLength) throw new Error("errors.cloud_response_parse");
  const metadata = JSON.parse(
    new TextDecoder().decode(envelope.subarray(4, bodyOffset)),
  ) as ProxyResponseMetadata;
  const response = new Response(envelope.slice(bodyOffset), {
    status: metadata.status,
    statusText: metadata.statusText,
    headers: metadata.headers,
  });
  return response;
}

function createKeyringFetch(
  provider: CloudProviderId,
  settings: CloudProviderSettings,
) {
  return async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    if (init?.signal?.aborted) throw new DOMException("Request aborted", "AbortError");
    const request = new Request(input, init);
    const headers = Array.from(request.headers.entries()).filter(
      ([name]) =>
        ![
          "authorization",
          "xi-api-key",
          "x-gladia-key",
          "x-goog-api-key",
          "ocp-apim-subscription-key",
        ].includes(name.toLowerCase()),
    );
    const metadata = encodeMetadata({
      provider,
      method: request.method,
      url: request.url,
      headers,
      settings,
    });
    const body = new Uint8Array(await request.arrayBuffer());
    const result = await invoke<ArrayBuffer | number[]>("byok_http_request", body, {
      headers: { "x-simplevoice-byok-request": metadata },
    });
    if (init?.signal?.aborted) throw new DOMException("Request aborted", "AbortError");
    return decodeProxyResponse(result);
  };
}

async function createTranscriptionModel(
  provider: CloudProviderId,
  modelId: string,
  language?: string,
): Promise<TranscriptionModel> {
  const settings = getCloudProviderSettings(provider);
  const providerFetch = createKeyringFetch(provider, settings);
  switch (provider) {
    case "openai": {
      const createOpenAI = await loadOpenAI();
      return createOpenAI({ apiKey: KEYRING_PLACEHOLDER, fetch: providerFetch }).transcription(
        modelId,
      );
    }
    case "groq": {
      const createGroq = await loadGroq();
      return createGroq({ apiKey: KEYRING_PLACEHOLDER, fetch: providerFetch }).transcription(
        modelId,
      );
    }
    case "deepgram": {
      const createDeepgram = await loadDeepgram();
      return createDeepgram({ apiKey: KEYRING_PLACEHOLDER, fetch: providerFetch }).transcription(
        modelId,
      );
    }
    case "assemblyai": {
      const createAssemblyAI = await loadAssemblyAI();
      return createAssemblyAI({
        apiKey: KEYRING_PLACEHOLDER,
        fetch: providerFetch,
      }).transcription(modelId);
    }
    case "gladia": {
      const createGladia = await loadGladia();
      return createGladia({
        apiKey: KEYRING_PLACEHOLDER,
        fetch: providerFetch,
      }).transcription();
    }
    case "revai": {
      if (!language) {
        return createBridgeTranscriptionModel(provider, modelId, language, settings);
      }
      const createRevai = await loadRevai();
      return createRevai({
        apiKey: KEYRING_PLACEHOLDER,
        fetch: providerFetch,
      }).transcription(modelId as "machine" | "low_cost" | "fusion");
    }
    case "elevenlabs": {
      const createElevenLabs = await loadElevenLabs();
      return createElevenLabs({
        apiKey: KEYRING_PLACEHOLDER,
        fetch: providerFetch,
      }).transcription(modelId);
    }
    case "together": {
      const createOpenAI = await loadOpenAI();
      return createOpenAI({
        name: "together",
        baseURL: "https://api.together.ai/v1",
        apiKey: KEYRING_PLACEHOLDER,
        fetch: providerFetch,
      }).transcription(modelId);
    }
    case "fireworks": {
      const createOpenAI = await loadOpenAI();
      return createOpenAI({
        name: "fireworks",
        baseURL: "https://audio-turbo.us-virginia-1.direct.fireworks.ai/v1",
        apiKey: KEYRING_PLACEHOLDER,
        fetch: providerFetch,
      }).transcription(modelId);
    }
    case "lemonfox": {
      const createOpenAI = await loadOpenAI();
      return createOpenAI({
        name: "lemonfox",
        baseURL: "https://api.lemonfox.ai/v1",
        apiKey: KEYRING_PLACEHOLDER,
        fetch: providerFetch,
      }).transcription(modelId);
    }
    case "cloudflare": {
      if (modelId === "@cf/deepgram/nova-3") {
        return createBridgeTranscriptionModel(provider, modelId, language, settings);
      }
      const accountId = settings.accountId;
      if (!accountId) throw new Error("errors.provider_setting_missing");
      const createWorkersAI = await loadWorkersAI();
      return createWorkersAI({
        accountId,
        apiKey: KEYRING_PLACEHOLDER,
        fetch: providerFetch,
      }).transcription(modelId, language ? { language } : {});
    }
    default:
      return createBridgeTranscriptionModel(provider, modelId, language, settings);
  }
}

function decodeBase64(value: string): Uint8Array {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

function createBridgeTranscriptionModel(
  provider: CloudProviderId,
  modelId: string,
  language: string | undefined,
  settings: CloudProviderSettings,
): TranscriptionModel {
  return {
    specificationVersion: "v4",
    provider: `${provider}.transcription`,
    modelId,
    async doGenerate(options) {
      if (options.abortSignal?.aborted) {
        throw new DOMException("Request aborted", "AbortError");
      }
      const audio =
        typeof options.audio === "string" ? decodeBase64(options.audio) : options.audio;
      const metadata = encodeMetadata({
        provider,
        modelId,
        language,
        mediaType: options.mediaType,
        settings,
      });
      const raw = await invoke<ArrayBuffer | number[]>("byok_transcribe", audio, {
        headers: { "x-simplevoice-byok-transcription": metadata },
      });
      if (options.abortSignal?.aborted) {
        throw new DOMException("Request aborted", "AbortError");
      }
      const result = JSON.parse(
        new TextDecoder().decode(asUint8Array(raw)),
      ) as ProviderTranscriptionResult;
      return {
        text: result.text,
        segments: result.segments ?? [],
        language: result.language,
        durationInSeconds: result.durationInSeconds,
        warnings: [],
        response: {
          timestamp: new Date(),
          modelId,
          body: result as unknown as JSONValue,
        },
      };
    },
  };
}

function providerOptions(
  provider: CloudProviderId,
  language?: string,
): Record<string, Record<string, JSONValue>> {
  switch (provider) {
    case "openai":
    case "together":
    case "fireworks":
    case "lemonfox":
      return { openai: language ? { language } : {} };
    case "groq":
      return { groq: language ? { language } : {} };
    case "deepgram":
      return {
        deepgram: {
          ...(language ? { language } : { detectLanguage: true }),
          smartFormat: true,
          punctuate: true,
          diarize: false,
        },
      };
    case "assemblyai":
      return {
        assemblyai: language ? { languageCode: language } : { languageDetection: true },
      };
    case "gladia":
      return {
        gladia: language ? { language } : { detectLanguage: true },
      };
    case "revai":
      return { revai: language ? { language } : {} };
    case "elevenlabs":
      return {
        elevenlabs: {
          ...(language ? { languageCode: language } : {}),
          tagAudioEvents: false,
          diarize: false,
          timestampsGranularity: "none" as const,
        },
      };
    default:
      return {};
  }
}

function errorMessage(error: unknown): string {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : String(error ?? "Unknown provider error");
  return message.slice(0, 600);
}

async function transcribeChunk(
  provider: CloudProviderId,
  model: TranscriptionModel,
  audio: Uint8Array,
  language?: string,
): Promise<string> {
  const { transcribe } = await import("ai");
  const result = await transcribe({
    model,
    audio,
    providerOptions: providerOptions(provider, language),
    maxRetries: 2,
  });
  return result.text.trim();
}

export async function transcribeCloudRecording(options: {
  provider: CloudProviderId;
  modelId: string;
  language?: string;
}): Promise<string> {
  const { provider, modelId, language } = options;
  if (!modelId.trim()) throw new Error("errors.no_transcription_model_selected");
  const plan = await invoke<CloudTranscriptionPlan>("prepare_cloud_transcription", {
    provider,
  });
  let completed = false;
  try {
    const model = await createTranscriptionModel(provider, modelId, language);
    const results: CloudChunkResult[] = Array.from({ length: plan.chunkCount }, () => ({}));
    let nextIndex = 0;
    let finishedCount = 0;
    let firstFailedIndex = Number.POSITIVE_INFINITY;

    const worker = async () => {
      while (true) {
        const index = nextIndex++;
        if (index >= plan.chunkCount || index > firstFailedIndex) return;
        try {
          const rawAudio = await invoke<ArrayBuffer | number[]>(
            "get_cloud_transcription_chunk",
            { sessionId: plan.sessionId, index },
          );
          const text = await transcribeChunk(
            provider,
            model,
            asUint8Array(rawAudio),
            language,
          );
          results[index] = { text };
        } catch (error) {
          firstFailedIndex = Math.min(firstFailedIndex, index);
          results[index] = { error: errorMessage(error) };
        } finally {
          finishedCount += 1;
          if (plan.chunkCount > 1) {
            await emit("transcription-progress", {
              done: finishedCount,
              total: plan.chunkCount,
            });
          }
        }
      }
    };

    await Promise.all(
      Array.from(
        { length: Math.min(MAX_CONCURRENCY, Math.max(plan.chunkCount, 1)) },
        worker,
      ),
    );
    for (let index = 0; index < results.length; index += 1) {
      if (!results[index].text && !results[index].error) {
        results[index] = { error: "Skipped after an earlier chunk failed" };
      }
    }
    const text = await invoke<string>("complete_cloud_transcription", {
      sessionId: plan.sessionId,
      results,
      language: language || null,
    });
    completed = true;
    return text;
  } finally {
    if (!completed) {
      await invoke("cancel_cloud_transcription", { sessionId: plan.sessionId }).catch(() => {});
    }
  }
}
