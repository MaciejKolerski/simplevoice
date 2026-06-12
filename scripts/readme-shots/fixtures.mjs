// Fixture data for README captures. Illustrative product-UI values consistent
// with existing brand assets (42m 13s / 48,210 / +12% / Parakeet TDT v3).
// Dates are computed at runtime so "today" is always the last chart bar.

const DAY_SECONDS = [241, 393, 177, 494, 291, 570, 367]; // Mon..Sun ≈ silhouette 38/62/28/78/46/90/58, sum 2533s = 42m13s
const WORDS_PER_SEC = 19.032; // yields ~48,210 words for 2533s

export function isoDaysAgo(n) {
  const d = new Date();
  d.setDate(d.getDate() - n);
  const y = d.getFullYear(), m = String(d.getMonth() + 1).padStart(2, "0"), r = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${r}`;
}

export function usageStats() {
  const daily = [];
  // current week: today = index 6 (Sun position in the chart = last bar)
  for (let i = 6; i >= 0; i--) {
    const sec = DAY_SECONDS[6 - i];
    daily.push({ date: isoDaysAgo(i), time_transcribed_sec: sec, words_generated: Math.round(sec * WORDS_PER_SEC) });
  }
  // previous week: ~12% lower per day -> +12% trend on both cards
  for (let i = 13; i >= 7; i--) {
    const sec = Math.round(DAY_SECONDS[13 - i] / 1.12);
    daily.push({ date: isoDaysAgo(i), time_transcribed_sec: sec, words_generated: Math.round((sec * WORDS_PER_SEC) / 1.0) });
  }
  const total_duration_sec = daily.reduce((s, d) => s + d.time_transcribed_sec, 0) + 14760; // all-time padding
  const total_words = daily.reduce((s, d) => s + d.words_generated, 0) + 281400;
  return { total_transcriptions: 162, total_words, total_duration_sec, daily };
}

// Field names verified against TranscriptionItem interface in TranscriptionsView.tsx:
// id: string, timestamp: string, date: string, text: string, model: string,
// wav_path?: string, duration_sec?: number. No word_count or created_at field.
export function transcriptions() {
  const mk = (daysAgo, timeStr, text, duration_sec) => ({
    id: String(Math.abs(text.length * 7919 + daysAgo)),
    date: isoDaysAgo(daysAgo),
    timestamp: timeStr,
    text,
    duration_sec,
    model: "parakeet-tdt-0.6b-v3.onnx",
    wav_path: null,
  });
  return [
    mk(0, "09:14", "Ship the release notes today, then schedule the launch review for Friday morning.", 9),
    mk(0, "11:32", "Sounds great — let's lock Friday for the launch review.", 5),
    mk(1, "14:05", "Draft the changelog before standup and flag anything risky for the desktop build.", 8),
    mk(2, "16:48", "Remember to test the new recording overlay position controls on the external display.", 9),
    mk(3, "10:21", "Shipping the fix in ten minutes.", 4),
    mk(5, "13:57", "Walk through the onboarding flow once more and tighten the copy on the final step.", 8),
  ];
}

export const CONFIG = {
  onboarding_completed: true,
  ui_language: "en",
  sound_feedback_enabled: true,
  pause_audio_on_record: false,
  // recording_window_mode is stored in localStorage, not config.json
  // vad_enabled is stored in localStorage, not config.json
  // shortcuts are stored in localStorage, not config.json
};

export const MODELS = {
  modelsDir: "/Users/you/Library/Application Support/com.woro.simplevoice/models",
  active: "parakeet-tdt-0.6b-v3.onnx",
  // LocalModel shape per ModelsView.tsx interface:
  // name, filename, path, size_bytes, size_formatted, quality, speed,
  // is_active, format, architecture, hf_model_id, needs_conversion
  scan: [
    {
      name: "Parakeet TDT v3 (ONNX)",
      filename: "parakeet-tdt-0.6b-v3.onnx",
      path: "/Users/you/Library/Application Support/com.woro.simplevoice/models/parakeet-tdt-0.6b-v3.onnx",
      size_bytes: 671088640,
      size_formatted: "640 MB",
      quality: 5,
      speed: 5,
      is_active: true,
      format: "onnx",
      architecture: null,
      hf_model_id: null,
      needs_conversion: false,
    },
    {
      name: "Whisper Base English (GGML)",
      filename: "ggml-base.en.bin",
      path: "/Users/you/Library/Application Support/com.woro.simplevoice/models/ggml-base.en.bin",
      size_bytes: 147951465,
      size_formatted: "141 MB",
      quality: 3,
      speed: 4,
      is_active: false,
      format: "ggml_bin",
      architecture: null,
      hf_model_id: null,
      needs_conversion: false,
    },
  ],
  cloud: ["whisper-1", "gpt-4o-mini-transcribe"],
};

// list_audio_devices returns string[] per SettingsView.tsx (useState<string[]>)
export const DEVICES = ["MacBook Pro Microphone", "AirPods Pro"];

// check_permissions_status shape per SettingsView.tsx:
// { accessibility, microphone, platform, is_wayland, desktop_env }
export const PERMISSIONS = { platform: "macos", microphone: true, accessibility: true, is_wayland: false, desktop_env: "macos" };
