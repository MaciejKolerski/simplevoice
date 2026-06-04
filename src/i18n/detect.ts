export const SUPPORTED_LANGUAGES = ["en", "pl", "de"] as const;
export type Language = (typeof SUPPORTED_LANGUAGES)[number];

export function isSupported(lang: string): lang is Language {
  return (SUPPORTED_LANGUAGES as readonly string[]).includes(lang);
}

// First-run default: derive from the OS/webview locale, else English.
export function detectOsLanguage(): Language {
  const raw = (navigator.language || "en").slice(0, 2).toLowerCase();
  return isSupported(raw) ? raw : "en";
}
