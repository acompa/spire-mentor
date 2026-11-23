import { render } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import Combatant from "../Combatant";
import CardFrame from "../CardFrame";
import EventScreen from "../EventScreen";
import { enemyLabeller } from "../../enemyLabels";
import { powerItem } from "../../inspect/powerItem";
import type { Card, Step } from "../../api";

vi.mock("../SpineSprite", () => ({
  default: () => <div data-testid="spine-mock" />,
}));

/**
 * Chinese display mode: the backend adds `display_name` / `description` /
 * `type_name` / `event_name_en` beside the English fields, which stay the keys
 * art, skins and logic read. The page SHOWS the display fields and keeps
 * keying on the English ones.
 */
describe("backend display fields", () => {
  it("the enemy nameplate, intent and powers show the display text", () => {
    const enemies = [
      { index: 0, name: "Two-Tailed Rat", display_name: "双尾鼠" },
      { index: 1, name: "Two-Tailed Rat", display_name: "双尾鼠" },
    ];
    const labelOf = enemyLabeller(enemies);
    expect(labelOf(1)).toBe("双尾鼠 #2");
    const c = {
      name: "Two-Tailed Rat", display_name: "双尾鼠", hp: 10, max_hp: 10, block: 0,
      intents: [{ type: "Attack", damage: 5, display_name: "攻击", description: "该敌人打算造成5点伤害。" }],
      powers: [{ name: "Strength", id: "STRENGTH", display_name: "力量", description: "增加攻击伤害。", amount: 2 }],
    };
    const { container } = render(<Combatant c={c} isEnemy label={labelOf(1)} />);
    expect(container.querySelector(".gs-nameplate")!.textContent).toBe("双尾鼠 #2");
    const img = container.querySelector(".gs-intent-img") as HTMLImageElement;
    expect(img.title).toBe("攻击");
    // The art is still picked by the English intent type.
    expect(img.getAttribute("src")).toContain("intent_attack");
    const power = container.querySelector(".gs-power") as HTMLElement;
    expect(power.title).toBe("力量 — 增加攻击伤害。");
    expect(power.textContent).toContain("力量");
    const item = powerItem(c.powers[0], "e1");
    expect(item.name).toBe("力量");
    // The sheet entry still keys on the English name.
    expect(item.key).toBe("power:e1:Strength");
  });

  it("English payloads (no display fields) render as before", () => {
    const c = { name: "Louse", hp: 10, max_hp: 10, block: 0, intents: [{ type: "Attack", damage: 5 }] };
    const { container } = render(<Combatant c={c} isEnemy />);
    expect(container.querySelector(".gs-nameplate")!.textContent).toBe("Louse");
    expect((container.querySelector(".gs-intent-img") as HTMLImageElement).title).toBe("Aggressive");
  });

  it("the card plaque prints type_name while the frame keys on type", () => {
    const card: Card = {
      index: 0, id: "STRIKE", name: "打击", cost: 1, type: "Attack", type_name: "攻击",
      rarity: "Basic", rarity_name: "基础", color: "red",
      description: "造成6点伤害。", can_play: true, image: null,
    };
    const { container, getByText } = render(<CardFrame card={card} />);
    expect(getByText("攻击")).toBeInTheDocument();
    const srcs = Array.from(container.querySelectorAll("img")).map((i) => i.getAttribute("src") ?? "");
    expect(srcs.some((s) => s.includes("frames/attack_"))).toBe(true);
  });

  it("the event room keys on event_name_en and shows the epithet", () => {
    const step = {
      idx: 0, decision: "event_choice", scores: [], pv: null, chosen_action: null, source: "test",
      meta: { turn: null, step_no: 0 },
      state: {
        decision: "event_choice", event_name: "涅奥", event_name_en: "Neow", event_epithet: "复生之母（流放）",
        description: "", event_options: [],
      },
    } as unknown as Step;
    const { container, getByText } = render(<EventScreen step={step} />);
    expect(container.querySelector(".gs-event--neow")).not.toBeNull();
    expect(getByText("涅奥")).toBeInTheDocument();
    expect(getByText("复生之母（流放）")).toBeInTheDocument();
  });
});
