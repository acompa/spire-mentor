import { describe, it, expect } from "vitest";
import {
  BLITZ_ASCENSION_MAX, BLITZ_GAME_VERSION, BLITZ_LEVELS, blitzAscensionTitle, blitzLevel, blitzModifiers,
  aiReach, blitzPicksLine, blitzUnavailableReason, isBlitz, isBlitzFight,
} from "../blitz";
import { ASCENSION_LEVELS } from "../ascension";
import { effectiveGameMode, gameModeBody, gameModeUnavailableReason } from "../dread";

describe("blitz", () => {
  it("is only offered on the Rust engine at v0.113.0", () => {
    expect(BLITZ_GAME_VERSION).toBe("v0.113.0");
    expect(blitzUnavailableReason("rust", "v0.113.0")).toBeNull();
    expect(blitzUnavailableReason("rust", "v0.111.0")).toBe("Blitz needs game build v0.113.0.");
    expect(blitzUnavailableReason("csharp", "v0.113.0")).toBe("Blitz needs the Rust engine.");
  });

  // `AscensionHelper.GetLevelForBlitz`: Blitz level i switches on one
  // standard level, and the ladder stops at 5.
  it("has a five-level ladder of its own, named as the game names it", () => {
    expect(BLITZ_ASCENSION_MAX).toBe(5);
    expect(BLITZ_LEVELS.map((l) => l.level)).toEqual([0, 1, 2, 3, 4, 5]);
    expect(BLITZ_LEVELS.map((l) => l.title)).toEqual([
      "No Ascension", "Weary Traveler", "Tight Belt", "Tough Enemies", "Deadly Enemies", "Ascender's Bane",
    ]);
    // The text is the standard level's own.
    expect(BLITZ_LEVELS[3].description).toBe(ASCENSION_LEVELS[8].description);
    expect(BLITZ_LEVELS[5].description).toBe(ASCENSION_LEVELS[5].description);
    // The standard ladder is not touched.
    expect(ASCENSION_LEVELS[2].level).toBe(2);
  });

  it("stacks the modifiers below a level, highest first, and clamps", () => {
    expect(blitzModifiers(0)).toEqual([]);
    expect(blitzModifiers(3).map((m) => m.title)).toEqual(["Tough Enemies", "Tight Belt", "Weary Traveler"]);
    expect(blitzModifiers(9)).toHaveLength(5);
    expect(blitzLevel(9).level).toBe(5);
    expect(blitzLevel(-1).level).toBe(0);
  });

  it("titles the flame with the Blitz modifiers, never the standard ones", () => {
    expect(blitzAscensionTitle(0)).toBe("Blitz ascension 0 · no modifiers");
    expect(blitzAscensionTitle(2)).toBe("Blitz ascension 2 · Tight Belt, Weary Traveler");
    expect(blitzAscensionTitle(5)).not.toMatch(/Swarming|Poverty|Inflation|Scarcity|Double Boss/);
  });

  it("reads the mode off a state", () => {
    expect(isBlitz({ game_mode: "blitz" })).toBe(true);
    expect(isBlitz({ game_mode: "dread" })).toBe(false);
    expect(isBlitz({})).toBe(false);
    expect(isBlitz(null)).toBe(false);
  });

  it("says how many wares are left to take, as the merchant does", () => {
    expect(blitzPicksLine(3)).toBe("You may choose [blue]3[/blue] more items.");
    expect(blitzPicksLine(1)).toBe("You may choose [blue]1[/blue] more item.");
    expect(blitzPicksLine(0)).toMatch(/can't choose any more/);
  });
});

// GH #1347: a fight is the same game in every mode, so its AI (the hint, the
// search tree, the in-fight choice ranking, the turn grade, fight autoplay)
// answers in a Blitz run. The run policies know neither the Blitz merchant
// nor its map: outside a fight a Blitz run has no AI.
describe("the AI's reach in a Blitz run", () => {
  const step = (decision: string, state: Record<string, unknown> = {}) =>
    ({ decision, state: { game_mode: "blitz", ...state } });
  const ENEMY = [{ index: 0, hp: 12 }];
  const all = {};

  it("knows a fight: a card play, or a choice opened inside one", () => {
    expect(isBlitzFight(step("combat_play", { enemies: ENEMY }))).toBe(true);
    expect(isBlitzFight(step("card_select", { enemies: ENEMY, context: "discard" }))).toBe(true);
    // The merchant's deck pick and Neow's card grid are run-level selects.
    expect(isBlitzFight(step("card_select", { context: "upgrade" }))).toBe(false);
    expect(isBlitzFight(step("card_select", { context: "grant", enemies: [] }))).toBe(false);
    for (const d of ["map_select", "shop", "rewards", "event_choice", "rest_site", "card_reward", "game_over"]) {
      expect(isBlitzFight(step(d))).toBe(false);
    }
    expect(isBlitzFight(null)).toBe(false);
  });

  it("leaves a standard or Dread run alone", () => {
    for (const mode of [undefined, "dread"]) {
      const s = { decision: "map_select", state: { game_mode: mode } };
      expect(aiReach(all, s, false)).toEqual({ advisors: true, runMode: true, quiet: false });
      expect(aiReach({ advisors: false, run_mode: false }, s, false))
        .toEqual({ advisors: false, runMode: false, quiet: false });
    }
    expect(aiReach(all, null, false)).toEqual({ advisors: true, runMode: true, quiet: false });
  });

  it("offers the advisors in a Blitz fight, never the run's agent", () => {
    expect(aiReach(all, step("combat_play", { enemies: ENEMY }), false))
      .toEqual({ advisors: true, runMode: false, quiet: false });
    expect(aiReach(all, step("card_select", { enemies: ENEMY }), false))
      .toEqual({ advisors: true, runMode: false, quiet: false });
    // A build without advisors has none in a fight either.
    expect(aiReach({ advisors: false }, step("combat_play", { enemies: ENEMY }), false))
      .toEqual({ advisors: false, runMode: false, quiet: false });
  });

  it("has no AI on a Blitz run's other screens, and keeps advice quiet there", () => {
    for (const d of ["map_select", "shop", "rewards", "event_choice", "rest_site", "card_reward", "game_over"]) {
      expect(aiReach(all, step(d), false)).toEqual({ advisors: false, runMode: false, quiet: true });
    }
    expect(aiReach(all, step("card_select", { context: "remove" }), false))
      .toEqual({ advisors: false, runMode: false, quiet: true });
  });

  it("has none on a recorded Blitz run: the live routes are the only ones opened", () => {
    expect(aiReach(all, step("combat_play", { enemies: ENEMY }), true))
      .toEqual({ advisors: false, runMode: false, quiet: true });
  });
});

describe("game modes", () => {
  it("names the mode in a start body only off standard", () => {
    expect(gameModeBody("standard")).toEqual({});
    expect(gameModeBody("dread")).toEqual({ game_mode: "dread" });
    expect(gameModeBody("blitz")).toEqual({ game_mode: "blitz" });
  });

  it("applies a remembered pick only where the mode can run", () => {
    expect(effectiveGameMode("blitz", "rust", "v0.113.0")).toBe("blitz");
    expect(effectiveGameMode("blitz", "rust", "v0.111.0")).toBe("standard");
    expect(effectiveGameMode("blitz", "csharp", "v0.113.0")).toBe("standard");
    expect(effectiveGameMode("dread", "rust", "v0.113.0")).toBe("dread");
    expect(effectiveGameMode("dread", "rust", "v0.107.1")).toBe("standard");
    expect(effectiveGameMode("standard", "csharp", "v0.107.1")).toBe("standard");
    expect(gameModeUnavailableReason("standard", "csharp", "v0.107.1")).toBeNull();
    expect(gameModeUnavailableReason("blitz", "csharp", "v0.113.0")).toMatch(/Rust engine/);
  });
});
