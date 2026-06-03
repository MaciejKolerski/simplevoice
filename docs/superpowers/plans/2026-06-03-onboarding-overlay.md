# Onboarding Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a first-run, spotlight-overlay guided tour that highlights real UI elements and walks a new user through getting Simplevoice working.

**Architecture:** A dedicated `OnboardingProvider` detects first run (reading `config.json` directly), builds a platform-specific step list, drives the active view via the existing `navigate-to-view` event, and gates "Next" on key steps. An `OnboardingOverlay` renders a four-panel dim mask with an interactive "hole" over the highlighted element (found via `data-tour` attributes) plus a tooltip card. No third-party tour library; no backend changes.

**Tech Stack:** React 19 + TypeScript (strict), Tailwind v4, existing `Button` UI primitive, Tauri `invoke` for `check_permissions_status` / `get_model_status` / `has_secure_api_key` / `load_config` / `save_config`.

---

## Testing approach (read first)

This project has **no JS test runner** and the approved spec explicitly decided against adding one (scope creep). Per the instruction-priority rule (user/spec decisions override the skill's TDD default), each task is verified by:

- **`pnpm lint`** = `tsc --noEmit --strict` — the type-check gate. Expected on success: **no output, exit code 0**.
- A final **manual run-through** task (`pnpm tauri dev` with the first-run flag cleared).

All onboarding logic that benefits from isolation lives in the pure `buildSteps(platform)` function, so it can be unit-tested later if a runner is ever added.

Commit after every task. Do not bundle unrelated working-tree changes — only stage the files listed in each task.

## File structure

| File | Responsibility | Task |
|------|----------------|------|
| `src/components/onboarding/steps.tsx` | Types + pure `buildSteps(platform)` returning the step array, including gate predicates. | 1 |
| `src/components/onboarding/useSpotlight.ts` | Hook tracking the highlighted element's rect (resize/async/recompute). | 2 |
| `src/components/onboarding/OnboardingProvider.tsx` | Context + state, first-run detection, view driving, gate polling, completion persistence. | 3 |
| `src/components/onboarding/OnboardingOverlay.tsx` | Visual layer: mask, ring, tooltip card, controls. | 4 |
| `src/components/layout/Sidebar.tsx` (modify) | `data-tour="sidebar"`. | 5 |
| `src/views/ModelsView.tsx` (modify) | `data-tour="engine-tabs"`. | 5 |
| `src/views/SettingsView.tsx` (modify) | `data-tour` on permissions/shortcuts/recording sections, record-shortcut button, language field. | 5 |
| `src/main.tsx` (modify) | Wrap `App` in `OnboardingProvider` (main window only). | 6 |
| `src/App.tsx` (modify) | Render `<OnboardingOverlay />`. | 6 |

> Note on persistence: the spec described reading the flag "through ConfigContext". This plan instead reads and writes `config.json` directly via `invoke("load_config")` / `invoke("save_config")` inside the provider. This is a deliberate robustness refinement — it avoids a stale-state clobber (ConfigContext holds its own in-memory copy that other backend `set_*` commands can diverge from) and removes the provider's dependency on `ConfigContext`. Still the same file and key (`onboarding_completed`).

---

## Task 1: Step definitions (`steps.tsx`)

**Files:**
- Create: `src/components/onboarding/steps.tsx`

- [ ] **Step 1: Create the file with types and the step builder**

```tsx
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
```

- [ ] **Step 2: Type-check**

Run: `pnpm lint`
Expected: no output, exit code 0.

- [ ] **Step 3: Commit**

```bash
git add src/components/onboarding/steps.tsx
git commit -m "feat(onboarding): add step definitions and gate predicates"
```

---

## Task 2: Spotlight hook (`useSpotlight.ts`)

**Files:**
- Create: `src/components/onboarding/useSpotlight.ts`

- [ ] **Step 1: Create the hook**

```ts
import { useEffect, useState } from "react";

export interface SpotlightRect {
  top: number;
  left: number;
  width: number;
  height: number;
}

export function useSpotlight(
  target: string | undefined,
  active: boolean,
  stepIndex: number,
): SpotlightRect | null {
  const [rect, setRect] = useState<SpotlightRect | null>(null);

  useEffect(() => {
    if (!active || !target) {
      setRect(null);
      return;
    }

    let scrolled = false;

    const locate = () => {
      const el = document.querySelector<HTMLElement>(
        `[data-tour="${target}"]`,
      );
      if (!el) {
        setRect(null);
        return;
      }
      if (!scrolled) {
        scrolled = true;
        el.scrollIntoView({ block: "center", behavior: "smooth" });
      }
      const r = el.getBoundingClientRect();
      setRect({ top: r.top, left: r.left, width: r.width, height: r.height });
    };

    locate();
    const interval = window.setInterval(locate, 250);
    window.addEventListener("resize", locate);

    return () => {
      window.clearInterval(interval);
      window.removeEventListener("resize", locate);
    };
  }, [target, active, stepIndex]);

  return rect;
}
```

- [ ] **Step 2: Type-check**

Run: `pnpm lint`
Expected: no output, exit code 0.

- [ ] **Step 3: Commit**

```bash
git add src/components/onboarding/useSpotlight.ts
git commit -m "feat(onboarding): add spotlight position hook"
```

---

## Task 3: Provider (`OnboardingProvider.tsx`)

**Files:**
- Create: `src/components/onboarding/OnboardingProvider.tsx`

- [ ] **Step 1: Create the provider**

```tsx
import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { buildSteps, OnboardingStep, PermissionsStatus } from "./steps";

interface OnboardingContextValue {
  active: boolean;
  step: OnboardingStep | null;
  index: number;
  total: number;
  gateReady: boolean;
  next: () => void;
  back: () => void;
  skip: () => void;
}

const OnboardingContext = createContext<OnboardingContextValue | undefined>(
  undefined,
);

export function OnboardingProvider({ children }: { children: ReactNode }) {
  const [steps, setSteps] = useState<OnboardingStep[]>([]);
  const [index, setIndex] = useState(0);
  const [active, setActive] = useState(false);
  const [gateReady, setGateReady] = useState(true);
  const startedRef = useRef(false);

  const step = active && steps[index] ? steps[index] : null;

  useEffect(() => {
    if (startedRef.current) return;
    startedRef.current = true;

    const detect = async () => {
      try {
        const [status, cfgStr] = await Promise.all([
          invoke<PermissionsStatus>("check_permissions_status"),
          invoke<string>("load_config"),
        ]);
        const cfg = JSON.parse(cfgStr || "{}");
        if (!cfg.onboarding_completed) {
          setSteps(buildSteps(status.platform));
          setIndex(0);
          setActive(true);
        }
      } catch (err) {
        console.error("Onboarding: failed to detect first run:", err);
      }
    };
    detect();
  }, []);

  const finish = () => {
    setActive(false);
    (async () => {
      try {
        const cur = JSON.parse((await invoke<string>("load_config")) || "{}");
        cur.onboarding_completed = true;
        await invoke("save_config", { config: JSON.stringify(cur) });
      } catch (err) {
        console.error("Onboarding: failed to persist completion:", err);
      }
    })();
  };

  const next = () => {
    if (index >= steps.length - 1) {
      finish();
    } else {
      setIndex((i) => i + 1);
    }
  };

  const back = () => setIndex((i) => Math.max(0, i - 1));
  const skip = () => finish();

  useEffect(() => {
    if (active && step) {
      window.dispatchEvent(
        new CustomEvent("navigate-to-view", { detail: step.view }),
      );
    }
  }, [index, active]);

  useEffect(() => {
    if (!active || !step?.gate) {
      setGateReady(true);
      return;
    }
    setGateReady(false);
    let cancelled = false;
    const check = async () => {
      try {
        const ok = await step.gate!();
        if (!cancelled) setGateReady(ok);
      } catch (err) {
        console.error("Onboarding: gate check failed:", err);
      }
    };
    check();
    const id = window.setInterval(check, step.gatePollMs ?? 1500);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [index, active]);

  useEffect(() => {
    if (!active || !step?.awaitWindowEvent) return;
    const eventName = step.awaitWindowEvent;
    const handler = () => next();
    window.addEventListener(eventName, handler);
    return () => window.removeEventListener(eventName, handler);
  }, [index, active]);

  return (
    <OnboardingContext.Provider
      value={{
        active,
        step,
        index,
        total: steps.length,
        gateReady,
        next,
        back,
        skip,
      }}
    >
      {children}
    </OnboardingContext.Provider>
  );
}

export function useOnboarding() {
  const ctx = useContext(OnboardingContext);
  if (ctx === undefined) {
    throw new Error("useOnboarding must be used within an OnboardingProvider");
  }
  return ctx;
}
```

> The `index`/`active`-only dependency arrays are intentional: `step` is derived from `index`, and each effect re-runs when the step changes. `tsc` does not enforce React hook dependency completeness, so this type-checks cleanly.

- [ ] **Step 2: Type-check**

Run: `pnpm lint`
Expected: no output, exit code 0.

- [ ] **Step 3: Commit**

```bash
git add src/components/onboarding/OnboardingProvider.tsx
git commit -m "feat(onboarding): add provider with first-run detection and gating"
```

---

## Task 4: Overlay (`OnboardingOverlay.tsx`)

**Files:**
- Create: `src/components/onboarding/OnboardingOverlay.tsx`

- [ ] **Step 1: Create the overlay component**

```tsx
import { CSSProperties } from "react";
import { useOnboarding } from "./OnboardingProvider";
import { useSpotlight } from "./useSpotlight";
import { Button } from "@/components/ui/button";

const CARD_WIDTH = 340;
const GAP = 14;

export function OnboardingOverlay() {
  const { active, step, index, total, gateReady, next, back, skip } =
    useOnboarding();
  const rect = useSpotlight(step?.target, active, index);

  if (!active || !step) return null;

  const isLast = index === total - 1;
  const nextLabel = step.nextLabel ?? (isLast ? "Finish" : "Next");
  const nextDisabled = !gateReady;

  const card = (
    <div className="pointer-events-auto w-[340px] max-w-[calc(100vw-32px)] rounded-2xl border border-border bg-popover/95 p-5 shadow-[0_24px_64px_-16px_rgba(0,0,0,0.85)] backdrop-blur-md">
      <div className="mb-2 font-mono text-[10px] uppercase tracking-[0.2em] text-muted">
        Step {index + 1} / {total}
      </div>
      <h2 className="mb-2 text-lg font-medium tracking-tight text-white">
        {step.title}
      </h2>
      <p className="mb-5 text-sm leading-normal text-muted">{step.body}</p>
      <div className="flex items-center justify-between gap-2">
        <button
          onClick={skip}
          className="cursor-pointer bg-transparent text-xs text-muted-foreground transition-colors hover:text-white"
        >
          Skip tour
        </button>
        <div className="flex gap-2">
          {index > 0 && (
            <Button variant="outline" size="sm" onClick={back}>
              Back
            </Button>
          )}
          <Button size="sm" onClick={next} disabled={nextDisabled}>
            {nextDisabled ? "Waiting…" : nextLabel}
          </Button>
        </div>
      </div>
    </div>
  );

  if (step.hideMask) {
    return (
      <div className="pointer-events-none fixed inset-0 z-40 flex items-start justify-center pt-8">
        {card}
      </div>
    );
  }

  if (!rect) {
    return (
      <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/70 backdrop-blur-sm">
        {card}
      </div>
    );
  }

  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const placeBelow = vh - (rect.top + rect.height) > 240;
  const cardLeft = Math.min(Math.max(rect.left, 16), vw - CARD_WIDTH - 16);

  const cardPos: CSSProperties = placeBelow
    ? { top: rect.top + rect.height + GAP, left: cardLeft }
    : { bottom: vh - rect.top + GAP, left: cardLeft };

  const maskClass =
    "pointer-events-auto absolute bg-black/60 backdrop-blur-[1px]";

  return (
    <div className="fixed inset-0 z-40">
      <div
        className={maskClass}
        style={{ top: 0, left: 0, width: vw, height: rect.top }}
      />
      <div
        className={maskClass}
        style={{
          top: rect.top + rect.height,
          left: 0,
          width: vw,
          height: vh - (rect.top + rect.height),
        }}
      />
      <div
        className={maskClass}
        style={{ top: rect.top, left: 0, width: rect.left, height: rect.height }}
      />
      <div
        className={maskClass}
        style={{
          top: rect.top,
          left: rect.left + rect.width,
          width: vw - (rect.left + rect.width),
          height: rect.height,
        }}
      />
      <div
        className="pointer-events-none absolute rounded-xl border-2 border-white/70 shadow-[0_0_0_4px_rgba(255,255,255,0.15)]"
        style={{
          top: rect.top - 4,
          left: rect.left - 4,
          width: rect.width + 8,
          height: rect.height + 8,
        }}
      />
      <div className="pointer-events-none absolute" style={cardPos}>
        {card}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: Type-check**

Run: `pnpm lint`
Expected: no output, exit code 0.

- [ ] **Step 3: Commit**

```bash
git add src/components/onboarding/OnboardingOverlay.tsx
git commit -m "feat(onboarding): add spotlight overlay component"
```

---

## Task 5: Add `data-tour` anchors to existing views

**Files:**
- Modify: `src/components/layout/Sidebar.tsx`
- Modify: `src/views/ModelsView.tsx`
- Modify: `src/views/SettingsView.tsx`

- [ ] **Step 1: Sidebar — tag the `<aside>`**

In `src/components/layout/Sidebar.tsx`, replace:

```tsx
    <aside className={clsx("sidebar", collapsed && "collapsed")}>
```

with:

```tsx
    <aside data-tour="sidebar" className={clsx("sidebar", collapsed && "collapsed")}>
```

- [ ] **Step 2: ModelsView — wrap the `<Tabs>` in a tagged div**

In `src/views/ModelsView.tsx`, replace:

```tsx
      <Tabs
        value={asrEngine}
        onValueChange={(v) => handleSelectEngine(v as "local" | "openai-cloud")}
        className="w-full"
      >
```

with:

```tsx
      <div data-tour="engine-tabs" className="w-full">
      <Tabs
        value={asrEngine}
        onValueChange={(v) => handleSelectEngine(v as "local" | "openai-cloud")}
        className="w-full"
      >
```

Then find the matching closing tag for that `Tabs` (the `</Tabs>` near the end of the returned JSX) and replace:

```tsx
      </Tabs>
    </div>
  );
}
```

with:

```tsx
      </Tabs>
      </div>
    </div>
  );
}
```

(The `<Tabs>` is the last element before the component's outermost `</div>`; this wraps it without changing layout, since the new div carries the same `w-full`.)

- [ ] **Step 3: SettingsView — tag the language field**

In `src/views/SettingsView.tsx`, replace:

```tsx
          <div className="flex flex-col p-5 border-b border-border last:border-b-0">
            <Label className="mb-3">Transcription Language</Label>
