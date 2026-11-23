import type { Advisor, Score } from "./api";

/** The characters Instinct plays: the all-character student's five (GH #1197)
 *  on the build it was trained on, and the Fool, whose own student plays
 *  v0.113.0 (GH #1535). */
export const E2E_CHARACTERS = ["Ironclad", "Silent", "Defect", "Regent", "Necrobinder", "Fool"] as const;
/** The character Instinct falls back to when the picked one is not its. */
export const E2E_CHARACTER = "Ironclad";
/** The all-character student's build. */
export const E2E_GAME_VERSION = "v0.111.0";
/** The Fool's student's build. */
export const E2E_FOOL_GAME_VERSION = "v0.113.0";

/** Whether Instinct plays `character`. */
export function isE2eCharacter(character: string): boolean {
  return (E2E_CHARACTERS as readonly string[]).includes(character);
}

/** The build Instinct plays `character` on (its student's). */
export function e2eGameVersion(character: string): string {
  return character.trim().toLowerCase() === "fool" ? E2E_FOOL_GAME_VERSION : E2E_GAME_VERSION;
}

/** Whether the e2e student can advise a run of `character` on `gameVersion`
 *  played on `backend` (one of its characters on that student's build, Rust
 *  engine only). */
export function e2eAdvisable(
  e2eAvailable: boolean, character: string, gameVersion: string | null | undefined, backend = "rust",
): boolean {
  return e2eAvailable && backend === "rust" && isE2eCharacter(character)
    && gameVersion === e2eGameVersion(character);
}

/** localStorage key of the hand-played run's advisor preference. */
export const ADVISOR_KEY = "wb.advisor";

/** The stored advisor preference. A fresh browser prefers Instinct, as the
 *  mod does since 0.10 (it advises only a run it can read; every other run is
 *  the search's whatever the preference). "search" is kept once chosen. */
export function readAdvisorPref(): Advisor {
  try {
    const v = localStorage.getItem(ADVISOR_KEY);
    return v === "search" ? "search" : v === "e2e" || v == null ? "e2e" : "search";
  } catch {
    return "e2e";
  }
}

/** Whether combat `scores` are the e2e advisor's (policy probabilities,
 *  `prob` on every row) rather than the search's (win chances + visits). */
export function isE2eScores(scores: Score[]): boolean {
  return scores.length > 0 && scores.every((s) => typeof s.prob === "number");
}
