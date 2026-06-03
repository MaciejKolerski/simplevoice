import { useEffect, useRef, useState } from "react";

export interface SpotlightRect {
  top: number;
  left: number;
  width: number;
  height: number;
}

function sameRect(a: SpotlightRect | null, b: SpotlightRect | null): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return (
    Math.abs(a.top - b.top) < 0.5 &&
    Math.abs(a.left - b.left) < 0.5 &&
    Math.abs(a.width - b.width) < 0.5 &&
    Math.abs(a.height - b.height) < 0.5
  );
}

export function useSpotlight(
  target: string | undefined,
  active: boolean,
  stepIndex: number,
): SpotlightRect | null {
  const [rect, setRect] = useState<SpotlightRect | null>(null);
  const rectRef = useRef<SpotlightRect | null>(null);

  useEffect(() => {
    if (!active || !target) {
      rectRef.current = null;
      setRect(null);
      return;
    }

    let scrolled = false;
    let cancelled = false;

    const commit = (next: SpotlightRect | null) => {
      if (cancelled) return;
      if (sameRect(rectRef.current, next)) return;
      rectRef.current = next;
      setRect(next);
    };

    const locate = () => {
      const el = document.querySelector<HTMLElement>(
        `[data-tour="${target}"]`,
      );
      if (!el) {
        commit(null);
        return;
      }
      if (!scrolled) {
        scrolled = true;
        // Instant scroll only when needed so the spotlight does not chase a
        // smooth-scroll animation. block:"nearest" avoids gratuitous jumps.
        el.scrollIntoView({ block: "nearest", behavior: "auto" });
      }
      const r = el.getBoundingClientRect();
      commit({ top: r.top, left: r.left, width: r.width, height: r.height });
    };

    // Measure after paint/layout settles to avoid stale rects on view switch.
    let raf1 = 0;
    let raf2 = 0;
    raf1 = window.requestAnimationFrame(() => {
      raf2 = window.requestAnimationFrame(locate);
    });

    const ro = new ResizeObserver(locate);
    let observed: HTMLElement | null = document.querySelector<HTMLElement>(
      `[data-tour="${target}"]`,
    );
    if (observed) ro.observe(observed);

    // Light fallback poll catches targets that mount late or move without a
    // resize (e.g. layout reflow after the view becomes visible).
    const poll = window.setInterval(() => {
      const el = document.querySelector<HTMLElement>(
        `[data-tour="${target}"]`,
      );
      if (el && el !== observed) {
        if (observed) ro.unobserve(observed);
        observed = el;
        ro.observe(observed);
        scrolled = false;
      }
      locate();
    }, 400);

    window.addEventListener("resize", locate);
    window.addEventListener("scroll", locate, true);

    return () => {
      cancelled = true;
      window.cancelAnimationFrame(raf1);
      window.cancelAnimationFrame(raf2);
      window.clearInterval(poll);
      ro.disconnect();
      window.removeEventListener("resize", locate);
      window.removeEventListener("scroll", locate, true);
    };
  }, [target, active, stepIndex]);

  return rect;
}
