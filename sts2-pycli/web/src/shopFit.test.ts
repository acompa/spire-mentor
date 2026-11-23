import { describe, expect, it } from "vitest";
import { SHOP_MIN_ZOOM, searchZoom, splitShelves } from "./shopFit";
import type { Card } from "./api";

const card = (index: number, color?: string): Card =>
  ({ index, id: `C${index}`, name: `C${index}`, cost: 50, type: "Attack", description: "", color }) as Card;

describe("splitShelves: the game's two shelves", () => {
  it("puts the colorless cards on the floor beside the wares", () => {
    const cards = [0, 1, 2, 3, 4].map((i) => card(i, "red")).concat([card(5, "colorless"), card(6, "colorless")]);
    const { rack, bin } = splitShelves(cards);
    expect(rack.map((c) => c.index)).toEqual([0, 1, 2, 3, 4]);
    expect(bin.map((c) => c.index)).toEqual([5, 6]);
  });

  it("keeps a bought-out class shelf short instead of pulling a colorless card up", () => {
    const { rack, bin } = splitShelves([card(1, "red"), card(5, "colorless")]);
    expect(rack.map((c) => c.index)).toEqual([1]);
    expect(bin.map((c) => c.index)).toEqual([5]);
  });

  it("falls back to the merchant's slot order when no card carries a colour", () => {
    const { rack, bin } = splitShelves([0, 1, 2, 3, 4, 5, 6].map((i) => card(i)));
    expect(rack).toHaveLength(5);
    expect(bin.map((c) => c.index)).toEqual([5, 6]);
  });

  it("leaves an all-colorless shop on the rack", () => {
    const { rack, bin } = splitShelves([card(0, "colorless"), card(1, "colorless")]);
    expect(rack).toHaveLength(2);
    expect(bin).toHaveLength(0);
  });
});

describe("searchZoom", () => {
  // A composition 640 design px tall in a 466px stage: fits at z <= 0.728.
  const fitsIn = (h: number, natural = 640) => (z: number) => natural * z <= h;

  it("finds the largest step that fits, from a guess on either side", () => {
    expect(searchZoom(fitsIn(466), 0.5)).toBeCloseTo(0.72);
    expect(searchZoom(fitsIn(466), 0.9)).toBeCloseTo(0.72);
  });

  it("never zooms in past the design size", () => {
    expect(searchZoom(fitsIn(2000), 0.9)).toBe(1);
  });

  it("stops at the floor when nothing fits (the stage scrolls)", () => {
    expect(searchZoom(fitsIn(100), 0.5)).toBe(SHOP_MIN_ZOOM);
  });
});
