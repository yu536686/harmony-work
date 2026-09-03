import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import zh from "./zh";
import en from "./en";

const LANGUAGE_STORAGE_KEY = "workbuddy-language";
const LEGACY_LANGUAGE_STORAGE_KEY = "piscis-language";

function initialLanguage(): string {
  const current = localStorage.getItem(LANGUAGE_STORAGE_KEY);
  if (current) return current;
  const legacy = localStorage.getItem(LEGACY_LANGUAGE_STORAGE_KEY);
  if (legacy) {
    localStorage.setItem(LANGUAGE_STORAGE_KEY, legacy);
    localStorage.removeItem(LEGACY_LANGUAGE_STORAGE_KEY);
  }
  return legacy || "zh";
}

i18n
  .use(initReactI18next)
  .init({
    resources: {
      zh: { translation: zh },
      en: { translation: en },
    },
    lng: initialLanguage(),
    fallbackLng: "zh",
    interpolation: {
      escapeValue: false,
    },
  });

export default i18n;

/** 切换语言并持久化 */
export function setLanguage(lang: "zh" | "en") {
  i18n.changeLanguage(lang);
  localStorage.setItem(LANGUAGE_STORAGE_KEY, lang);
}
