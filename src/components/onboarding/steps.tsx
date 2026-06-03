import { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";

export type ViewId = "usage" | "models" | "transcriptions" | "settings";

export interface PermissionsStatus {
  accessibility: boolean;
  microphone: boolean;
  platform: string;
  is_wayland: boolean;
  desktop_env: string;
}

export interface ModelStatus {
  active: string | null;
  loading: string | null;
}

export interface OnboardingStep {
  id: string;
  view: ViewId;
  target?: string;
  title: string;
  body: ReactNode;
  nextLabel?: string;
  hideMask?: boolean;
  gate?: () => Promise<boolean>;
  gatePollMs?: number;
  awaitWindowEvent?: string;
}

async function recordingReady(): Promise<boolean> {
  const engine = localStorage.getItem("asr_engine") || "local";
  if (engine === "local") {
    const status = await invoke<ModelStatus>("get_model_status");
    return status.active != null;
  }
  const provider = localStorage.getItem("asr_provider") || "openai";
  return invoke<boolean>("has_secure_api_key", { provider });
}

async function permissionsGranted(): Promise<boolean> {
  const s = await invoke<PermissionsStatus>("check_permissions_status");
  return s.accessibility && s.microphone;
}

export function buildSteps(platform: string): OnboardingStep[] {
  const steps: OnboardingStep[] = [];

  steps.push({
    id: "welcome",
    view: "usage",
    title: "Welcome to Simplevoice",
    body: (
      <>
        Simplevoice turns your voice into text, fully local and private. The
        flow is simple: press your shortcut, speak, and the transcription is
        pasted straight into whatever app you are using. This quick tour gets
        you set up in a minute.
      </>
    ),
  });

  if (platform === "macos") {
    steps.push({
      id: "permissions",
      view: "settings",
      target: "permissions-section",
      title: "Grant system permissions",
      body: (
        <>
          Simplevoice needs the <strong>Microphone</strong> to record and{" "}
          <strong>Accessibility</strong> to paste text for you. Use the Grant
          buttons here. The tour continues once both are granted.
        </>
      ),
      gate: permissionsGranted,
      gatePollMs: 2000,
    });
  } else if (platform === "linux") {
    steps.push({
      id: "permissions",
      view: "settings",
      target: "shortcuts-section",
      title: "Global hotkeys on Linux",
      body: (
        <>
          On Linux the global shortcut is captured directly from your keyboard.
          If it ever does nothing, add your user to the <strong>input</strong>{" "}
          group and log back in. The status box here tells you whether it is
          active.
        </>
      ),
    });
  }

  steps.push({
    id: "model",
    view: "models",
    target: "engine-tabs",
    title: "Pick how you transcribe",
    body: (
      <>
        Choose <strong>Local</strong> to run a model on your machine: click{" "}
        <strong>Get</strong> to download one, then <strong>Load</strong> it. Or
        choose <strong>Cloud (BYOK)</strong> and paste your own API key. The
        tour continues once a model is ready.
      </>
    ),
    gate: recordingReady,
    gatePollMs: 1500,
  });

  steps.push({
    id: "shortcut",
    view: "settings",
    target: "record-shortcut",
    title: "Your recording shortcut",
    body: (
      <>
        This global hotkey starts and stops recording from anywhere. A default
        is already set, so you can click it any time to record a new
        combination.
      </>
    ),
  });

  steps.push({
    id: "language",
    view: "settings",
    target: "language-select",
    title: "Transcription language",
    body: (
      <>
        Leave this on <strong>Auto-detect</strong> for multilingual use, or pick
        a specific language to force the output and improve accuracy.
      </>
    ),
  });

  steps.push({
    id: "recording-options",
    view: "settings",
    target: "recording-section",
    title: "Recording options",
    body: (
      <>
        <strong>Voice Activity Detection</strong> stops recording automatically
        when you go quiet. You can also toggle sound cues and pause system audio
        while recording. Turn on whatever fits your workflow.
      </>
    ),
  });

  steps.push({
    id: "test",
    view: "usage",
    hideMask: true,
    title: "Try it now",
    body: (
      <>
        Press your recording shortcut and say a sentence. Watch it get
        transcribed and pasted. This step completes itself once you do.
      </>
    ),
    nextLabel: "I'll do this later",
    awaitWindowEvent: "transcription-added",
  });

  steps.push({
    id: "done",
    view: "usage",
    target: "sidebar",
    title: "You're all set",
    body: (
      <>
        That's it. Use the sidebar to see your <strong>Usage</strong>, manage{" "}
        <strong>Models</strong>, browse past <strong>Transcriptions</strong>,
        and fine-tune everything under <strong>Settings</strong>. Enjoy
        Simplevoice.
      </>
    ),
    nextLabel: "Finish",
  });

  return steps;
}
