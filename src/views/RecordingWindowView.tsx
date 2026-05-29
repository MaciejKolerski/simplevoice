import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

export function RecordingWindowView() {
  const [status, setStatus] = useState<"idle" | "recording" | "transcribing">("idle");
  const [amplitude, setAmplitude] = useState<number>(0);
  const [locked, setLocked] = useState<boolean>(true);

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

    // Listen to recording-started
    const unlistenStarted = listen("recording-started", () => {
      setStatus("recording");
    });

    // Listen to recording-stopped
    const unlistenStopped = listen("recording-stopped", () => {
      setStatus("transcribing");
      setAmplitude(0);
    });

    // Listen to transcribing-status
    const unlistenTranscribing = listen<boolean>("transcribing-status", (event) => {
      const active = event.payload;
      if (!active) {
        setStatus("idle");
      } else {
        setStatus("transcribing");
      }
    });

    // Listen to audio amplitude from backend
    const unlistenAmplitude = listen<number>("audio-amplitude", (event) => {
      setAmplitude(event.payload);
    });

    // Listen to lock status events
    const unlistenLock = listen<boolean>("recording-window-lock-status", (event) => {
      setLocked(event.payload);
    });

    return () => {
      unlistenStarted.then((f) => f());
      unlistenStopped.then((f) => f());
      unlistenTranscribing.then((f) => f());
      unlistenAmplitude.then((f) => f());
      unlistenLock.then((f) => f());
    };
  }, []);

  // Determine wave heights for 9 bars
  // Left-to-right multiplier factor (Gaussian distribution)
  const multipliers = [0.2, 0.45, 0.75, 0.95, 1.0, 0.95, 0.75, 0.45, 0.2];

  // Calculate heights
  const barHeights = multipliers.map((mult) => {
    if (status === "recording") {
      // amplitude from backend is typically in range [0.0, 0.2] depending on microphone gain.
      // We multiply it to get a responsive scale.
      const normalizedAmp = Math.min(amplitude * 6.0, 1.0);
      return 3 + normalizedAmp * 21 * mult; // Min 3px, Max 24px
    } else if (status === "transcribing") {
      return 10;
    } else {
      // Idle state: tiny dots
      return 3;
    }
  });

  return (
    <div className="w-full h-full flex items-center justify-center select-none pointer-events-none">
      {/* Sleek Glassmorphic Pill */}
      <div
        data-tauri-drag-region
        className={`flex items-center justify-center px-5 h-[36px] rounded-full border bg-[#0d0d0e]/75 backdrop-blur-xl shadow-[0_12px_40px_rgba(0,0,0,0.6),inset_0_1px_1px_rgba(255,255,255,0.1)] transition-all duration-300 pointer-events-auto cursor-grab active:cursor-grabbing ${
          !locked ? "border-amber-500/80 shadow-[0_0_12px_rgba(245,158,11,0.4)]" : "border-white/10"
        }`}
      >
        {/* Visualizer centered - key forces complete DOM remount on status change, preventing rendering glitches */}
        <div key={status} className="flex items-center justify-center">
          {status === "transcribing" ? (
            // A gorgeous smooth wave effect during transcription
            <div className="flex items-center justify-center gap-1 h-6">
              {Array.from({ length: 9 }).map((_, i) => (
                <div
                  key={i}
                  className="w-1 rounded-full bg-gradient-to-t from-[#6366f1] to-[#a855f7] animate-[pulse_1s_infinite_ease-in-out]"
                  style={{
                    height: "10px",
                    animationDelay: `${i * 0.1}s`,
                  }}
                />
              ))}
            </div>
          ) : (
            // Interactive waveform for recording or idle
            <div className="flex items-center justify-center gap-1 h-6">
              {barHeights.map((height, i) => {
                // Color gets more vibrant at the center
                const opacity = 0.3 + multipliers[i] * 0.7;
                return (
                  <div
                    key={i}
                    className="w-1 rounded-full transition-[height] duration-75 ease-out"
                    style={{
                      height: `${height}px`,
                      opacity,
                      background: status === "recording"
                        ? "linear-gradient(to top, #ec4899, #a855f7)"
                        : "rgba(255, 255, 255, 0.4)",
                    }}
                  />
                );
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