```

with:

```tsx
          <div data-tour="language-select" className="flex flex-col p-5 border-b border-border last:border-b-0">
            <Label className="mb-3">Transcription Language</Label>
```

- [ ] **Step 4: SettingsView — tag the recording section**

Replace:

```tsx
        {/* GROUP: Recording & Feedback */}
        <section>
```

with:

```tsx
        {/* GROUP: Recording & Feedback */}
        <section data-tour="recording-section">
```

- [ ] **Step 5: SettingsView — tag the keyboard-shortcuts section**

Replace:

```tsx
        {/* GROUP: Keyboard Shortcuts */}
        <section>
```

with:

```tsx
        {/* GROUP: Keyboard Shortcuts */}
        <section data-tour="shortcuts-section">
```

- [ ] **Step 6: SettingsView — tag the record-shortcut button**

Replace:

```tsx
            <button
              onClick={() => startRecordingShortcut("record")}
              className="font-mono text-sm px-3.5 py-1.5 bg-surface-active rounded-md border border-border text-foreground min-w-[150px] text-center hover:border-border-hover hover:bg-surface-hover active:scale-[0.985] transition-all select-none"
              title="Click to change shortcut"
            >
```

with:

```tsx
            <button
              data-tour="record-shortcut"
              onClick={() => startRecordingShortcut("record")}
              className="font-mono text-sm px-3.5 py-1.5 bg-surface-active rounded-md border border-border text-foreground min-w-[150px] text-center hover:border-border-hover hover:bg-surface-hover active:scale-[0.985] transition-all select-none"
              title="Click to change shortcut"
            >
