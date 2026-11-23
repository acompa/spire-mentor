import { render } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import MapView from "../MapView";
import { HintsFixture } from "../../hints/HintsContext";
import { PHONE_PORTRAIT_QUERY } from "../../useMediaQuery";
import type { MapGraph, MapRow } from "../../api";

/** `window.matchMedia` answering true for exactly `query` (jsdom has none,
 *  which is the desktop path), restored after each test. */
const win = window as unknown as { matchMedia?: unknown };
const had = "matchMedia" in win;
const prev = win.matchMedia;
function mediaQuery(query: string | null): void {
  win.matchMedia = (q: string) => ({
    matches: q === query, media: q, addEventListener() {}, removeEventListener() {},
  });
}
afterEach(() => {
  if (had) win.matchMedia = prev;
  else delete win.matchMedia;
});

function graph(): MapGraph {
  return {
    idx: 0, act: 1, act_name: "act 1", floor: 1,
    full_map: {
      rows: [[
        { col: 0, row: 0, type: "Monster", children: [{ col: 0, row: 1 }] },
        { col: 1, row: 0, type: "Event", children: [{ col: 0, row: 1 }] },
      ], [{ col: 0, row: 1, type: "Rest", children: [] }]],
      boss: { col: 0, row: 2, type: "Boss", name: "The Guardian" },
      current_coord: null,
    },
  };
}

/** Live play: the map is the picker, so the banner has nothing to confirm. */
const live = () => (
  <HintsFixture on={false}>
    <MapView rows={[]} graphs={[graph()]} currentIdx={0} live
      selectable={new Set(["0,0", "1,0"])} onSelect={() => {}} />
  </HintsFixture>
);

/** Replay with a later map decision: the banner jumps to it. */
const replayRows: MapRow[] = [0, 1].map((idx) => ({
  idx, act: 1, act_name: "act 1", floor: idx + 1,
  nodes: [{ col: 0, row: idx, type: "Monster", chosen: true }],
}));
const replay = () => (
  <HintsFixture on={false}>
    <MapView rows={replayRows} graphs={[graph()]} currentIdx={0} onJump={() => {}} />
  </HintsFixture>
);

describe("map Proceed banner on a phone (#1443)", () => {
  it("is left out in live play, where it is always disabled and covered the bottom rooms", () => {
    mediaQuery(PHONE_PORTRAIT_QUERY);
    const { container } = render(live());
    expect(container.querySelector(".gsm-proceed")).toBeNull();
    // ... and the sheet drops the bottom padding that scrolled past it
    expect(container.querySelector(".gs-map--no-proceed")).not.toBeNull();
  });

  it("stays in replay, where it jumps to the next map decision", () => {
    mediaQuery(PHONE_PORTRAIT_QUERY);
    const { container } = render(replay());
    const btn = container.querySelector<HTMLButtonElement>(".gsm-proceed");
    expect(btn).not.toBeNull();
    expect(btn!.disabled).toBe(false);
    expect(container.querySelector(".gs-map--no-proceed")).toBeNull();
  });

  it("stays, disabled, in live play off a phone", () => {
    mediaQuery(null);
    const { container } = render(live());
    const btn = container.querySelector<HTMLButtonElement>(".gsm-proceed");
    expect(btn).not.toBeNull();
    expect(btn!.disabled).toBe(true);
    expect(container.querySelector(".gs-map--no-proceed")).toBeNull();
  });
});
