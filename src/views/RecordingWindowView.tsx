import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import i18n from "../i18n";
import { isSupported } from "../i18n/detect";
import { applyTranscribingStatus, type OverlayStatus as Status } from "@/lib/overlayStatus";

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
  // "full" = whole running text; "recent" = only the last few words. Shared via
  // localStorage (same origin) at mount, updated live via a Tauri event.
  const [overlayMode, setOverlayMode] = useState<string>(
    () => localStorage.getItem("live_overlay_mode") || "full",
  );

  // Status and amplitude are read inside the rAF loop, so they live in refs to
  // avoid re-renders (and to avoid relying on CSS transitions, which ghost on
  // transparent WebKitGTK windows under Linux).
  const statusRef = useRef<Status>("idle");
  const amplitudeRef = useRef<number>(0);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const { t } = useTranslation();
  const [elapsed, setElapsed] = useState<number | null>(null);
  const [progress, setProgress] = useState<{ done: number; total: number } | null>(null);
  const [warningSecs, setWarningSecs] = useState<number | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const warningDeadlineRef = useRef<number | null>(null);
  const startedAtRef = useRef<number>(0);

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

    // This window skips applyPersistedLanguage (it would re-push tray labels),
    // so sync the overlay strings to the configured UI language directly.
    invoke<string>("load_config")
      .then((str) => {
        const lang = JSON.parse(str || "{}").ui_language;
        if (typeof lang === "string" && isSupported(lang) && i18n.language !== lang) {
          i18n.changeLanguage(lang).catch(() => {});
        }
      })
      .catch(() => {});

    // Follow live language switches made in the main window's settings.
    const unlistenLanguage = listen<string>("ui-language-changed", (event) => {
      const lang = event.payload;
      if (typeof lang === "string" && isSupported(lang) && i18n.language !== lang) {
        i18n.changeLanguage(lang).catch(() => {});
      }
    });

    const stopTimer = () => {
      if (timerRef.current) {
        clearInterval(timerRef.current);
        timerRef.current = null;
      }
    };

    const unlistenStarted = listen("recording-started", () => {
      statusRef.current = "recording";
      setCommitted("");
      setTentative("");
      setProgress(null);
      setWarningSecs(null);
      warningDeadlineRef.current = null;
      startedAtRef.current = Date.now();
      setElapsed(0);
      stopTimer();
      timerRef.current = setInterval(() => {
        setElapsed(Math.floor((Date.now() - startedAtRef.current) / 1000));
        if (warningDeadlineRef.current !== null) {
          setWarningSecs(Math.max(0, Math.round((warningDeadlineRef.current - Date.now()) / 1000)));
        }
      }, 1000);
    });

    const unlistenStopped = listen("recording-stopped", () => {
      statusRef.current = "transcribing";
      amplitudeRef.current = 0;
      stopTimer();
      setElapsed(null);
      setWarningSecs(null);
      warningDeadlineRef.current = null;
    });

    const unlistenTranscribing = listen<boolean>("transcribing-status", (event) => {
      statusRef.current = applyTranscribingStatus(statusRef.current, event.payload);
      if (!event.payload) {
        // Clear the finished transcription's progress panel even mid-recording,
        // but leave the live amplitude alone while a recording is active.
        if (statusRef.current !== "recording") {
          amplitudeRef.current = 0;
        }
        setProgress(null);
      }
    });

    const unlistenAmplitude = listen<number>("audio-amplitude", (event) => {
      amplitudeRef.current = event.payload;
    });

    const unlistenLock = listen<boolean>("recording-window-lock-status", (event) => {
      setLocked(event.payload);
    });

    // Live transcription stream: committed = solid, tentative = dimmed.
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

    const unlistenOverlayMode = listen<string>(
      "live-overlay-mode-changed",
      (event) => setOverlayMode(event.payload || "full"),
    );

    const unlistenProgress = listen<{ done: number; total: number }>(
      "transcription-progress",
      (event) => setProgress(event.payload),
    );
    const unlistenTimeWarning = listen<{ seconds_left: number }>(
      "recording-time-warning",
      (event) => {
        warningDeadlineRef.current = Date.now() + event.payload.seconds_left * 1000;
        setWarningSecs(event.payload.seconds_left);
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
      unlistenOverlayMode.then((f) => f());
      unlistenLanguage.then((f) => f());
      unlistenProgress.then((f) => f());
      unlistenTimeWarning.then((f) => f());
      stopTimer();
    };
  }, []);

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

  // In "recent" mode show only the last few words (committed + tentative),
  // preserving styling. "full" shows the whole running text.
  const RECENT_WORDS = 12;
  let dispCommitted = committed;
  let dispTentative = tentative;
  if (overlayMode === "recent") {
    const cw = committed ? committed.split(/\s+/).filter(Boolean) : [];
    const tw = tentative ? tentative.split(/\s+/).filter(Boolean) : [];
    const tShow = tw.slice(-RECENT_WORDS);
    const remaining = Math.max(0, RECENT_WORDS - tShow.length);
    dispTentative = tShow.join(" ");
    dispCommitted = cw.slice(-remaining).join(" ");
  }
  const hasText = dispCommitted.length > 0 || dispTentative.length > 0;

  const formatElapsed = (secs: number) => {
    const h = Math.floor(secs / 3600);
    const m = Math.floor((secs % 3600) / 60);
    const s = secs % 60;
    return h > 0
      ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
      : `${m}:${String(s).padStart(2, "0")}`;
  };

  // The overlay window is a fixed 200x180 transparent, click-through panel. We
  // top-anchor the content (pt-3 == the old 12px vertical centering inside the
  // former 60px window) so the waveform pill sits in exactly the same on-screen
  // spot as before; the live-text panel renders below it in the (otherwise
  // transparent) space, so nothing is clipped.
  return (
    <div className="w-full h-full flex flex-col items-center justify-start pt-3 select-none pointer-events-none">
      <div
        data-tauri-drag-region
        className={`flex items-center justify-center px-5 h-[36px] rounded-full border bg-[#0d0d0e]/75 backdrop-blur-xl shadow-[0_12px_40px_rgba(0,0,0,0.6),inset_0_1px_1px_rgba(255,255,255,0.1)] transition-all duration-300 pointer-events-auto cursor-grab active:cursor-grabbing ${
          !locked ? "border-amber-500/80 shadow-[0_0_12px_rgba(245,158,11,0.4)]" : "border-white/10"
        }`}
      >
        <canvas ref={canvasRef} className="block" />
        {elapsed !== null && (
          <span className="ml-2 text-[11px] tabular-nums text-white/70">
            {formatElapsed(elapsed)}
          </span>
        )}
      </div>
      {progress && progress.total > 1 && (
        <div className="mt-2 w-[184px] rounded-2xl border border-white/10 bg-[#0d0d0e]/80 backdrop-blur-xl px-3 py-2 shadow-[0_12px_40px_rgba(0,0,0,0.6)]">
          <p className="text-[11px] leading-snug text-white/85 tabular-nums">
            {t("overlay.transcribing", {
              percent: Math.round((progress.done / progress.total) * 100),
            })}
          </p>
          <div className="mt-1.5 h-1 w-full rounded-full bg-white/10 overflow-hidden">
            <div
              className="h-full rounded-full bg-white/80 transition-all duration-300"
              style={{ width: `${Math.round((progress.done / progress.total) * 100)}%` }}
            />
          </div>
        </div>
      )}
      {warningSecs !== null && (
        <div className="mt-2 rounded-2xl border border-amber-500/40 bg-[#0d0d0e]/80 backdrop-blur-xl px-3 py-1.5 shadow-[0_12px_40px_rgba(0,0,0,0.6)]">
          <p className="text-[11px] leading-snug text-amber-400/95">
            {t("overlay.timeWarning", { time: formatElapsed(warningSecs) })}
          </p>
        </div>
      )}
      {hasText && (
        <div
          className={`mt-2 flex w-[184px] ${warningSecs !== null ? "max-h-[76px]" : "max-h-[120px]"} flex-col justify-end overflow-hidden rounded-2xl border border-white/10 bg-[#0d0d0e]/80 backdrop-blur-xl px-3 py-2 text-left shadow-[0_12px_40px_rgba(0,0,0,0.6)]`}
        >
          <p className="text-[12px] leading-snug break-words text-white/95">
            {dispCommitted}
            {dispTentative && (
              <span className="text-white/45 italic">
                {dispCommitted ? " " : ""}
                {dispTentative}
              </span>
            )}
          </p>
        </div>
      )}
    </div>
  );
}
