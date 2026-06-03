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
