import { ReactNode } from "react";
import { Trans } from "react-i18next";
import type { TFunction } from "i18next";
import { invoke } from "@tauri-apps/api/core";

type ViewId = "usage" | "models" | "transcriptions" | "settings";

export interface PermissionsStatus {
  accessibility: boolean;
  microphone: boolean;
  platform: string;
  is_wayland: boolean;
  desktop_env: string;
}

interface ModelStatus {
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

export function buildSteps(platform: string, t: TFunction): OnboardingStep[] {
  const steps: OnboardingStep[] = [];

  steps.push({
    id: "welcome",
    view: "usage",
    title: t("onboarding.welcome.title"),
    body: <Trans i18nKey="onboarding.welcome.body" components={{ s: <strong /> }} />,
  });

  if (platform === "macos") {
    steps.push({
      id: "permissions",
      view: "settings",
      target: "permissions-section",
      title: t("onboarding.permissions.title"),
      body: (
        <Trans i18nKey="onboarding.permissions.body" components={{ s: <strong /> }} />
      ),
      gate: permissionsGranted,
      gatePollMs: 2000,
    });
  } else if (platform === "linux") {
    steps.push({
      id: "permissions",
      view: "settings",
      target: "shortcuts-section",
      title: t("onboarding.permissionsLinux.title"),
      body: (
        <Trans
          i18nKey="onboarding.permissionsLinux.body"
          components={{ s: <strong /> }}
        />
      ),
    });
  }

  steps.push({
    id: "model",
    view: "models",
    target: "engine-tabs",
    title: t("onboarding.model.title"),
    body: <Trans i18nKey="onboarding.model.body" components={{ s: <strong /> }} />,
    gate: recordingReady,
    gatePollMs: 1500,
  });

  steps.push({
    id: "shortcut",
    view: "settings",
    target: "record-shortcut",
    title: t("onboarding.shortcut.title"),
    body: <Trans i18nKey="onboarding.shortcut.body" components={{ s: <strong /> }} />,
  });

  steps.push({
    id: "language",
    view: "settings",
    target: "language-select",
    title: t("onboarding.language.title"),
    body: <Trans i18nKey="onboarding.language.body" components={{ s: <strong /> }} />,
  });

  steps.push({
    id: "recording-options",
    view: "settings",
    target: "recording-section",
    title: t("onboarding.recordingOptions.title"),
    body: (
      <Trans
        i18nKey="onboarding.recordingOptions.body"
        components={{ s: <strong /> }}
      />
    ),
  });

  steps.push({
    id: "test",
    view: "usage",
    hideMask: true,
    title: t("onboarding.test.title"),
    body: <Trans i18nKey="onboarding.test.body" components={{ s: <strong /> }} />,
    nextLabel: t("onboarding.test.nextLabel"),
    awaitWindowEvent: "transcription-added",
  });

  steps.push({
    id: "done",
    view: "usage",
    target: "sidebar",
    title: t("onboarding.done.title"),
    body: <Trans i18nKey="onboarding.done.body" components={{ s: <strong /> }} />,
    nextLabel: t("common.finish"),
  });

  return steps;
}
