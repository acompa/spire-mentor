// The Tools menu's language switch: English / 中文. The UI reads the
// language as it renders, so a pick reloads the page to apply it everywhere.
import { getLang, setLang, t, type Lang } from "../i18n";
import type { MenuItemSpec } from "./Menu";

const LANGS: { lang: Lang; label: string }[] = [
  { lang: "en", label: "English" },
  { lang: "zh", label: "中文" },
];

export function languageItems(reload: () => void = () => location.reload()): MenuItemSpec[] {
  const cur = getLang();
  return LANGS.map(({ lang, label }) => ({
    id: `lang-${lang}`,
    label,
    title: t("Language"),
    checked: cur === lang,
    onSelect: () => {
      if (lang === cur) return;
      setLang(lang);
      reload();
    },
  }));
}
