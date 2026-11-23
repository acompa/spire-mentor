import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import Hand from "../Hand";
import type { Card } from "../../api";
import { AdviceMarksCtx } from "../../advice/useAdviceMarks";
import { cardKey, type Mark } from "../../advice/marks";

// Hold the hand upright on a phone. jsdom has no stage to measure, so the
// metrics keep their fallbacks (cw 180, room 900): an 8-card fan is tight
// there, a 7-card one is not.
vi.mock("../../phone", async (orig) => ({
  ...(await orig<typeof import("../../phone")>()),
  usePhone: () => "portrait",
  isSmallPhoneScreen: () => false,
}));

const card = (index: number, name: string, type = "Attack"): Card => ({
  index, id: `CARD.${name.toUpperCase().replace(/ /g, "_")}`, name, cost: 1,
  type, description: `${name} text.`, image: null,
});
const slots = (c: HTMLElement) => [...c.querySelectorAll(".gs-handslot")];
const spines = (c: HTMLElement) => [...c.querySelectorAll(".gs-card-spine")].map((s) => s.textContent);
const counts = (c: HTMLElement) => [...c.querySelectorAll(".gs-count-tag")].map((s) => s.textContent);

describe("a crowded hand on a phone", () => {
  it("stacks copies into one slot with a count, keeping hand order", () => {
    const names = ["Strike", "Defend", "Bash", "Strike", "Shrug It Off", "Inflame", "Defend", "Anger", "Twin Strike", "Strike"];
    const { container } = render(<Hand cards={names.map((n, i) => card(i, n, n === "Defend" || n === "Shrug It Off" ? "Skill" : "Attack"))} />);
    expect(slots(container)).toHaveLength(7);
    expect(counts(container)).toEqual(["×3", "×2"]);
    // 7 slots no longer crowd the fan, so no spines.
    expect(spines(container)).toEqual([]);
  });

  it("names every covered card on a spine once the fan is still tight", () => {
    const names = ["Strike", "Defend", "Bash", "Pommel Strike", "Shrug It Off", "Inflame", "Anger", "Twin Strike", "Iron Wave"];
    const { container } = render(<Hand cards={names.map((n, i) => card(i, n))} />);
    expect(slots(container)).toHaveLength(9);
    // Every card but the last, which shows its whole face.
    expect(spines(container)).toEqual(names.slice(0, -1));
  });

  it("cards that differ in anything they show do not stack", () => {
    const a = card(0, "Strike"), b = { ...card(1, "Strike"), upgraded: true };
    const rest = ["Bash", "Anger", "Inflame", "Defend", "Iron Wave", "Twin Strike"].map((n, i) => card(i + 2, n));
    const { container } = render(<Hand cards={[a, b, ...rest]} />);
    expect(slots(container)).toHaveLength(8);
    expect(counts(container)).toEqual([]);
    expect(spines(container)).toContain("Strike+");
  });

  it("a stack reads as selected when any of its copies is", () => {
    const names = ["Strike", "Defend", "Bash", "Strike", "Inflame", "Anger", "Iron Wave", "Twin Strike"];
    const { container } = render(<Hand cards={names.map((n, i) => card(i, n))} selectedIndex={3} onCardTap={() => {}} />);
    const sel = container.querySelectorAll(".gs-handslot.gs-selected");
    expect(sel).toHaveLength(1);
    expect(sel[0].querySelector(".gs-count-tag")?.textContent).toBe("×2");
  });

  it("a planned card keeps its own slot, and its step number goes beside or inside the spine", () => {
    const mark = (order: number): Mark => ({ key: "", tier: "best", order, targetIndex: null, label: "", share: null, q: null });
    const ctx = (cards: Card[]) => ({ marks: new Map([[cardKey(cards[0].index), mark(1)]]), showNumbers: false });
    // 8 cards, two of them Strikes: the planned Strike does not stack with the
    // other. The fan's step (102px at the fallback 180px card) has room for
    // the badge beside the spine.
    const few = ["Strike", "Defend", "Bash", "Strike", "Inflame", "Anger", "Iron Wave", "Twin Strike"].map((n, i) => card(i, n));
    const a = render(<AdviceMarksCtx.Provider value={ctx(few)}><Hand cards={few} /></AdviceMarksCtx.Provider>).container;
    expect(slots(a)).toHaveLength(8);
    expect(a.querySelector(".gs-handslot.gs-badge-beside .gs-step-tag")?.textContent).toBe("1");
    // 14 cards step 55px: no room beside, so the number rides in the spine.
    const many = Array.from({ length: 14 }, (_, i) => card(i, `Card ${i}`));
    const b = render(<AdviceMarksCtx.Provider value={ctx(many)}><Hand cards={many} /></AdviceMarksCtx.Provider>).container;
    expect(b.querySelector(".gs-badge-beside")).toBeNull();
    expect(b.querySelector(".gs-step-tag")).toBeNull();
    expect(b.querySelector(".gs-card-spine .gs-spine-step")?.textContent).toBe("1");
  });

  it("a small hand is left alone", () => {
    const names = ["Strike", "Strike", "Defend", "Defend", "Bash"];
    const { container } = render(<Hand cards={names.map((n, i) => card(i, n))} />);
    expect(slots(container)).toHaveLength(5);
    expect(counts(container)).toEqual([]);
    expect(spines(container)).toEqual([]);
  });
});
