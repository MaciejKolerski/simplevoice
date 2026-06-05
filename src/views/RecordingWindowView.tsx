import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

type Status = "idle" | "recording" | "transcribing";

// Bar geometry (logical CSS pixels)
const BAR_COUNT = 9;
const BAR_WIDTH = 4; // matches previous w-1 (4px)
const BAR_GAP = 4; // matches previous gap-1 (4px)
const CANVAS_W = BAR_COUNT * BAR_WIDTH + (BAR_COUNT - 1) * BAR_GAP; // 68px
const CANVAS_H = 24; // matches previous h-6

// Gaussian-ish multiplier per bar (left-to-right), louder in the center
const MULTIPLIERS = [0.2, 0.45, 0.75, 0.95, 1.0, 0.95, 0.75, 0.45, 0.2];

export function RecordingWindowView() {
  const [locked, setLocked] = useState<boolean>(true);

  // Live transcription text (only populated when live mode is on; the backend
  // never emits these events otherwise, so the panel stays hidden).
  const [committed, setCommitted] = useState("");
  const [tentative, setTentative] = useState("");
  const expandedRef = useRef(false);

  // Status and amplitude are read inside the rAF loop, so they live in refs to
  // avoid re-renders (and to avoid relying on CSS transitions, which ghost on
  // transparent WebKitGTK windows under Linux).
  const statusRef = useRef<Status>("idle");
  const amplitudeRef = useRef<number>(0);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    // Override body backgrounds for transparency
    document.body.style.background = "transparent";
    document.body.style.backgroundColor = "transparent";
    document.documentElement.style.background = "transparent";
    document.documentElement.style.backgroundColor = "transparent";
    const root = document.getElementById("root");
    if (root) {
      root.style.background = "transparent";
      root.style.backgroundColor = "transparent";
    }

    // Query initial lock state
    invoke<boolean>("is_recording_window_locked_cmd")
      .then(setLocked)
      .catch(() => {});

    const unlistenStarted = listen("recording-started", () => {
      statusRef.current = "recording";
      setCommitted("");
      setTentative("");
    });

    const unlistenStopped = listen("recording-stopped", () => {
      statusRef.current = "transcribing";
      amplitudeRef.current = 0;
    });

    const unlistenTranscribing = listen<boolean>("transcribing-status", (event) => {
      statusRef.current = event.payload ? "transcribing" : "idle";
      if (!event.payload) amplitudeRef.current = 0;
    });

    const unlistenAmplitude = listen<number>("audio-amplitude", (event) => {
      amplitudeRef.current = event.payload;
    });

    const unlistenLock = listen<boolean>("recording-window-lock-status", (event) => {
      setLocked(event.payload);
    });

    // Live transcription stream (Faza 0b backend). committed = solid, tentative = dimmed.
    const unlistenCommitted = listen<{ delta: string; full: string }>(
      "transcription-committed",
      (event) => setCommitted(event.payload.full),
    );
    const unlistenPartial = listen<{ text: string }>(
      "transcription-partial",
      (event) => setTentative(event.payload.text),
    );
    const unlistenFinal = listen<{ text: string }>(
      "transcription-final",
      (event) => {
        setCommitted(event.payload.text);
        setTentative("");
      },
    );

    return () => {
      unlistenStarted.then((f) => f());
      unlistenStopped.then((f) => f());
      unlistenTranscribing.then((f) => f());
      unlistenAmplitude.then((f) => f());
      unlistenLock.then((f) => f());
      unlistenCommitted.then((f) => f());
      unlistenPartial.then((f) => f());
      unlistenFinal.then((f) => f());
    };
  }, []);

  // Grow the overlay window when live text is present, shrink it back when not.
  // Width stays 200 (preserves the centered position); only height changes.
  useEffect(() => {
    const hasText = committed.length > 0 || tentative.length > 0;
    if (hasText === expandedRef.current) return;
    expandedRef.current = hasText;
    getCurrentWindow()
      .setSize(new LogicalSize(200, hasText ? 170 : 60))
      .catch(() => {});
  }, [committed, tentative]);

  // Canvas render loop. Drawing on a fixed-size canvas and clearing every frame
  // avoids the partial-repaint ghosting that animated DOM elements suffer from
  // on transparent WebKitGTK (Linux) windows.
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = Math.max(1, window.devicePixelRatio || 1);
    canvas.width = Math.round(CANVAS_W * dpr);
    canvas.height = Math.round(CANVAS_H * dpr);
    canvas.style.width = `${CANVAS_W}px`;
    canvas.style.height = `${CANVAS_H}px`;
    ctx.scale(dpr, dpr);

    // Drive the waveform gradient from the brand tokens so it stays in sync with
    // the design system (brand: live waveform is indigo -> purple).
    const rootStyles = getComputedStyle(document.documentElement);
    const waveFrom = rootStyles.getPropertyValue("--wave-from").trim() || "#6366f1";
    const waveTo = rootStyles.getPropertyValue("--wave-to").trim() || "#a855f7";

    let raf = 0;
    let displayAmp = 0; // smoothed amplitude, lerped toward the target each frame

    const drawBar = (x: number, h: number, fill: string | CanvasGradient, alpha: number) => {
      const y = (CANVAS_H - h) / 2;
      const r = Math.min(BAR_WIDTH, h) / 2;
      ctx.globalAlpha = alpha;
      ctx.fillStyle = fill;
      ctx.beginPath();
      if (typeof ctx.roundRect === "function") {
        ctx.roundRect(x, y, BAR_WIDTH, h, r);
      } else {
        ctx.rect(x, y, BAR_WIDTH, h);
      }
      ctx.fill();
      ctx.globalAlpha = 1;
    };

    const frame = (t: number) => {
      ctx.clearRect(0, 0, CANVAS_W, CANVAS_H);
      const status = statusRef.current;

      // Smoothly approach the target amplitude (replaces the CSS height transition).
      const target = status === "recording" ? Math.min(amplitudeRef.current * 6.0, 1.0) : 0;
      displayAmp += (target - displayAmp) * 0.35;

      for (let i = 0; i < BAR_COUNT; i++) {
        const x = i * (BAR_WIDTH + BAR_GAP);
        let h: number;
        let fill: string | CanvasGradient;
        let alpha: number;

        if (status === "recording") {
          h = 3 + displayAmp * 21 * MULTIPLIERS[i]; // 3px..24px
          const g = ctx.createLinearGradient(0, (CANVAS_H + h) / 2, 0, (CANVAS_H - h) / 2);
          g.addColorStop(0, waveFrom);
          g.addColorStop(1, waveTo);
          fill = g;
          alpha = 0.3 + MULTIPLIERS[i] * 0.7;
        } else if (status === "transcribing") {
          // Gentle traveling pulse, 1s period with per-bar phase offset.
          const phase = (t / 1000 + i * 0.1) * Math.PI * 2;
          const pulse = (Math.sin(phase) + 1) / 2; // 0..1
          h = 6 + pulse * 6; // 6px..12px
          const g = ctx.createLinearGradient(0, (CANVAS_H + h) / 2, 0, (CANVAS_H - h) / 2);
          g.addColorStop(0, waveFrom);
          g.addColorStop(1, waveTo);
          fill = g;
          alpha = 0.5 + pulse * 0.5;
        } else {
          // idle: tiny dots
          h = 3;
          fill = "rgba(255, 255, 255, 0.4)";
          alpha = 1;
        }

        drawBar(x, h, fill, alpha);
      }

      raf = requestAnimationFrame(frame);
    };

    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, []);

  const hasText = committed.length > 0 || tentative.length > 0;

  return (
    <div className="w-full h-full flex items-center justify-center select-none pointer-events-none">
      <div className="flex flex-col items-center gap-2">
        <div
          data-tauri-drag-region
          className={`flex items-center justify-center px-5 h-[36px] rounded-full border bg-[#0d0d0e]/75 backdrop-blur-xl shadow-[0_12px_40px_rgba(0,0,0,0.6),inset_0_1px_1px_rgba(255,255,255,0.1)] transition-all duration-300 pointer-events-auto cursor-grab active:cursor-grabbing ${
            !locked ? "border-amber-500/80 shadow-[0_0_12px_rgba(245,158,11,0.4)]" : "border-white/10"
          }`}
        >
          <canvas ref={canvasRef} className="block" />
        </div>
        {hasText && (
          <div className="w-[180px] max-h-[110px] overflow-hidden rounded-2xl border border-white/10 bg-[#0d0d0e]/80 backdrop-blur-xl px-3 py-2 text-left shadow-[0_12px_40px_rgba(0,0,0,0.6)]">
            <p className="text-[12px] leading-snug break-words text-white/95">
              {committed}
              {tentative && (
                <span className="text-white/45 italic">
                  {committed ? " " : ""}
                  {tentative}
                </span>
              )}
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
