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
