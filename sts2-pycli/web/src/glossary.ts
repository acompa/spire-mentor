/** The game's own explanations of the terms it names (GH #1599, #1600): the
 *  enchantments, the card keywords and the static hover tips, one table per
 *  game build (`public/<ver>/glossary.json`, written by
 *  `python -m workbench.gen_glossary` from the build's localization).
 *
 *  The real game explains a term wherever it is named: hovering "Adroit" in
 *  an event option shows what Adroit does, and an enchanted card in the deck
 *  carries the enchantment's tab and its extra line of text. The workbench
 *  showed the bare words. This module is the data behind both. */

import { useSyncExternalStore } from "react";
import { localizedPublicPath, versionedPublicPath } from "./gameVersion";
import { t } from "./i18n";

export interface GlossaryEnchantment {
  title: string;
  /** The hover tip, a template (`{Amount}`, `{Block:diff()}`…). */
  description: string;
  /** The line an enchanted card adds to its own text (`extraCardText`). */
  extra?: string | null;
  /** The game draws the amount on the card's tab (C# `ShowAmount`). */
  show_amount: boolean;
  /** The icon, relative to the art root. */
  image: string;
}

export interface GlossaryTip {
  title: string;
  description: string;
}

export interface GlossaryOwned extends GlossaryTip {
  image: string;
}

export interface Glossary {
  enchantments: Record<string, GlossaryEnchantment>;
  keywords: Record<string, GlossaryTip>;
  tips: Record<string, GlossaryTip>;
  relics?: Record<string, GlossaryOwned>;
  potions?: Record<string, GlossaryOwned>;
  /** Each card's upgraded face: its text (markup kept) and, when the
   *  upgrade changes it, its cost. */
  upgrades?: Record<string, { description: string; cost?: number | string }>;
}

const EMPTY: Glossary = { enchantments: {}, keywords: {}, tips: {} };

let current: Glossary = EMPTY;
let currentVersion: string | null | undefined;
const listeners = new Set<() => void>();
const cache = new Map<string, Promise<Glossary>>();

function fetchTable(url: string): Promise<Glossary | null> {
  return fetch(url)
    .then((r) => (r.ok ? (r.json() as Promise<Glossary>) : null))
    .catch(() => null);
}

/** The build's table in the page's language (`public/zh/…` in Chinese),
 *  else the English one (a build with no Chinese table). */
function load(version: string | null | undefined): Promise<Glossary> {
  const url = localizedPublicPath("glossary.json", version);
  let p = cache.get(url);
  if (!p) {
    const english = versionedPublicPath("glossary.json", version);
    p = fetchTable(url)
      .then((g) => g ?? (url === english ? null : fetchTable(english)))
      .then((g) => g ?? EMPTY);
    cache.set(url, p);
  }
  return p;
}

/** Point the glossary at game build `version` (the run on screen). */
export function setGlossaryVersion(version: string | null | undefined): void {
  if (version === currentVersion) return;
  currentVersion = version;
  void load(version).then((g) => {
    if (currentVersion !== version) return;
    current = g;
    listeners.forEach((l) => l());
  });
}

/** For tests: install a table directly. */
export function setGlossaryForTest(g: Glossary): void {
  current = g;
  listeners.forEach((l) => l());
}

function subscribe(l: () => void): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

/** The glossary of the build on screen (empty until it loads). */
export function useGlossary(): Glossary {
  return useSyncExternalStore(subscribe, () => current, () => current);
}

/** Fill an enchantment template with its amount. Every enchantment's numbers
 *  are its `Amount` (`RecalculateValues` sets the named var from it: Adroit's
 *  `Block`, Inky's `WeakPower`), so each `{Var}` / `{Var:diff()}` reads
 *  `amount`; `{Var:plural:a|b}` picks a form, `{Var:energyIcons()}` reads
 *  "N Energy", and a `{X:choose(…):a|b}` keeps its default branch. */
export function fillTemplate(text: string, amount: number | string | null | undefined): string {
  const n = amount ?? "X";
  return text.replace(/\{([A-Za-z]+)(?::([^{}]*))?\}/g, (_m, _name: string, fmt?: string) => {
    if (!fmt || fmt === "diff()") return String(n);
    if (fmt.startsWith("plural:")) {
      const [one, many] = fmt.slice("plural:".length).split("|");
      return n === 1 ? one : (many ?? one);
    }
    if (fmt === "energyIcons()") return t("{n} Energy", { n });
    if (fmt.startsWith("choose(")) {
      const bar = fmt.lastIndexOf("|");
      return bar >= 0 ? fmt.slice(bar + 1) : "";
    }
    return String(n);
  });
}

