import { render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import Hand from "../Hand";
import { TOUCH_QUERY } from "../../useMediaQuery";
import type { Card } from "../../api";

const hand = (n: number): Card[] =>
  Array.from({ length: n }, (_, i) => ({
    index: i, id: `CARD.STRIKE_${i}`, name: `Strike ${i}`, cost: 1,
    type: "Attack", description: "Deal 6 damage.", image: null, can_play: true,
  }));

const win = window as unknown as { matchMedia?: unknown };
const hadMatchMedia = "matchMedia" in win;
const prevMatchMedia = win.matchMedia;

/** `window.matchMedia` answering true for exactly `query` (jsdom has none). */
function mediaMatching(query: string): void {
  win.matchMedia = (q: string) => ({
    matches: q === query, media: q, addEventListener() {}, removeEventListener() {},
  });
}

afterEach(() => {
  if (hadMatchMedia) win.matchMedia = prevMatchMedia;
  else delete win.matchMedia;
});

const tags = (container: HTMLElement) =>
  [...container.querySelectorAll(".gs-hotkey-tag")].map((e) => e.textContent);

describe("the hand's number-key tags", () => {
  it("count the cards where there is a keyboard", () => {
    const { container } = render(<Hand cards={hand(3)} onCardClick={() => {}} />);
    expect(tags(container)).toEqual(["1", "2", "3"]);
  });

  it("are not drawn on a touchscreen, where they read as the AI's play order", () => {
    // Bug report wb-2f2f80cf2b956066: in Coach mode on a phone the 1-2-3 in
    // each card's corner looked like the recommended order.
    mediaMatching(TOUCH_QUERY);
    const { container } = render(<Hand cards={hand(3)} onCardClick={() => {}} />);
    expect(tags(container)).toEqual([]);
  });
});
