import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Mic } from "lucide-react";

export function RecordingWindowView() {
  const [status, setStatus] = useState<"idle" | "recording" | "transcribing">("idle");
  const [amplitude, setAmplitude] = useState<number>(0);

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

    return () => {
      unlistenStarted.then((f) => f());
      unlistenStopped.then((f) => f());
      unlistenTranscribing.then((f) => f());
      unlistenAmplitude.then((f) => f());
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
      return 4 + normalizedAmp * 36 * mult; // Min 4px, Max 40px
    } else if (status === "transcribing") {
      return 15;
    } else {
      // Idle state: tiny dots
      return 4;
    }
  });

  return (
    <div className="w-full h-full flex items-center justify-center select-none pointer-events-none">
      {/* Sleek Glassmorphic Pill */}
      <div
        data-tauri-drag-region
        className="flex items-center justify-center px-6 h-[54px] rounded-full border border-white/10 bg-[#0d0d0e]/75 backdrop-blur-xl shadow-[0_12px_40px_rgba(0,0,0,0.6),inset_0_1px_1px_rgba(255,255,255,0.1)] transition-all duration-300 gap-4 pointer-events-auto cursor-grab active:cursor-grabbing"
        style={{ width: "310px" }}
      >
        {/* Left Side Status Indicator / Icon */}
        <div className="flex items-center justify-center">
          {status === "recording" ? (
            <div className="relative flex h-3 w-3">
              <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-red-400 opacity-75"></span>
              <span className="relative inline-flex rounded-full h-3 w-3 bg-red-500"></span>
            </div>
          ) : status === "transcribing" ? (
            // A small rotating loader
            <div className="w-4 h-4 rounded-full border-2 border-white/20 border-t-white animate-spin"></div>
          ) : (
            // Idle microphone icon
            <Mic className="w-4 h-4 text-white/50" />
          )}
        </div>

        {/* Center / Right Visualizer */}
        <div className="flex items-center justify-center flex-1">
          {status === "transcribing" ? (
            // A gorgeous smooth wave effect during transcription
            <div className="flex items-center justify-center gap-1.5 h-10">
              {Array.from({ length: 9 }).map((_, i) => (
                <div
                  key={i}
                  className="w-1.5 rounded-full bg-gradient-to-t from-[#6366f1] to-[#a855f7] animate-[pulse_1s_infinite_ease-in-out]"
                  style={{
                    height: "18px",
                    animationDelay: `${i * 0.1}s`,
                  }}
                />
              ))}
            </div>
          ) : (
            // Interactive waveform for recording or idle
            <div className="flex items-center justify-center gap-1.5 h-10">
              {barHeights.map((height, i) => {
                // Color gets more vibrant at the center
                const opacity = 0.3 + multipliers[i] * 0.7;
                return (
                  <div
                    key={i}
                    className="w-1.5 rounded-full transition-[height] duration-75 ease-out"
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

        {/* Right Side Text Label */}
        <div className="text-[11px] font-medium tracking-wide text-white/70 uppercase w-20 text-right pr-1">
          {status === "recording" ? "Rec..." : status === "transcribing" ? "STT..." : "Ready"}
        </div>
      </div>
    </div>
  );
}
