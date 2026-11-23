import { describe, it, expect } from "vitest";
import {
  DREAD_GAME_VERSION, dominionBoss, dominionTitle, dreadUnavailableReason, gameModeBody, isDread,
} from "../dread";

describe("dread", () => {
  it("is only offered on the Rust engine at v0.113.0", () => {
    expect(DREAD_GAME_VERSION).toBe("v0.113.0");
    expect(dreadUnavailableReason("rust", "v0.113.0")).toBeNull();
    expect(dreadUnavailableReason("rust", "v0.111.0")).toMatch(/v0\.113\.0/);
    expect(dreadUnavailableReason("rust", "v0.107.1")).toMatch(/v0\.113\.0/);
    expect(dreadUnavailableReason("csharp", "v0.113.0")).toMatch(/Rust engine/);
  });

  it("names the mode in a start body only for Dread", () => {
    expect(gameModeBody("standard")).toEqual({});
    expect(gameModeBody("dread")).toEqual({ game_mode: "dread" });
  });

  it("reads the mode off a state", () => {
    expect(isDread({ game_mode: "dread" })).toBe(true);
    expect(isDread({ game_mode: "standard" })).toBe(false);
    expect(isDread({})).toBe(false);
    expect(isDread(null)).toBe(false);
  });

  // The game titles a dominion `{BossName}'s Dominion` (`ascension.json`
  // `DREAD.title`) with the boss ENCOUNTER's title; two of them carry an
  // article the dominion's own entry drops.
  it("names a dominion after its boss, as the game does", () => {
    expect(dominionBoss("KAISER_CRAB_DOMINION")).toBe("Kaiser Crab");
    expect(dominionBoss("LAGAVULIN_MATRIARCH_DOMINION")).toBe("Lagavulin Matriarch");
    expect(dominionBoss("KIN_DOMINION")).toBe("The Kin");
    expect(dominionBoss("INSATIABLE_DOMINION")).toBe("The Insatiable");
    expect(dominionBoss("SOME_NEW_BOSS_DOMINION")).toBe("Some New Boss");
    expect(dominionBoss("ODD")).toBe("Odd");
    expect(dominionTitle("QUEEN_DOMINION")).toBe("Queen's Dominion");
    expect(dominionTitle("KIN_DOMINION")).toBe("The Kin's Dominion");
  });
});
