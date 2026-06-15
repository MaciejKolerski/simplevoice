import type { TFunction } from "i18next";

/**
 * Localizes an error string returned by the Rust backend.
 *
 * The backend encodes user-facing errors as i18n keys so the frontend can show
 * them in the currently selected UI language (instead of a fixed language):
 *
 *   "errors.<key>"            — a plain key, e.g. "errors.cloud_extract_text"
 *   "errors.<key>::<detail>"  — a key plus dynamic text interpolated as
 *                               {{detail}}, e.g. "errors.api_key_missing::OpenAI"
 *                               or "errors.cloud_api_error::404 Not Found — ...".
 *
 * Anything that does not start with "errors." (raw technical strings, JS errors)
 * is returned unchanged so we never hide an untranslated message behind a key.
 */
export function localizeError(t: TFunction, raw: unknown): string {
  const msg = typeof raw === "string" ? raw : String(raw ?? "");
  if (!msg.startsWith("errors.")) return msg;
  const sep = msg.indexOf("::");
  const key = sep === -1 ? msg : msg.slice(0, sep);
  const detail = sep === -1 ? undefined : msg.slice(sep + 2);
  return t(key, { defaultValue: detail ?? key, detail });
}
