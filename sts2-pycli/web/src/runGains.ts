import type { Card, CombatState, Step } from "./api";
import { t } from "./i18n";

/**
 * What a run-level step just handed you (or took away): the relics, potions,
 * cards, gold and HP that changed between two consecutive live states.
 *
 * The game shows every one of these as it happens — a relic flies to the
 * bar, a card to the deck, gold ticks up — but the workbench only moved to
 * the next screen. When the AI made the pick, or an event resolved on its
 * own, the only way to learn what happened was to open the log or count the
 * relics. This is the data behind the banner that says it instead.
 *
 * Fights are left out on purpose: a turn changes HP, potions and gold all
 * the time and the board already shows it. A step FROM a fight (the last
 * blow) is skipped too, since the reward screen that follows lists the
 * spoils.
 */

export type GainKind = "relic" | "potion" | "card" | "gold" | "hp" | "max_hp";

export interface Gain {
  kind: GainKind;
  /** `+` gained / raised, `-` lost / lowered, `^` upgraded, `*` enchanted
   *  (cards only). */
  sign: "+" | "-" | "^" | "*";
  text: string;
  image?: string | null;
  /** The card as it now is (or was, for a removal), for a card's gain: the
   *  banner draws it. */
  card?: Card;
}

type Player = CombatState["player"];

/** A fight's own step: the board, or a mid-fight card pick (a card_select
 *  with enemies standing — a run-level "choose a card" has none). */
function inFight(step: Step): boolean {
  if (step.decision === "combat_play") return true;
  const enemies = (step.state as { enemies?: unknown[] }).enemies;
  return step.decision === "card_select" && (enemies?.length ?? 0) > 0;
}

/** A multiset as name → count. */
function counts<T>(xs: T[], key: (x: T) => string): Map<string, { n: number; x: T }> {
  const m = new Map<string, { n: number; x: T }>();
  for (const x of xs) {
    const k = key(x);
    const e = m.get(k);
    if (e) e.n += 1;
    else m.set(k, { n: 1, x });
  }
  return m;
}

/** `a − b` as a multiset: every entry `a` holds more of than `b`. */
function minus<T>(a: Map<string, { n: number; x: T }>, b: Map<string, { n: number; x: T }>): T[] {
  const out: T[] = [];
  for (const [k, { n, x }] of a) {
    const extra = n - (b.get(k)?.n ?? 0);
    for (let i = 0; i < extra; i++) out.push(x);
  }
  return out;
}

function upgradeLevel(c: Card): number {
  return c.upgrade_level ?? (c.upgraded ? 1 : 0);
}

export function cardLabel(c: Card): string {
  const lvl = upgradeLevel(c);
  return lvl === 0 ? c.name : lvl === 1 ? `${c.name}+` : `${c.name}+${lvl}`;
}

function enchantLabel(c: Card): string {
  // The engine's Chinese name in Chinese mode, else the id title-cased.
  const name = c.enchantment_name
    ?? (c.enchantment ?? "").toLowerCase().replace(/_/g, " ").replace(/\b\w/g, (x) => x.toUpperCase());
  return c.enchantment_amount ? `${name} ${c.enchantment_amount}` : name;
}

function cardGains(before: Card[], after: Card[]): Gain[] {
  const key = (c: Card) => `${c.id}#${upgradeLevel(c)}#${c.enchantment ?? ""}#${c.enchantment_amount ?? ""}`;
  const added = minus(counts(after, key), counts(before, key));
  const removed = minus(counts(before, key), counts(after, key));
  const out: Gain[] = [];
  // An upgrade is the same card leaving at one level and arriving one higher;
  // an enchantment, the same card arriving enchanted.
  for (const a of [...added]) {
    const i = removed.findIndex((r) => r.id === a.id
      && (upgradeLevel(r) < upgradeLevel(a) || (!!a.enchantment && a.enchantment !== r.enchantment)));
    if (i < 0) continue;
    const [r] = removed.splice(i, 1);
    added.splice(added.indexOf(a), 1);
    if (a.enchantment && a.enchantment !== r.enchantment) {
      out.push({ kind: "card", sign: "*", text: `${cardLabel(a)} · ${enchantLabel(a)}`, card: a });
    } else {
      out.push({ kind: "card", sign: "^", text: cardLabel(a), card: a });
    }
  }
  for (const c of added) out.push({ kind: "card", sign: "+", text: cardLabel(c), card: c });
  for (const c of removed) out.push({ kind: "card", sign: "-", text: cardLabel(c), card: c });
  return out;
}

function named<T extends { name: string; image?: string | null }>(
  kind: "relic" | "potion", before: T[], after: T[],
): Gain[] {
  const key = (x: T) => x.name;
  const a = counts(after, key);
  const b = counts(before, key);
  return [
    ...minus(a, b).map((x): Gain => ({ kind, sign: "+", text: x.name, image: x.image })),
    ...minus(b, a).map((x): Gain => ({ kind, sign: "-", text: x.name, image: x.image })),
  ];
}

function delta(kind: "gold" | "hp" | "max_hp", before?: number, after?: number): Gain[] {
  if (before == null || after == null || before === after) return [];
  const d = after - before;
  const n = `${d > 0 ? "+" : "−"}${Math.abs(d)}`;
  const text = kind === "gold" ? t("{n} gold", { n }) : kind === "hp" ? t("{n} HP", { n }) : t("{n} Max HP", { n });
  return [{ kind, sign: d > 0 ? "+" : "-", text }];
}

/** The changes between two consecutive live steps, or `[]` when there are
 *  none worth a banner (either side in a fight, or the chest panel already
 *  shows them). Ordered: relics, cards, potions, then the numbers. */
export function runGains(prev: Step | null | undefined, next: Step | null | undefined): Gain[] {
  if (!prev || !next) return [];
  if (inFight(prev) || inFight(next)) return [];
  // The chest reveal is its own panel (TreasureReveal); saying it twice is noise.
  if (next.state.room_reveal) return [];
  const p: Player | undefined = prev.state.player;
  const n: Player | undefined = next.state.player;
  if (!p || !n) return [];
  return [
    ...named("relic", p.relics ?? [], n.relics ?? []),
    ...cardGains(p.deck ?? [], n.deck ?? []),
    ...named("potion", (p.potions ?? []).filter((x) => x.name), (n.potions ?? []).filter((x) => x.name)),
    ...delta("max_hp", p.max_hp, n.max_hp),
    ...delta("hp", p.hp, n.hp),
    ...delta("gold", p.gold, n.gold),
  ];
}
