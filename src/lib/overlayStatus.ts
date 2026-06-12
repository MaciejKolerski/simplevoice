export type OverlayStatus = "idle" | "recording" | "transcribing";

/**
 * Maps a `transcribing-status` event onto the overlay status.
 *
 * "recording" is owned by the recording-started/-stopped events and must win:
 * the previous transcription's status flips can land seconds late, after the
 * next recording already began (the hidden main webview's deferred
 * `set_transcribing` runs only once the process wakes — typically on that next
 * recording — and a long chunked transcription can finish mid-recording).
 * Letting them through froze the wavebar at idle for the whole take.
 */
export function applyTranscribingStatus(
  current: OverlayStatus,
  transcribing: boolean,
): OverlayStatus {
  if (current === "recording") {
    return "recording";
  }
  return transcribing ? "transcribing" : "idle";
}
