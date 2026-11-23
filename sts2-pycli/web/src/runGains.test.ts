import { describe, expect, it } from "vitest";
import type { Card, Step } from "./api";
import { runGains as runGainsWithCards } from "./runGains";

/** The gains without the card objects the banner draws. */
const runGains = (a: Step | null, b: Step | null) =>
  runGainsWithCards(a, b).map(({ card: _card, ...g }) => g);

function card(id: string, name: string, upgraded = false): Card {
  return { index: 0, id, name, cost: 1, type: "Attack", description: "", upgraded } as Card;
}

function step(decision: string, player: Record<string, unknown>, extra: Record<string, unknown> = {}): Step {
  return {
    decision,
    state: { player: { name: "Ironclad", hp: 70, max_hp: 80, block: 0, gold: 99, relics: [], potions: [], deck: [], ...player }, ...extra },
  } as unknown as Step;
}

describe("runGains", () => {
  it("names a relic, a card and the gold an event handed over", () => {
    const a = step("event_choice", { deck: [card("STRIKE", "Strike")] });
    const b = step("map_select", {
      gold: 149,
      relics: [{ name: "White Star", image: "w.png" }],
      deck: [card("STRIKE", "Strike"), card("INFLAME", "Inflame")],
    });
    expect(runGains(a, b)).toEqual([
      { kind: "relic", sign: "+", text: "White Star", image: "w.png" },
      { kind: "card", sign: "+", text: "Inflame" },
      { kind: "gold", sign: "+", text: "+50 gold" },
    ]);
  });

  it("reads a smith as an upgrade, not a swap", () => {
    const a = step("rest_site", { deck: [card("BASH", "Bash"), card("BASH", "Bash")] });
    const b = step("map_select", { deck: [card("BASH", "Bash"), card("BASH", "Bash", true)] });
    expect(runGains(a, b)).toEqual([{ kind: "card", sign: "^", text: "Bash+" }]);
  });

  it("reports losses: a removed card, max HP and a curse", () => {
    const a = step("event_choice", { max_hp: 80, deck: [card("STRIKE", "Strike")] });
    const b = step("event_choice", { max_hp: 72, deck: [card("REGRET", "Regret")] });
    expect(runGains(a, b).map((g) => `${g.sign}${g.text}`)).toEqual([
      "+Regret", "-Strike", "-−8 Max HP",
    ]);
  });

  it("stays quiet in and out of a fight, and when the chest panel shows it", () => {
    const fight = step("combat_play", { gold: 10 });
    const map = step("map_select", { gold: 50 });
    expect(runGains(fight, step("card_reward", { gold: 30 }))).toEqual([]);
    expect(runGains(map, fight)).toEqual([]);
    const pick = step("card_select", {}, { enemies: [{ name: "Nibbit" }] });
    expect(runGains(pick, map)).toEqual([]);
    const chest = step("map_select", { gold: 99 }, { room_reveal: { kind: "treasure" } });
    expect(runGains(map, chest)).toEqual([]);
    expect(runGains(null, map)).toEqual([]);
  });

  it("counts a run-level card pick (no enemies) as outside a fight", () => {
    const a = step("card_select", {}, { enemies: [] });
    const b = step("map_select", { deck: [card("APPARITION", "Apparition")] });
    expect(runGains(a, b)).toEqual([{ kind: "card", sign: "+", text: "Apparition" }]);
  });

  it("names the cards a random upgrade and an enchant picked (GH #1596)", () => {
    const a = step("shop", { deck: [card("DEFEND", "Defend"), card("IRON_WAVE", "Iron Wave"), card("BODY_SLAM", "Body Slam")] });
    const slam = { ...card("BODY_SLAM", "Body Slam"), enchantment: "VIGOROUS", enchantment_amount: 5 };
    const b = step("shop", { deck: [card("DEFEND", "Defend", true), card("IRON_WAVE", "Iron Wave", true), slam] });
    const gains = runGainsWithCards(a, b);
    expect(gains.map((g) => `${g.sign}${g.text}`)).toEqual([
      "^Defend+", "^Iron Wave+", "*Body Slam · Vigorous 5",
    ]);
    expect(gains[2].card?.enchantment).toBe("VIGOROUS");
  });
});
