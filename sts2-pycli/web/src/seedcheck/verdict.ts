/**
 * The seed checker's text verdict, for pasting into a thread. It always
 * names the conditions (character, ascension, build, "simulator") and never
 * claims more than was measured: no "impossible", a bound instead. A run's
 * line carries its route floor by floor (which map node, what was there,
 * what was chosen, HP on leaving), so a reader can follow it. Paragraphs are
 * blank-line separated and the route is a "- " list, which Reddit and Discord
 * markdown keep apart.
 */
import { upperBound95, wilson95, type AttemptResult, type FloorDeaths, type Tally } from "./stats";
import { getLang, t, tn } from "../i18n";

/** `POST /api/seedcheck/summary`. */
export interface Summary {
  won: boolean;
  outcome: string;
  hp: number;
  max_hp: number;
  floor: number;
  neow_pick: string | null;
  boss_relics: string[];
  events: { floor: number; event: string; option: string }[];
  cards_taken: { floor: number; card: string }[];
  shop: { floor: number; bought?: string; removed?: string }[];
  rests: { floor: number; choice: string; card?: string }[];
  /** One entry per map node walked, in order (`seedcheck::line_summary`). */
  route?: RouteStep[];
  deck: string[];
  relics: string[];
  death: { floor: number; act: number; room: string | null } | null;
}

/** A map node the run walked through. */
export interface RouteStep {
  act: number;
  floor: number;
  /** The map's node type: Monster, Elite, Unknown (a ?), RestSite, Shop, Treasure, Boss. */
  room: string;
  /** Its place in its row, counted from the left (1-based), of `of`. */
  pos: number;
  of: number;
  /** From the previous node; null at an act's first node and for a boss. */
  dir: "left" | "straight" | "right" | null;
  /** How many nodes the move could have reached. */
  options: number;
  /** The fight's encounter or the event, when there was one. */
  name: string | null;
  /** What the node turned out to be (a "?" can be either). */
  kind?: "fight" | "event" | null;
  notes: string[];
  hp?: number;
  max_hp?: number;
}

/** What the run was: shown in every header. */
export interface Conditions {
  character: string;
  ascension: number;
  version: string;
}

export const DEFAULT_CONDITIONS: Conditions = { character: "Ironclad", ascension: 10, version: "v0.111.0" };

const ROOM: Record<string, string> = {
  Monster: "Fight",
  Elite: "Elite",
  Unknown: "?",
  RestSite: "Rest",
  Shop: "Shop",
  Treasure: "Chest",
  Boss: "Boss",
  Ancient: "Ancient",
};

function ordinal(n: number): string {
  if (getLang() === "zh") return `第${n}个`;
  const s = n % 100 >= 11 && n % 100 <= 13 ? "th" : ({ 1: "st", 2: "nd", 3: "rd" } as Record<number, string>)[n % 10] ?? "th";
  return `${n}${s}`;
}

/** Which node to take, in words a player can act on. */
function whereTo(r: RouteStep): string {
  if (r.room === "Boss") return "";
  const place = t("{nth} of {of} from the left", { nth: ordinal(r.pos), of: r.of });
  if (r.dir === null) return t("start on the {place}", { place });
  if (r.options <= 1) return t("only path ({place})", { place });
  return t({ left: "go left ({place})", straight: "go straight ({place})", right: "go right ({place})" }[r.dir], { place });
}

/** The route as one "- " line per floor, grouped by act. */
export function routeLines(route: RouteStep[]): string[] {
  const out: string[] = [];
  let act = -1;
  for (const r of route) {
    if (r.act !== act) {
      act = r.act;
      if (out.length) out.push("");
      out.push(t("Act {n}:", { n: act }), "");
    }
    const where = whereTo(r);
    const what = [r.name, ...r.notes].filter(Boolean).join("; ");
    const hp = r.hp !== undefined && r.max_hp !== undefined ? ` ${t("(HP {hp}/{max})", { hp: r.hp, max: r.max_hp })}` : "";
    // ROOM values and r.kind ("fight" / "event") are closed sets listed in zh/simlab.ts.
    const base = ROOM[r.room] ? t(ROOM[r.room]) : r.room;
    const room = r.room === "Unknown" && r.kind ? `${base} (${t(r.kind)})` : base;
    out.push(`- F${r.floor} ${room}${where ? `, ${where}` : ""}${what ? `: ${what}` : ""}${hp}`);
  }
  return out;
}

const STARTERS = new Set(["Strike", "Defend", "Bash", "Ascender's Bane"]);
const CAVEAT =
  "Simulator result: following it in the real game only works where the engine matches the game on this seed. " +
  "Your fights will go differently from the bot's, so HP and some rewards can differ from the route.";

const ROUTE_HELP =
  "Route: one line per floor. \"Go left/straight/right\" is from the node you stand on; \"Nth of M from the left\" counts that floor's nodes on the map. HP is when leaving the floor.";

function header(seed: string, build: string, c: Conditions): string {
  return t("Seed {seed} — {character} A{asc}, {version} (simulator, workbench {build})", { seed, character: c.character, asc: c.ascension, version: c.version, build });
}

/** 0.0679 → "6.8", 0.18 → "18". */
function pct(x: number, digits = 1): string {
  return String(Number((x * 100).toFixed(digits)));
}

