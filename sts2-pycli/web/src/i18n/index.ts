/**
 * The workbench's UI language (English, Simplified Chinese).
 *
 * Strings are keyed by their English text, gettext-style: `t("End turn")`
 * renders "End turn" in English and the `zh` table's entry in Chinese, and a
 * string missing from the table falls back to its English key, so an
 * untranslated label is English, never blank. `{name}` placeholders are
 * filled from `vars`: `t("Floor {n}", { n: 3 })`.
 *
 * The language is picked once, at load: `?lang=zh|en` (remembered in
 * localStorage, so a link the mod opens keeps it), else the remembered
 * choice, else the browser's language list (`navigator.languages`, the
 * first entry that is English or Chinese). Anything else is English.
 *
 * The Chinese tables live in `./zh/*.ts`, one per area of the UI, each a
 * default-exported `Record<english, chinese>`; they are merged here, so a new
 * table needs no registration. `i18n.test.ts` fails when a literal `t("…")`
 * in `src/` has no `zh` entry.
 *
 * Game content (card, relic and monster names, card text) is not translated
 * here: it comes from the game's own localization tables.
 */

export type Lang = "en" | "zh";

/** localStorage key of the remembered language. */
export const LANG_KEY = "sts2.lang";

type Table = Record<string, string>;

const zhModules = import.meta.glob<{ default: Table }>("./zh/*.ts", { eager: true });

/** Every `zh` table merged (a key in two tables: the later file wins). */
export const ZH: Table = Object.assign(
  {},
  ...Object.keys(zhModules)
    .sort()
    .map((k) => zhModules[k].default),
);

/** `zh` for a BCP 47 tag naming Chinese (`zh`, `zh-CN`, `zh-Hans-SG`,
 *  `zh-TW`…), `en` for English, else null. Traditional Chinese reads the
 *  Simplified table: the game ships no Traditional one either. */
export function langFromTag(tag: string | null | undefined): Lang | null {
  const t = (tag ?? "").trim().toLowerCase();
  if (t === "zh" || t.startsWith("zh-") || t.startsWith("zh_") || t === "zhs" || t === "chinese") {
    return "zh";
  }
  if (t === "en" || t.startsWith("en-") || t.startsWith("en_") || t === "eng" || t === "english") {
    return "en";
  }
  return null;
}

function storageGet(): string | null {
  try {
    return globalThis.localStorage?.getItem(LANG_KEY) ?? null;
  } catch {
    return null;
  }
}

function storageSet(v: Lang): void {
  try {
    globalThis.localStorage?.setItem(LANG_KEY, v);
  } catch {
    /* private window: the choice lasts this page only */
  }
}

/** The language for a page at `search` with browser languages `navLangs`
 *  and remembered choice `stored` (pure; see the module doc for the order). */
export function resolveLang(
  search: string,
  stored: string | null,
  navLangs: readonly string[],
): { lang: Lang; fromUrl: boolean } {
  let fromUrl: Lang | null = null;
  try {
    fromUrl = langFromTag(new URLSearchParams(search).get("lang"));
  } catch {
    fromUrl = null;
  }
  if (fromUrl) {
    return { lang: fromUrl, fromUrl: true };
  }
  const remembered = langFromTag(stored);
  if (remembered) {
    return { lang: remembered, fromUrl: false };
  }
  for (const tag of navLangs) {
    const l = langFromTag(tag);
    if (l) {
      return { lang: l, fromUrl: false };
    }
  }
  return { lang: "en", fromUrl: false };
}

function detect(): Lang {
  const nav = globalThis.navigator;
  const navLangs = nav ? (nav.languages?.length ? nav.languages : [nav.language]) : [];
  const search = globalThis.location?.search ?? "";
  const { lang, fromUrl } = resolveLang(search, storageGet(), navLangs);
  if (fromUrl) {
    storageSet(lang);
  }
  return lang;
}

let current: Lang = detect();
if (typeof document !== "undefined") {
  document.documentElement.lang = current === "zh" ? "zh-Hans" : "en";
}

/** The UI language this page renders in. */
export function getLang(): Lang {
  return current;
}

/** Switch the language and remember it. The UI reads the language as it
 *  renders, so callers reload the page (`location.reload()`) to apply it
 *  everywhere; tests call this between renders. */
export function setLang(lang: Lang, remember = true): void {
  current = lang;
  if (remember) {
    storageSet(lang);
  }
  if (typeof document !== "undefined") {
    document.documentElement.lang = lang === "zh" ? "zh-Hans" : "en";
  }
}

/** Fill `{name}` placeholders from `vars` (a missing var stays as written). */
export function fill(template: string, vars?: Record<string, string | number>): string {
  if (!vars) {
    return template;
  }
  return template.replace(/\{(\w+)\}/g, (m, k: string) =>
    Object.prototype.hasOwnProperty.call(vars, k) ? String(vars[k]) : m,
  );
}

/** `en` in the page's language, placeholders filled. */
export function t(en: string, vars?: Record<string, string | number>): string {
  const s = current === "zh" ? (ZH[en] ?? en) : en;
  return fill(s, vars);
}

/** `t` for a count: `one` when `n` is 1, else `many` (English plurals;
 *  Chinese has one form, so both keys usually translate the same). `{n}` is
 *  filled with `n`. */
export function tn(n: number, one: string, many: string, vars?: Record<string, string | number>): string {
  return t(n === 1 ? one : many, { n, ...vars });
}
