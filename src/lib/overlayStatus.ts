export type OverlayStatus = "idle" | "recording" | "transcribing";

/** Recording events own the recording state. Late transcription status events
 * must not overwrite it after another capture starts. */
export function applyTranscribingStatus(
  current: OverlayStatus,
  transcribing: boolean,
): OverlayStatus {
  if (current === "recording") {
    return "recording";
  }
  return transcribing ? "transcribing" : "idle";
}
