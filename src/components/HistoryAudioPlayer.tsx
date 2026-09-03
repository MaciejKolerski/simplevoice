import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Loader2, Pause, Play } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";

interface HistoryAudioStatus {
  id: string | null;
  positionSec: number;
  durationSec: number;
  playing: boolean;
}

interface HistoryAudioPlayerProps {
  id: string;
  path: string;
  durationSec?: number;
}

function safeSeconds(value: number | undefined) {
  return Number.isFinite(value) && value != null && value > 0 ? value : 0;
}

function formatTime(value: number) {
  const totalSeconds = Math.max(0, Math.floor(safeSeconds(value)));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;

  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  }
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

export function HistoryAudioPlayer({
  id,
  path,
  durationSec,
}: HistoryAudioPlayerProps) {
  const { t } = useTranslation();
  const [position, setPosition] = useState(0);
  const [duration, setDuration] = useState(() => safeSeconds(durationSec));
  const [playing, setPlaying] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(true);
  const seeking = useRef(false);

  const applyStatus = (status: HistoryAudioStatus) => {
    if (status.id !== id) {
      setPlaying(false);
      setLoaded(false);
      return;
    }
    if (!seeking.current) setPosition(safeSeconds(status.positionSec));
    if (status.durationSec > 0) setDuration(status.durationSec);
    setPlaying(status.playing);
    setLoaded(true);
  };

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      void invoke("stop_history_audio", { id }).catch(() => {});
    };
  }, [id]);

  useEffect(() => {
    if (!playing) return;
    let cancelled = false;

    const refresh = async () => {
      try {
        const status = await invoke<HistoryAudioStatus>(
          "get_history_audio_status",
          { id },
        );
        if (!cancelled && mounted.current) applyStatus(status);
      } catch (statusError) {
        if (!cancelled && mounted.current) {
          setPlaying(false);
          setError(String(statusError));
        }
      }
    };

    void refresh();
    const interval = window.setInterval(refresh, 250);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [id, playing]);

  const togglePlayback = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);

    try {
      const status = playing
        ? await invoke<HistoryAudioStatus>("pause_history_audio", { id })
        : await invoke<HistoryAudioStatus>("play_history_audio", {
            id,
            path,
            positionSec: duration > 0 && position >= duration ? 0 : position,
          });
      if (mounted.current) applyStatus(status);
    } catch (playbackError) {
      if (mounted.current) {
        setPlaying(false);
        setLoaded(false);
        setError(String(playbackError));
      }
    } finally {
      if (mounted.current) setBusy(false);
    }
  };

  const commitSeek = async (nextPosition: number) => {
    seeking.current = false;
    const next = Math.min(Math.max(nextPosition, 0), duration);
    setPosition(next);
    if (!loaded) return;

    try {
      const status = await invoke<HistoryAudioStatus>("seek_history_audio", {
        id,
        positionSec: next,
      });
      if (mounted.current) applyStatus(status);
    } catch (seekError) {
      if (mounted.current) setError(String(seekError));
    }
  };

  return (
    <div
      data-history-audio-player="native"
      role="group"
      aria-label={t("transcriptions.playerLabel")}
      className="bg-surface-active rounded-xl p-4"
      onClick={(event) => event.stopPropagation()}
    >
      <div className="flex items-center gap-3">
        <Button
          type="button"
          variant="secondary"
          size="icon-sm"
          onClick={togglePlayback}
          disabled={busy}
          aria-label={t(
            playing
              ? "transcriptions.pauseRecording"
              : "transcriptions.playRecording",
          )}
          className="shrink-0 text-success"
        >
          {busy ? (
            <Loader2 size={15} className="animate-spin" />
          ) : playing ? (
            <Pause size={15} fill="currentColor" />
          ) : (
            <Play size={15} fill="currentColor" />
          )}
        </Button>
        <span className="mono w-10 shrink-0 text-right text-[11px] text-muted tabular-nums">
          {formatTime(position)}
        </span>
        <input
          type="range"
          min={0}
          max={Math.max(duration, 0.1)}
          step={0.1}
          value={Math.min(position, Math.max(duration, 0.1))}
          disabled={duration <= 0 || busy}
          aria-label={t("transcriptions.seekRecording")}
          aria-valuetext={`${formatTime(position)} / ${formatTime(duration)}`}
          className="h-1 min-w-0 flex-1 cursor-pointer accent-success disabled:cursor-not-allowed disabled:opacity-50"
          onPointerDown={() => {
            seeking.current = true;
          }}
          onKeyDown={() => {
            seeking.current = true;
          }}
          onChange={(event) => setPosition(Number(event.currentTarget.value))}
          onPointerUp={(event) => {
            void commitSeek(Number(event.currentTarget.value));
          }}
          onKeyUp={(event) => {
            void commitSeek(Number(event.currentTarget.value));
          }}
          onBlur={(event) => {
            if (seeking.current) {
              void commitSeek(Number(event.currentTarget.value));
            }
          }}
        />
        <span className="mono w-14 shrink-0 text-[11px] text-muted tabular-nums">
          {formatTime(duration)}
        </span>
      </div>
      {error && (
        <p role="alert" className="mt-2 text-xs leading-relaxed text-danger">
          {t("transcriptions.audioPlaybackFailed", { detail: error })}
        </p>
      )}
    </div>
  );
}
