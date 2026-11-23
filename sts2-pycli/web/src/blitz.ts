/** Blitz mode (GH #1347): a v0.113.0 run of three short acts. Each act is a
 *  fixed line of eight rooms with no map to choose from (Ancient, two fights,
 *  the merchant, an elite, a chest, a rest site, the boss), Neow hands out
 *  three of ten cards, rooms pay no gold, the merchant gives three free
 *  wares and the chest holds two relics. Only the Rust engine builds one. */

import { ASCENSION_LEVELS, type AscensionLevel } from "./ascension";
import { getLang, t, tn } from "./i18n";

/** The one game build with Blitz. */
export const BLITZ_GAME_VERSION = "v0.113.0";

/** The top of the Blitz ladder (`AscensionManager.maxAscensionAllowedBlitz`). */
export const BLITZ_ASCENSION_MAX = 5;

/** Blitz level i (1..5) as the standard level it switches on
 *  (`AscensionHelper.GetLevelForBlitz`): Weary Traveler, Tight Belt, Tough
 *  Enemies, Deadly Enemies, Ascender's Bane. The other standard levels
 *  (Swarming Elites, Poverty, Inflation, Scarcity, Double Boss) never apply. */
const BLITZ_STANDARD_LEVELS = [2, 4, 8, 9, 5] as const;

/** The Blitz ladder, with the game's own names and descriptions: the entry
 *  at index i is Blitz level i. */
export const BLITZ_LEVELS: readonly AscensionLevel[] = [
  { ...ASCENSION_LEVELS[0] },
  ...BLITZ_STANDARD_LEVELS.map((standard, i) => ({ ...ASCENSION_LEVELS[standard], level: i + 1 })),
];

/** The level's own entry, clamped into the Blitz ladder. */
export function blitzLevel(level: number): AscensionLevel {
  const k = Math.max(0, Math.min(BLITZ_ASCENSION_MAX, Math.round(level)));
  return BLITZ_LEVELS[k];
}

/** Every modifier in force at Blitz `level` (1..level), highest first. Empty
 *  at 0. */
export function blitzModifiers(level: number): AscensionLevel[] {
  const k = blitzLevel(level).level;
  return BLITZ_LEVELS.slice(1, k + 1).reverse();
}

/** Why a Blitz run cannot be started on this engine and build, or `null`
 *  when it can. One short sentence, shown beside the disabled control. */
export function blitzUnavailableReason(backend: string, gameVersion: string): string | null {
  if (backend !== "rust") return t("Blitz needs the Rust engine.");
  if (gameVersion !== BLITZ_GAME_VERSION) return t("Blitz needs game build {v}.", { v: BLITZ_GAME_VERSION });
  return null;
}

/** Whether a state belongs to a Blitz run (`state.game_mode`; a standard
 *  run's state carries no such key). */
export function isBlitz(state: { game_mode?: unknown } | null | undefined): boolean {
  return state?.game_mode === "blitz";
}

/** A shown step, as far as the AI's reach reads it. */
export interface AiStep {
  decision?: string | null;
  state?: { game_mode?: unknown; enemies?: unknown } | null;
}

/** Whether a step is a fight's: a card play, or a choice a card or potion
 *  opened inside one. The engine keeps the live enemies on such a choice
 *  (`Game::serialize`); the merchant's deck pick and Neow's card grid carry
 *  none. */
export function isBlitzFight(step: AiStep | null | undefined): boolean {
  if (!step) return false;
  if (step.decision === "combat_play") return true;
  const enemies = step.state?.enemies;
  return step.decision === "card_select" && Array.isArray(enemies) && enemies.length > 0;
}

/** What the AI can be asked about the step on screen. */
export interface AiReach {
  /** The advisors: the hint, the search tree, peek, "let the AI pick", the
   *  in-fight choice ranking, the turn grade, autoplay of the fight. */
  advisors: boolean;
  /** The run's agent: the run policy, play-to-end, autobattle, Watch. */
  runMode: boolean;
  /** Every advice surface reads the Hints switch as quiet. */
  quiet: boolean;
}

/** The AI's reach on `step` (GH #1347). A standard or Dread run has what the
 *  backend's feature flags say. A Blitz run has the advisors in a fight and
 *  nothing else: a fight is the same game in every mode, but no run policy
 *  knows the Blitz merchant or its map, and the server answers their routes
 *  409. A recorded Blitz run has none: only the live routes were opened. */
export function aiReach(
  features: { advisors?: boolean; run_mode?: boolean },
  step: AiStep | null | undefined,
  replay: boolean,
): AiReach {
  const advisors = features.advisors !== false;
  const runMode = features.run_mode !== false;
  if (!isBlitz(step?.state)) return { advisors, runMode, quiet: false };
  const fight = !replay && isBlitzFight(step);
  return { advisors: advisors && fight, runMode: false, quiet: !fight };
}

/** The flame's tooltip in a Blitz run: the level and the modifiers it
 *  stacks, by name. The number is a Blitz level, not a standard ascension. */
export function blitzAscensionTitle(level: number): string {
  const mods = blitzModifiers(level);
  if (mods.length === 0) return t("Blitz ascension 0 · no modifiers");
  // The level titles are the game's (`zh/game.ts`); Chinese lists with "、".
  const titles = mods.map((m) => t(m.title)).join(getLang() === "zh" ? "、" : ", ");
  return `${t("Blitz ascension {n}", { n: blitzLevel(level).level })} · ${titles}`;
}

/** One ware of the Blitz merchant, as the server lists it
 *  (`state.blitz_wares`). `index` is what `blitz_choice` takes. */
export interface BlitzWare {
  index: number;
  /** The game's `BlitzMerchantEntryType` member (`"TwoRelics"`). */
  kind: string;
  title: string;
  /** The game's own text, rich-text tags kept. */
  description: string;
  /** One of the six wares that also hand out a Curse. */
  cursed: boolean;
  /** One of the three wares the merchant rolls at half weight. */
  rare: boolean;
  status: "Available" | "Used" | "Disabled";
  enabled: boolean;
}

/** One relic of the Blitz chest (`state.rewards`). `index` is what
 *  `take_reward` takes; it does not renumber between rounds. */
export interface ChestReward {
  index: number;
  type: string;
  id?: string;
  name: string;
  description?: string;
  image?: string | null;
}

/** The merchant's line under its wares (`MERCHANT.blitzSelection.*`). */
export function blitzPicksLine(remaining: number): string {
  if (remaining <= 0) return t("[red]You can't choose any more items.[/red]");
  return tn(remaining, "You may choose [blue]{n}[/blue] more item.", "You may choose [blue]{n}[/blue] more items.");
}
