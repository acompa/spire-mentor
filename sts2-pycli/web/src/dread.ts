/** Dread mode (GH #1346): an ascension-10 run on v0.113.0 with a per-act
 *  dominion, one more Ascender's Bane on entering acts 2 and 3, and stronger
 *  bosses. Only the Rust engine builds one. */

import { blitzUnavailableReason } from "./blitz";
import { t } from "./i18n";

/** The modes a run can be started in: Dread here, Blitz in `blitz.ts`
 *  (GH #1347). */
export type GameMode = "standard" | "dread" | "blitz";

/** The one game build with Dread. */
export const DREAD_GAME_VERSION = "v0.113.0";

/** The game's Dread screen always embarks at ascension 10. */
export const DREAD_ASCENSION = 10;

/** Why a Dread run cannot be started on this engine and build, or `null`
 *  when it can. One short sentence, shown beside the disabled control. */
export function dreadUnavailableReason(backend: string, gameVersion: string): string | null {
  if (backend !== "rust") return t("Dread needs the Rust engine.");
  if (gameVersion !== DREAD_GAME_VERSION) return t("Dread needs game build {v}.", { v: DREAD_GAME_VERSION });
  return null;
}

/** Why `mode` cannot be started on this engine and build, or `null` when
 *  it can. A standard run always can. */
export function gameModeUnavailableReason(mode: GameMode, backend: string, gameVersion: string): string | null {
  if (mode === "dread") return dreadUnavailableReason(backend, gameVersion);
  if (mode === "blitz") return blitzUnavailableReason(backend, gameVersion);
  return null;
}

/** The mode a new run would actually start in: a remembered Dread or Blitz
 *  pick applies only where that mode can run. The pick itself is left
 *  alone, so switching the build away and back keeps it. */
export function effectiveGameMode(pick: GameMode, backend: string, gameVersion: string): GameMode {
  return gameModeUnavailableReason(pick, backend, gameVersion) === null ? pick : "standard";
}

/** The request-body fragment for `mode`: only a Dread or Blitz start names
 *  it, so a standard start is the request it always was. */
export function gameModeBody(mode: GameMode): { game_mode?: "dread" | "blitz" } {
  return mode === "standard" ? {} : { game_mode: mode };
}

/** Whether a state belongs to a Dread run (`state.game_mode`; a standard
 *  run's state carries no such key). */
export function isDread(state: { game_mode?: unknown } | null | undefined): boolean {
  return state?.game_mode === "dread";
}

/** Boss encounters whose title carries an article the dominion's entry
 *  drops (`THE_KIN_BOSS` -> `KIN_DOMINION`). */
const BOSS_TITLES: Record<string, string> = {
  KIN: "The Kin",
  INSATIABLE: "The Insatiable",
};

/** The boss a dominion belongs to, as the game titles the encounter:
 *  `KAISER_CRAB_DOMINION` -> `Kaiser Crab`. Every v0.113.0 boss title is its
 *  id in title case (checked against `encounters.json`), so this needs no
 *  localization table. */
export function dominionBoss(entry: string): string {
  const stem = entry.replace(/_DOMINION$/, "");
  // The English title is also the key of the game's Chinese one (`zh/game.ts`).
  return t(BOSS_TITLES[stem] ?? stem
    .split("_")
    .filter(Boolean)
    .map((w) => w[0].toUpperCase() + w.slice(1).toLowerCase())
    .join(" "));
}

/** A dominion's display name, the game's `DREAD.title`
 *  (`{BossName}'s Dominion`). */
export function dominionTitle(entry: string): string {
  return t("{boss}'s Dominion", { boss: dominionBoss(entry) });
}
