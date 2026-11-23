import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import AutobattleFloat from "./AutobattleFloat";
import type { AutobattleDiff, AutobattleVerdict } from "../api";

const DIFF: AutobattleDiff = {
  from_floor: 1, to_floor: 2, autoplayed: 0, hp: 76, max_hp: 80, hp_delta: 0, gold: 116,
  gold_delta: 0, deck_size: 14, deck: { added: ["Rupture"], removed: [], upgraded: [] },
  relics_gained: [], relics_lost: [], potions_gained: [], potions_lost: [], lines: [],
};
const DIFFERED: AutobattleVerdict = {
  available: true, agreed: false, band: "differed", brain: "e2e",
  champion_label: "Take Twin Strike", your_label: "Take Rupture", champion_prob: 1, your_prob: 0,
} as AutobattleVerdict;

function props(over: Partial<React.ComponentProps<typeof AutobattleFloat>> = {}) {
  return {
    diff: DIFF, verdict: DIFFERED, busy: false, over: false, won: false, watch: true,
    onWatchChange: () => {}, onAdvance: () => {}, onStop: () => {}, ...over,
  };
}

function setup(over = {}) {
  const outside = document.createElement("button");
  document.body.appendChild(outside);
  const box = document.createElement("div");
  document.body.appendChild(box);
  Object.defineProperty(box, "clientWidth", { value: 1000, configurable: true });
  Object.defineProperty(box, "clientHeight", { value: 600, configurable: true });
  const r = render(<AutobattleFloat {...props(over)} />, { container: box });
  return { outside, box, ...r };
}

describe("AutobattleFloat", () => {
  beforeEach(() => {
    localStorage.clear();
    document.body.innerHTML = "";
  });

  it("folds itself when the board is tapped, keeping the button and a one-line verdict", () => {
    const { outside } = setup();
    expect(screen.getByText("Rupture")).toBeTruthy();
    fireEvent.pointerDown(outside);
    expect(screen.queryByText("Rupture")).toBeNull();
    expect(screen.getByTestId("autobattle-summary").textContent).toBe("✗ champion: Take Twin Strike");
    expect(screen.getByRole("button", { name: "Play to next decision" })).toBeTruthy();
  });

  it("does not fold on a tap inside it", () => {
    setup();
    fireEvent.pointerDown(screen.getByText("Rupture"));
    expect(screen.getByText("Rupture")).toBeTruthy();
  });

  it("unfolds when the next result arrives", () => {
    const { outside, rerender } = setup();
    fireEvent.pointerDown(outside);
    expect(screen.queryByText("Rupture")).toBeNull();
    rerender(<AutobattleFloat {...props({ diff: { ...DIFF, to_floor: 3 } })} />);
    expect(screen.getByText("Rupture")).toBeTruthy();
  });

  it("stays folded across results once folded with its caret, until the caret unfolds it", () => {
    const { rerender } = setup();
    fireEvent.click(screen.getByRole("button", { name: "collapse autobattle panel" }));
    rerender(<AutobattleFloat {...props({ diff: { ...DIFF, to_floor: 3 } })} />);
    expect(screen.queryByText("Rupture")).toBeNull();
    expect(localStorage.getItem("autobattleFloatPinned")).toBe("1");
    fireEvent.click(screen.getByRole("button", { name: "expand autobattle panel" }));
    expect(screen.getByText("Rupture")).toBeTruthy();
    expect(localStorage.getItem("autobattleFloatPinned")).toBe("0");
  });

  it("drags by its grip, remembers the spot, and double-tap resets it", () => {
    setup();
    const card = screen.getByTestId("autobattle-float");
    const grip = screen.getByRole("button", { name: /move the autobattle panel/ });
    act(() => {
      fireEvent.pointerDown(grip, { button: 0, clientX: 10, clientY: 10, pointerId: 1 });
      fireEvent.pointerMove(grip, { clientX: 410, clientY: 210, pointerId: 1 });
      fireEvent.pointerUp(grip, { pointerId: 1 });
    });
    expect(card.style.left).toBe("400px");
    expect(card.style.top).toBe("200px");
    expect(JSON.parse(localStorage.getItem("autobattleFloatPos") ?? "null")).toEqual({ x: 400, y: 200 });
    fireEvent.doubleClick(grip);
    expect(card.style.left).toBe("8px"); // the default spot
    expect(card.style.top).toBe("64px"); // no HUD on this board: the fallback top
    expect(localStorage.getItem("autobattleFloatPos")).toBeNull();
  });

  it("rises to fit when it unfolds near the bottom edge, and drops back to its spot folded", () => {
    localStorage.setItem("autobattleFloatPos", JSON.stringify({ x: 100, y: 550 }));
    const { outside, rerender } = setup();
    const card = screen.getByTestId("autobattle-float");
    // jsdom lays nothing out: a folded card is 40px tall, an unfolded one 300px.
    Object.defineProperty(card, "offsetHeight", {
      configurable: true,
      get: () => (card.classList.contains("wb-autobattle-float--compact") ? 40 : 300),
    });
    rerender(<AutobattleFloat {...props({ diff: { ...DIFF, to_floor: 3 } })} />);
    expect(card.style.top).toBe("300px"); // 600 − 300: the whole card on the board
    fireEvent.pointerDown(outside);
    expect(card.style.top).toBe("550px"); // folded, back at the anchor
    expect(localStorage.getItem("autobattleFloatPos")).toBe(JSON.stringify({ x: 100, y: 550 }));
  });

  it("keeps the whole card on the board when dragged past an edge", () => {
    setup();
    const card = screen.getByTestId("autobattle-float");
    const grip = screen.getByRole("button", { name: /move the autobattle panel/ });
    act(() => {
      fireEvent.pointerDown(grip, { button: 0, clientX: 0, clientY: 0, pointerId: 1 });
      fireEvent.pointerMove(grip, { clientX: 5000, clientY: 5000, pointerId: 1 });
      fireEvent.pointerUp(grip, { pointerId: 1 });
    });
    expect(card.style.left).toBe("1000px"); // clientWidth − offsetWidth (0 under jsdom)
    expect(card.style.top).toBe("600px"); // clientHeight − offsetHeight (0 under jsdom)
  });
});