/** A card's enchantment as the card and its tip draw it, or null. */
export interface CardEnchantment {
  id: string;
  title: string;
  amount: number;
  showAmount: boolean;
  image: string;
  /** The hover tip, filled. */
  description: string;
  /** The card's extra line, filled, or null. */
  extra: string | null;
}

export function cardEnchantment(
  g: Glossary,
  card: { enchantment?: string | null; enchantment_amount?: number | null },
): CardEnchantment | null {
  const id = card.enchantment;
  if (!id) return null;
  const e = g.enchantments[id];
  const amount = card.enchantment_amount ?? 0;
  const title = e?.title ?? id.toLowerCase().replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
  return {
    id,
    title,
    amount,
    showAmount: e?.show_amount ?? amount > 1,
    image: e?.image ?? `enchantments/${id.toLowerCase()}.png`,
    description: e ? fillTemplate(e.description, amount) : "",
    extra: e?.extra ? fillTemplate(e.extra, amount) : null,
  };
}

/** What a hover tip over a named term shows. */
export interface TermInfo {
  title: string;
  description: string;
  image?: string;
}

/** The glossary entry a coloured run of game text names, or null: an
 *  enchantment ("Adroit", "Swift 2" — the amount fills its text; else
 *  `amount`, the number right after the run, when there is one), a relic, a
 *  potion, a card keyword or a static tip. Plural and past forms
 *  ("Exhausted", "Relics") fall back to the base word. */
export function lookupTerm(g: Glossary, text: string, amount?: number | null): TermInfo | null {
  const m = text.trim().match(/^(.*?)(?:\s+(\d+))?$/);
  if (!m || !m[1]) return null;
  const name = m[1].toLowerCase();
  const n = m[2] != null ? Number(m[2]) : amount ?? null;
  const forms = [name, name.replace(/s$/, ""), name.replace(/ed$/, ""), name.replace(/d$/, "")];
  for (const f of forms) {
    for (const e of Object.values(g.enchantments)) {
      if (e.title.toLowerCase() !== f) continue;
      return {
        title: n != null && e.show_amount ? `${e.title} ${n}` : e.title,
        description: fillTemplate(e.description, n),
        image: e.image,
      };
    }
    for (const table of [g.relics, g.potions, g.keywords, g.tips]) {
      for (const e of Object.values(table ?? {})) {
        if (e.title.toLowerCase() === f) {
          return { title: e.title, description: e.description, image: (e as { image?: string }).image };
        }
      }
    }
  }
  return null;
}

/** The upgraded face of `card` (GH #1598): the build's `upgrades` entry's
 *  text and cost, the numbers the upgrade changed painted green as the
 *  game's "View upgrades" does. `card` itself when it is already upgraded or
 *  cannot upgrade. */
export function upgradedTwin<C extends {
  id: string; upgraded?: boolean; cost?: number | string | null;
  description?: string; description_rich?: string;
}>(card: C, g: Glossary): C {
  if (card.upgraded) return card;
  const up = g.upgrades?.[card.id];
  if (!up) return card;
  const before = card.description_rich ?? card.description ?? "";
  const rich = markChanged(before, up.description);
  return {
    ...card,
    upgraded: true,
    cost: up.cost ?? card.cost,
    description_rich: rich,
    description: up.description.replace(/\[\/?[a-z]+\]/g, ""),
  };
}

/** `after` with every number token that differs from `before`'s token in
 *  the same position painted `[green]`, when the two read alike word for
 *  word; else `after` as is. */
function markChanged(before: string, after: string): string {
  const strip = (s: string) => s.replace(/\[\/?[a-z]+\]/g, "");
  const a = strip(before).split(/(\s+)/);
  const b = after.split(/(\s+)/);
  const plainB = b.map(strip);
  if (a.length !== b.length) return after;
  return b.map((tok, i) => (/\d/.test(plainB[i]) && plainB[i] !== a[i] ? `[green]${plainB[i]}[/green]` : tok)).join("");
}

let viewUpgrades = (() => {
  try { return localStorage.getItem("wb.viewUpgrades") === "1"; } catch { return false; }
})();
const upgradeListeners = new Set<() => void>();

/** The "View upgrades" switch, shared by every card grid and remembered. */
export function useViewUpgrades(): [boolean, (on: boolean) => void] {
  const on = useSyncExternalStore(
    (l) => { upgradeListeners.add(l); return () => upgradeListeners.delete(l); },
    () => viewUpgrades, () => viewUpgrades,
  );
  return [on, (next: boolean) => {
    viewUpgrades = next;
    try { localStorage.setItem("wb.viewUpgrades", next ? "1" : "0"); } catch { /* private mode */ }
    upgradeListeners.forEach((l) => l());
  }];
}
