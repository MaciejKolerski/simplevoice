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
