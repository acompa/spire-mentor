import { describe, expect, it } from "vitest";
import { cardEnchantment, fillTemplate, lookupTerm, upgradedTwin, type Glossary } from "./glossary";

const G: Glossary = {
  enchantments: {
    ADROIT: { title: "Adroit", description: "Gain {Block:diff()} [gold]Block[/gold].", extra: "Gain {Block} Block.", show_amount: true, image: "enchantments/adroit.png" },
    VIGOROUS: { title: "Vigorous", description: "The first time this card is played, it deals {Amount} additional damage.", extra: null, show_amount: true, image: "enchantments/vigorous.png" },
  },
  keywords: { EXHAUST: { title: "Exhaust", description: "Removed until end of combat." } },
  tips: {},
  relics: { BURNING_BLOOD: { title: "Burning Blood", description: "Heal 6 HP.", image: "relic_atlas/burning_blood.png" } },
  upgrades: {
    BASH: { description: "Deal 10 damage.\nApply 3 [gold]Vulnerable[/gold]." },
    BODY_SLAM: { description: "Deal damage equal to your [gold]Block[/gold].", cost: 0 },
  },
};

describe("glossary", () => {
  it("fills an enchantment template with its amount", () => {
    expect(fillTemplate("Gain {Block:diff()} Block.", 3)).toBe("Gain 3 Block.");
    expect(fillTemplate("{Cards:plural:card|cards}", 1)).toBe("card");
    expect(fillTemplate("Gain {Energy:energyIcons()}.", 2)).toBe("Gain 2 Energy.");
  });

  it("finds the term a coloured run names (GH #1599)", () => {
    expect(lookupTerm(G, "Adroit 3")).toEqual({ title: "Adroit 3", description: "Gain 3 [gold]Block[/gold].", image: "enchantments/adroit.png" });
    expect(lookupTerm(G, "Adroit", 2)?.title).toBe("Adroit 2");
    expect(lookupTerm(G, "Exhausted")?.title).toBe("Exhaust");
    expect(lookupTerm(G, "Burning Blood")?.image).toBe("relic_atlas/burning_blood.png");
    expect(lookupTerm(G, "Nonsense")).toBeNull();
  });

  it("describes a card's enchantment (GH #1600)", () => {
    const e = cardEnchantment(G, { enchantment: "VIGOROUS", enchantment_amount: 5 });
    expect(e).toMatchObject({ title: "Vigorous", amount: 5, showAmount: true, image: "enchantments/vigorous.png" });
    expect(e?.description).toMatch(/deals 5 additional damage/);
    expect(cardEnchantment(G, {})).toBeNull();
  });

  it("draws a card upgraded with its changed numbers green (GH #1598)", () => {
    const bash: { id: string; cost: number; description: string; description_rich: string; upgraded?: boolean } = { id: "BASH", cost: 2, description: "Deal 8 damage.\nApply 2 Vulnerable.", description_rich: "Deal 8 damage.\nApply 2 [gold]Vulnerable[/gold]." };
    const up = upgradedTwin(bash, G);
    expect(up.upgraded).toBe(true);
    expect(up.description).toBe("Deal 10 damage.\nApply 3 Vulnerable.");
    expect(up.description_rich).toBe("Deal [green]10[/green] damage.\nApply [green]3[/green] [gold]Vulnerable[/gold].");
    expect(upgradedTwin({ id: "BODY_SLAM", cost: 1 }, G).cost).toBe(0);
    // Already upgraded, or no upgrade on file: the card as it is.
    const done = { ...bash, upgraded: true };
    expect(upgradedTwin(done, G)).toBe(done);
    const strike = { id: "UNKNOWN" };
    expect(upgradedTwin(strike, G)).toBe(strike);
  });
});
