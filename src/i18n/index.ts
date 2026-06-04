import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./locales/en.json";
import pl from "./locales/pl.json";
import de from "./locales/de.json";
import { detectOsLanguage, SUPPORTED_LANGUAGES } from "./detect";

i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    pl: { translation: pl },
    de: { translation: de },
  },
  lng: detectOsLanguage(),
  fallbackLng: "en",
  supportedLngs: SUPPORTED_LANGUAGES as unknown as string[],
  interpolation: { escapeValue: false },
  returnNull: false,
});

export default i18n;
