import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import type { JSONValue, TranscriptionModel } from "ai";

export type CloudProviderId = "openai" | "groq" | "deepgram" | "elevenlabs";

export interface CloudProviderInfo {
  id: CloudProviderId;
  name: string;
  sdkPackage: string;
  sdkVersion: string | null;
  defaultModel: string;
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

const FALLBACK_PROVIDERS: CloudProviderInfo[] = [
  {
    id: "openai",
    name: "OpenAI",
    sdkPackage: "@ai-sdk/openai",
    sdkVersion: null,
    defaultModel: "gpt-4o-mini-transcribe",
  },
  {
    id: "groq",
    name: "Groq",
    sdkPackage: "@ai-sdk/groq",
    sdkVersion: null,
    defaultModel: "whisper-large-v3-turbo",
  },
  {
    id: "deepgram",
    name: "Deepgram",
    sdkPackage: "@ai-sdk/deepgram",
    sdkVersion: null,
    defaultModel: "nova-3",
  },
  {
    id: "elevenlabs",
    name: "ElevenLabs",
    sdkPackage: "@ai-sdk/elevenlabs",
    sdkVersion: null,
    defaultModel: "scribe_v2",
  },
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

export function isCloudProviderId(value: string | null): value is CloudProviderId {
  return value !== null && PROVIDER_IDS.has(value as CloudProviderId);
}

export function fallbackCloudProviders(): CloudProviderInfo[] {
  return FALLBACK_PROVIDERS.map((provider) => ({ ...provider }));
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
    case "elevenlabs":
      await loadElevenLabs();
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

function createKeyringFetch(provider: CloudProviderId) {
  return async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    if (init?.signal?.aborted) throw new DOMException("Request aborted", "AbortError");
    const request = new Request(input, init);
    const headers = Array.from(request.headers.entries()).filter(
      ([name]) => !["authorization", "xi-api-key"].includes(name.toLowerCase()),
    );
    const metadata = encodeMetadata({
      provider,
      method: request.method,
      url: request.url,
      headers,
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
): Promise<TranscriptionModel> {
  const providerFetch = createKeyringFetch(provider);
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
    case "elevenlabs": {
      const createElevenLabs = await loadElevenLabs();
      return createElevenLabs({
        apiKey: KEYRING_PLACEHOLDER,
        fetch: providerFetch,
      }).transcription(modelId);
    }
  }
}

function providerOptions(
  provider: CloudProviderId,
  language?: string,
): Record<string, Record<string, JSONValue>> {
  switch (provider) {
    case "openai":
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
    case "elevenlabs":
      return {
        elevenlabs: {
          ...(language ? { languageCode: language } : {}),
          tagAudioEvents: false,
          diarize: false,
          timestampsGranularity: "none" as const,
        },
      };
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
  const plan = await invoke<CloudTranscriptionPlan>("prepare_cloud_transcription");
  let completed = false;
  try {
    const model = await createTranscriptionModel(provider, modelId);
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
