import {
  CSSProperties,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { useOnboarding } from "./OnboardingProvider";
import { useSpotlight, SpotlightRect } from "./useSpotlight";
import { Button } from "@/components/ui/button";

const CARD_WIDTH = 340;
const GAP = 14;
const MARGIN = 16;
const TRANSITION = "top 180ms cubic-bezier(0.16,1,0.3,1), left 180ms cubic-bezier(0.16,1,0.3,1), width 180ms cubic-bezier(0.16,1,0.3,1), height 180ms cubic-bezier(0.16,1,0.3,1)";

type Side = "below" | "above" | "right" | "left";

interface Placement {
  top: number;
  left: number;
  maxHeight: number;
}

function clamp(value: number, min: number, max: number): number {
  if (max < min) return min;
  return Math.min(Math.max(value, min), max);
}

// Choose the side of the target with the most room, then clamp the card box
// fully inside the viewport on both axes with a MARGIN gutter.
function placeCard(
  rect: SpotlightRect,
  cardW: number,
  cardH: number,
  vw: number,
  vh: number,
): Placement {
  const roomBelow = vh - (rect.top + rect.height) - GAP;
  const roomAbove = rect.top - GAP;
  const roomRight = vw - (rect.left + rect.width) - GAP;
  const roomLeft = rect.left - GAP;
  const tall = rect.height > vh * 0.55;

  const options: { side: Side; room: number }[] = [
    { side: "below", room: roomBelow },
    { side: "above", room: roomAbove },
    { side: "right", room: roomRight },
    { side: "left", room: roomLeft },
  ];
  // Tall targets (sidebar, settings sections) strongly prefer a beside
  // placement so the card never has to fit in the sliver above/below.
  options.sort((a, b) => {
    const beside = (s: Side) => (s === "left" || s === "right" ? 1 : 0);
    if (tall && beside(a.side) !== beside(b.side)) {
      return beside(b.side) - beside(a.side);
    }
    return b.room - a.room;
  });
  const side = options[0].side;

  const maxTop = vh - MARGIN - Math.min(cardH, vh - 2 * MARGIN);
  const maxLeft = vw - MARGIN - Math.min(cardW, vw - 2 * MARGIN);

  let top: number;
  let left: number;
  let maxHeight: number;

  if (side === "below") {
    top = rect.top + rect.height + GAP;
    left = rect.left;
    maxHeight = vh - MARGIN - top;
  } else if (side === "above") {
    top = rect.top - GAP - cardH;
    left = rect.left;
    maxHeight = rect.top - GAP - MARGIN;
  } else {
    // beside: vertically center against the target, then clamp.
    left = side === "right" ? rect.left + rect.width + GAP : rect.left - GAP - cardW;
    top = rect.top + rect.height / 2 - cardH / 2;
    maxHeight = vh - 2 * MARGIN;
  }

  top = clamp(top, MARGIN, maxTop);
  left = clamp(left, MARGIN, maxLeft);
  maxHeight = clamp(maxHeight, 120, vh - 2 * MARGIN);

  return { top, left, maxHeight };
}

export function OnboardingOverlay() {
  const { t } = useTranslation();
  const { active, step, index, total, gateReady, next, back, skip } =
    useOnboarding();
  const rect = useSpotlight(step?.target, active, index);

  const cardRef = useRef<HTMLDivElement | null>(null);
  const [cardSize, setCardSize] = useState({ w: CARD_WIDTH, h: 200 });
  // Hold the last good rect for the CURRENT target only, so transient nulls
  // within a step do not flicker. Drop the hold when the target changes so the
  // spotlight never glides in from a previous (now-hidden) element.
  const lastRectRef = useRef<SpotlightRect | null>(null);
  const lastTargetRef = useRef<string | undefined>(undefined);
  if (step?.target !== lastTargetRef.current) {
    lastTargetRef.current = step?.target;
    lastRectRef.current = null;
  }
  if (rect) lastRectRef.current = rect;

  useLayoutEffect(() => {
    const el = cardRef.current;
    if (!el) return;
    const measure = () => {
      const w = el.offsetWidth;
      const h = el.offsetHeight;
      setCardSize((prev) =>
        Math.abs(prev.w - w) < 0.5 && Math.abs(prev.h - h) < 0.5
          ? prev
          : { w, h },
      );
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [active, step, rect]);

  if (!active || !step) return null;

  const isLast = index === total - 1;
  const nextLabel =
    step.nextLabel ?? (isLast ? t("common.finish") : t("common.next"));
  const nextDisabled = !gateReady;

  const cardBody = (
    <>
      <div className="mb-2 font-mono text-[10px] uppercase tracking-[0.2em] text-muted">
        {t("onboarding.stepCounter", { current: index + 1, total })}
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
          {t("onboarding.skipTour")}
        </button>
        <div className="flex gap-2">
          {index > 0 && (
            <Button variant="outline" size="sm" onClick={back}>
              {t("common.back")}
            </Button>
          )}
          <Button size="sm" onClick={next} disabled={nextDisabled}>
            {nextDisabled ? t("onboarding.waiting") : nextLabel}
          </Button>
        </div>
      </div>
    </>
  );

  const renderCard = (style?: CSSProperties) => (
    <div
      ref={cardRef}
      style={style}
      className="pointer-events-auto flex w-[340px] max-w-[calc(100vw-32px)] flex-col overflow-y-auto rounded-2xl border border-border bg-popover/95 p-5 shadow-[0_24px_64px_-16px_rgba(0,0,0,0.85)] backdrop-blur-md"
    >
      {cardBody}
    </div>
  );

  // Test step: no mask, card pinned top-center, never clipped.
  if (step.hideMask) {
    return (
      <div
        key="onboarding-hidemask"
        className="pointer-events-none fixed inset-0 z-40 flex items-start justify-center p-4"
      >
        {renderCard({ maxHeight: "calc(100vh - 32px)" })}
      </div>
    );
  }

  // Genuinely target-less step (e.g. welcome): centered card on a dim backdrop.
  if (!step.target) {
    return (
      <div
        key="onboarding-centered"
        className="fixed inset-0 z-40 flex items-center justify-center bg-black/70 p-4 backdrop-blur-sm"
      >
        {renderCard({ maxHeight: "calc(100vh - 32px)" })}
      </div>
    );
  }

  const display = rect ?? lastRectRef.current;

  // Targeted step whose element is not measured yet (a 1-2 frame gap on step
  // change): hold a plain dim so the spotlight neither flashes a centered card
  // nor glides in from the previous target.
  if (!display) {
    return (
      <div
        key="onboarding-gap"
        className="fixed inset-0 z-40 bg-black/60 backdrop-blur-[1px]"
      />
    );
  }

  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const pos = placeCard(display, cardSize.w, cardSize.h, vw, vh);

  const maskClass =
    "pointer-events-auto absolute bg-black/60 backdrop-blur-[1px] transition-all duration-200 ease-out";

  const rightW = vw - (display.left + display.width);
  const belowH = vh - (display.top + display.height);

  return (
    <div
      key={`onboarding-spot-${index}`}
      className="pointer-events-none fixed inset-0 z-40"
    >
      <div
        className={maskClass}
        style={{ top: 0, left: 0, width: vw, height: Math.max(0, display.top) }}
      />
      <div
        className={maskClass}
        style={{
          top: display.top + display.height,
          left: 0,
          width: vw,
          height: Math.max(0, belowH),
        }}
      />
      <div
        className={maskClass}
        style={{
          top: display.top,
          left: 0,
          width: Math.max(0, display.left),
          height: display.height,
        }}
      />
      <div
        className={maskClass}
        style={{
          top: display.top,
          left: display.left + display.width,
          width: Math.max(0, rightW),
          height: display.height,
        }}
      />
      <div
        className="pointer-events-none absolute rounded-xl border-2 border-white/70 shadow-[0_0_0_4px_rgba(255,255,255,0.15)] transition-all duration-200 ease-out"
        style={{
          top: display.top - 4,
          left: display.left - 4,
          width: display.width + 8,
          height: display.height + 8,
        }}
      />
      <div
        className="pointer-events-none absolute"
        style={{ top: pos.top, left: pos.left, transition: TRANSITION }}
      >
        {renderCard({ maxHeight: pos.maxHeight })}
      </div>
    </div>
  );
}
