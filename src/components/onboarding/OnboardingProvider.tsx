import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
  ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
          // The main window starts hidden (visible: false); reveal it on first
          // run so the tour is actually visible.
          try {
            const win = getCurrentWindow();
            await win.show();
            await win.setFocus();
          } catch (err) {
            console.error("Onboarding: failed to show main window:", err);
          }
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
  }, [index, active, step?.view]);

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
    // Ignore events that fire in the first moment after the step mounts: those
    // are stale / in-flight dispatches that would skip the step before the user
    // has done anything.
    const enteredAt = Date.now();
    const handler = (e: Event) => {
      if (Date.now() - enteredAt < 400) return;
      // If a dispatcher tags an explicit non-recording source (delete / clear
      // history), do not treat it as completing the test step.
      const source = (e as CustomEvent).detail?.source;
      if (source && source !== "recording") return;
      next();
    };
    window.addEventListener(eventName, handler);
    return () => window.removeEventListener(eventName, handler);
  }, [index, active, step?.awaitWindowEvent]);

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