```

- [ ] **Step 7: SettingsView — tag the macOS permissions section**

Replace:

```tsx
        {platform === "macos" && (
          <section>
            <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
              <Shield size={16} className="text-muted" /> System Permissions
```

with:

```tsx
        {platform === "macos" && (
          <section data-tour="permissions-section">
            <h2 className="mt-0 mb-4 text-base text-white font-medium flex items-center gap-2">
              <Shield size={16} className="text-muted" /> System Permissions
```

- [ ] **Step 8: Type-check**

Run: `pnpm lint`
Expected: no output, exit code 0.

- [ ] **Step 9: Commit**

```bash
git add src/components/layout/Sidebar.tsx src/views/ModelsView.tsx src/views/SettingsView.tsx
git commit -m "feat(onboarding): add data-tour anchors to existing views"
```

---

## Task 6: Wire the provider and overlay

**Files:**
- Modify: `src/main.tsx`
- Modify: `src/App.tsx`

- [ ] **Step 1: Import the provider in `main.tsx`**

In `src/main.tsx`, after the existing import of `Toaster` (the line `import { Toaster } from "@/components/ui/sonner";`), add:

```tsx
import { OnboardingProvider } from "./components/onboarding/OnboardingProvider";
```

- [ ] **Step 2: Wrap `App` with the provider (main-window branch only)**

In `src/main.tsx`, replace:

```tsx
  return (
    <ConfigProvider>
      <TooltipProvider delay={300}>
        <App />
        <Toaster />
      </TooltipProvider>
    </ConfigProvider>
  );
```

with:

```tsx
  return (
    <ConfigProvider>
      <TooltipProvider delay={300}>
        <OnboardingProvider>
          <App />
        </OnboardingProvider>
        <Toaster />
      </TooltipProvider>
    </ConfigProvider>
  );
```

(The `recording_window` branch above it is left untouched, so the tour never runs in the recording window.)

- [ ] **Step 3: Import the overlay in `App.tsx`**

In `src/App.tsx`, after the existing import of `Updater` (the line `import { Updater } from "./components/Updater";`), add:

```tsx
import { OnboardingOverlay } from "./components/onboarding/OnboardingOverlay";
```

- [ ] **Step 4: Render the overlay**

In `src/App.tsx`, replace:

```tsx
        <Updater />
      </div>
  );
}
```

with:

```tsx
        <Updater />
        <OnboardingOverlay />
      </div>
  );
}
```

- [ ] **Step 5: Type-check**

Run: `pnpm lint`
Expected: no output, exit code 0.

- [ ] **Step 6: Commit**

```bash
git add src/main.tsx src/App.tsx
git commit -m "feat(onboarding): mount provider and overlay in the main window"
```

---

## Task 7: Manual verification

No code changes. This task confirms the tour behaves correctly on the real window.

**Find the config file** (macOS, bundle id `com.woro.simplevoice`):

```
~/Library/Application Support/com.woro.simplevoice/config.json
```

(Linux: `~/.local/share/com.woro.simplevoice/config.json`. Windows: `%APPDATA%\com.woro.simplevoice\config.json`.)

- [ ] **Step 1: Force first-run state**

Edit `config.json` and remove the `"onboarding_completed"` key (or set it to `false`). If the file does not exist yet, that already counts as first run.

- [ ] **Step 2: Launch the app**

Run: `pnpm tauri dev`
Expected: the main window opens and the centered "Welcome to Simplevoice" card (Step 1 / 8) appears over a dimmed background.

- [ ] **Step 3: Walk the happy path**

Verify, step by step:
- Clicking **Next** advances and the app switches to the correct view for each step (Models, Settings, Usage).
- On a step with a target, the real element is highlighted with a ring and a "hole" in the dim, and that element stays **clickable** (e.g. you can click **Get** / **Load** under the model step, or the shortcut button).
- **Model step:** Next shows "Waiting…" and is disabled until a model is loaded (Local) or an API key is set (Cloud); it enables within ~1.5 s after.
- **macOS permissions step:** Next stays disabled until both Microphone and Accessibility are granted (re-checks every ~2 s).
- **Try it now step:** the dim disappears, only the top instruction bar shows; pressing the global shortcut and speaking produces a transcription and the tour auto-advances to "You're all set".
- **Finish** closes the tour.

- [ ] **Step 4: Verify persistence**

Fully quit and relaunch (`pnpm tauri dev`). Expected: the tour does **not** appear again. Confirm `config.json` now contains `"onboarding_completed": true`.

- [ ] **Step 5: Verify Skip**

Reset the flag (Step 1), relaunch, click **Skip tour** on step 1. Expected: the tour closes immediately and `config.json` gets `"onboarding_completed": true`; relaunching does not show it.

- [ ] **Step 6: Final type-check**

Run: `pnpm lint`
Expected: no output, exit code 0.

---

## Self-review notes

- **Spec coverage:** welcome (Task 1 step welcome), permissions macOS/Linux/Windows variants (Task 1 + gate), engine/model with `recordingReady` gate (Task 1), shortcut/language/recording-options (Task 1 + Task 5 anchors), live test via `transcription-added` + mask removal (Task 1 `hideMask`/`awaitWindowEvent`, Task 4), done/sidebar recap (Task 1), first-run flag in `config.json` (Task 3), skip + no replay UI (Task 3/4), four-panel mask with interactive hole (Task 4), platform adaptation (Task 1 `buildSteps`), StrictMode guard (Task 3 `startedRef`), `z-40` vs HUD `z-50` (Task 4). All spec sections map to a task.
- **Type consistency:** `OnboardingStep`, `PermissionsStatus`, `ModelStatus`, `SpotlightRect`, `useSpotlight(target, active, stepIndex)`, `useOnboarding()` value shape, and `buildSteps(platform)` signatures match across Tasks 1–4.
- **No placeholders:** every code step contains complete, final code.