/** A bound rounded UP so it never claims more than was measured. */
function pctUp(x: number): string {
  return String(Math.ceil(x * 1000) / 10);
}

function deathLine(deaths: FloorDeaths[], label: string): string | null {
  if (!deaths.length) return null;
  const parts = deaths.slice(0, 3).map((d) => t("floor {floor} ({room}, {count})", { floor: d.floor, room: d.room ?? t("unknown"), count: d.count }));
  return t("{label}: {parts}.", { label, parts: parts.join(", ") });
}

function deckHighlights(deck: string[]): string {
  const counts = new Map<string, number>();
  for (const c of deck) if (!STARTERS.has(c)) counts.set(c, (counts.get(c) ?? 0) + 1);
  return [...counts.entries()]
    .slice(0, 12)
    .map(([c, n]) => (n > 1 ? `${c} ×${n}` : c))
    .join(", ");
}

function winningLine(s: Summary): string[] {
  const out: string[] = [];
  if (s.neow_pick) out.push(t("Neow: {pick}.", { pick: s.neow_pick }));
  if (s.boss_relics.length) out.push(t("Boss relics: {relics}.", { relics: s.boss_relics.join(", ") }));
  // Events and shops are in the route, floor by floor; without one (an older
  // summary) name them here.
  if (!s.route?.length) {
    if (s.events.length) {
      out.push(t("Events: {events}.", { events: s.events.map((e) => `${e.event} → ${e.option} ${t("(floor {n})", { n: e.floor })}`).join("; ") }));
    }
    const bought = s.shop.filter((x) => x.bought).map((x) => x.bought);
    const removed = s.shop.filter((x) => x.removed).map((x) => x.removed);
    if (bought.length || removed.length) {
      const bits = [bought.length ? t("bought {cards}", { cards: bought.join(", ") }) : "", removed.length ? t("removed {cards}", { cards: removed.join(", ") }) : ""];
      out.push(t("Shops: {bits}.", { bits: bits.filter(Boolean).join("; ") }));
    }
  }
  const deck = deckHighlights(s.deck);
  if (deck) out.push(t("Deck: {deck}.", { deck }));
  out.push(t("Ended with {hp}/{max} HP.", { hp: s.hp, max: s.max_hp }));
  return out;
}

export function checkVerdict(p: {
  seed: string;
  build: string;
  conditions?: Conditions;
  tally: Tally;
  firstWin: AttemptResult | null;
  summary: Summary | null;
  deaths: FloorDeaths[];
}): string {
  const lines = [header(p.seed, p.build, p.conditions ?? DEFAULT_CONDITIONS)];
  const counted = p.tally.played - p.tally.errored;
  if (p.firstWin) {
    const how = p.firstWin.temp === 0 ? t("greedy") : t("sampled");
    const n = p.tally.played;
    lines.push(
      tn(n, "Winnable: the bot won on attempt {k} ({how}); {n} attempt played.", "Winnable: the bot won on attempt {k} ({how}); {n} attempts played.", { k: p.firstWin.k, how }),
    );
    if (p.summary) lines.push(t("Winning line (overview): {line}", { line: winningLine(p.summary).join(" ") }));
  } else if (counted === 0) {
    lines.push(t("No attempt finished, so there is no result for this seed."));
  } else {
    lines.push(
      t("No win in {n} attempts: the bot's chance per attempt is below {pct}% (95% confidence).", { n: counted, pct: pctUp(upperBound95(counted)) }),
    );
    lines.push(t("The best attempt reached floor {n}.", { n: p.tally.bestFloor }));
    const d = deathLine(p.deaths, t("Most attempts died on"));
    if (d) lines.push(d);
  }
  const route = p.summary?.route ?? [];
  if (route.length) {
    // Both forms of ROUTE_HELP are listed in zh/simlab.ts.
    lines.push(t(p.firstWin ? ROUTE_HELP : ROUTE_HELP.replace("Route:", "The best attempt's route:")));
    lines.push(routeLines(route).join("\n"));
  }
  if (p.tally.errored) {
    const e = p.tally.errored;
    lines.push(tn(e, "({n} attempt ended in an engine error and is not counted.)", "({n} attempts ended in an engine error and are not counted.)"));
  }
  lines.push(t(CAVEAT));
  return lines.join("\n\n");
}

export function rateVerdict(p: {
  seed: string;
  build: string;
  conditions?: Conditions;
  wins: number;
  n: number;
  deaths: FloorDeaths[];
  errored?: number;
}): string {
  const [lo, hi] = wilson95(p.wins, p.n);
  const lines = [
    header(p.seed, p.build, p.conditions ?? DEFAULT_CONDITIONS),
    t("The bot won {wins} of {n} sampled runs ({pct}%, 95% interval {lo}–{hi}%).", { wins: p.wins, n: p.n, pct: pct(p.n ? p.wins / p.n : 0), lo: pct(lo, 0), hi: pct(hi, 0) }),
  ];
  const d = deathLine(p.deaths, t("Most losses"));
  if (d) lines.push(d);
  if (p.errored) lines.push(t("({n} more ended in an engine error and are not counted.)", { n: p.errored }));
  lines.push(t(CAVEAT));
  return lines.join("\n\n");
}
