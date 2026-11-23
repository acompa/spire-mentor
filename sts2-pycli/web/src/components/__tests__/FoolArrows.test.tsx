import { render } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import Combatant from "../Combatant";
import FoolQuiver from "../FoolQuiver";
import { SlotArrows } from "../ImpaledArrows";

/**
 * The Fool's arrows (v0.113.0): the bow and its quiver bottom-left
 * (`fool_combat_ui.tscn`), and the arrows stuck in an impaled enemy
 * (`impaled_arrow_vfx.tscn`). Without them the board had no way to show how
 * many shots are left or which enemy holds the arrow.
 */
describe("the Fool's quiver", () => {
  it("draws the bow and one slot per arrow, in the game's art", () => {
    const { container } = render(<FoolQuiver arrows={["retrieved", "away", "retrieved"]} />);
    const bow = container.querySelector<HTMLImageElement>(".gs-quiver-bow");
    expect(bow?.src).toContain("v0.113.0/ui/fool/bow");
    const slots = Array.from(container.querySelectorAll<HTMLElement>(".gs-quiver-arrow"));
    expect(slots.map((s) => s.dataset.status)).toEqual(["retrieved", "away", "retrieved"]);
    // Odd slots are packed head-down, as NFoolArrowUi flips them.
    expect(slots.map((s) => s.classList.contains("gs-quiver-flip"))).toEqual([false, true, false]);
    expect(container.querySelector(".gs-quiver")?.getAttribute("title")).toBe("arrows: 2 ready of 3");
  });

  it("hides a destroyed arrow (a Quietus arrow after it impales)", () => {
    const { container } = render(<FoolQuiver arrows={["destroyed", "retrieved"]} />);
    const slots = Array.from(container.querySelectorAll<HTMLElement>(".gs-quiver-arrow"));
    expect(slots[0].hidden).toBe(true);
    expect(slots[1].hidden).toBe(false);
    expect(container.querySelector(".gs-quiver")?.getAttribute("title")).toBe("arrows: 1 ready of 1");
  });
});

describe("arrows stuck in an impaled enemy", () => {
  const nibbit = (impaled_arrows?: number, extra: object = {}) => ({
    name: "Nibbit", hp: 40, max_hp: 44, block: 0, impaled_arrows, ...extra,
  });

  it("is absent on an enemy with no arrow in it", () => {
    const { container } = render(<Combatant c={nibbit()} isEnemy />);
    expect(container.querySelector(".gs-impaled")).toBeNull();
  });

  it("splits each arrow around the body: head behind, shaft in front", () => {
    const { container } = render(<Combatant c={nibbit(2)} isEnemy />);
    const back = container.querySelector(".gs-impaled-back");
    const front = container.querySelector(".gs-impaled-front");
    const body = container.querySelector(".gs-sprite-body");
    expect(back?.querySelectorAll("img")).toHaveLength(2);
    expect(front?.querySelectorAll("img")).toHaveLength(2);
    expect(back?.querySelector("img")?.src).toContain("ui/fool/arrow_bg");
    expect(front?.querySelector("img")?.src).toContain("ui/fool/arrow_fg");
    // DOM order is paint order among these positioned siblings.
    expect(back!.compareDocumentPosition(body!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(body!.compareDocumentPosition(front!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("sticks at the scene's arrow anchor, not a guessed centre", () => {
    // Sludge Spinner: %CenterPos (0, -175) on a 246-tall box. The game puts the
    // arrow there (ImpaleArrowIntoCreature), not halfway up the drawn body.
    const spinner = { ...nibbit(1), name: "Sludge Spinner", rig_bounds_h: 246, rig_arrow_x: 0, rig_arrow_y: -175 };
    const { container } = render(<Combatant c={spinner} isEnemy pxPerScene={0.5} />);
    const front = container.querySelector<HTMLElement>(".gs-impaled-front");
    expect(front?.style.transform).toBe("scale(0.5) translate(0px, -175px)");
  });

  it("falls back to half the authored box up when the table has no anchor", () => {
    const { container } = render(<Combatant c={nibbit(1, { rig_bounds_h: 146 })} isEnemy />);
    expect(container.querySelector<HTMLElement>(".gs-impaled-front")?.style.transform)
      .toBe("scale(1) translate(0px, -73px)");
  });

  it("is never drawn on the player", () => {
    const { container } = render(<Combatant c={nibbit(1)} isEnemy={false} />);
    expect(container.querySelector(".gs-impaled")).toBeNull();
  });
});

describe("arrows in a rig's arrow slots", () => {
  it("draws each half at the slot's authored offset, scaled back up by the rig scale", () => {
    // ImpaleArrowIntoSpineSlots: ArrowFg at (44.5, -5), scale 0.75 / Body.Scale.X;
    // the second arrow fans out by (15, 5) and -20 degrees.
    const { container } = render(<SlotArrows count={2} layer="front" bodyScale={0.25} />);
    const nodes = Array.from(container.children) as HTMLElement[];
    expect(nodes.map((n) => [n.style.left, n.style.top])).toEqual([["44.5px", "-5px"], ["59.5px", "0px"]]);
    expect(nodes[0].style.transform).toBe("rotate(0deg) scale(3)");
    const [, rot, scale] = /rotate\((.+)deg\) scale\((.+)\)/.exec(nodes[1].style.transform)!;
    expect(Number(rot)).toBe(-20);
    expect(Number(scale)).toBeCloseTo(2.85);
    expect(nodes[0].querySelector("img")?.src).toContain("ui/fool/arrow_fg");
  });
});
