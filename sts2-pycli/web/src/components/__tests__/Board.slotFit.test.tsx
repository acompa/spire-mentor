import { render } from "@testing-library/react";
import { afterAll, describe, expect, it } from "vitest";
import Board from "../Board";
import { stageZoom } from "../../stageZoom";
import fixture from "./fixture-step.json";
import type { Step } from "../../api";

/**
 * A fighter on an authored encounter marker stays on a narrow stage.
 *
 * `NCombatRoom.AdjustCreatureScaleForAspectRatio` shrinks the room when any
 * creature would stand past the right edge, measured where the creature
 * actually stands. The Ovicopter's marker is 1011 scene px right of centre,
 * past the 960 a 16:9 frame shows. The workbench measured the row algorithm's
 * slots instead, which a slotted fight does not use. On a tablet the
 * Ovicopter, its intent and its HP bar were off the glass for the whole
 * fight (bug report wb-6ee07f3a2a54ba6c).
 */

const LAYOUT_PROPS = ["clientWidth", "clientHeight", "offsetWidth", "offsetHeight"] as const;

function stubStage(w: number, h: number): number {
  const zoom = stageZoom(w, h);
  const dims: Record<string, number> = {
    clientWidth: w, clientHeight: h,
    offsetWidth: Math.round(w / zoom), offsetHeight: Math.round(h / zoom),
  };
  for (const k of LAYOUT_PROPS) {
    Object.defineProperty(HTMLElement.prototype, k, { configurable: true, get: () => dims[k] });
  }
  return dims.offsetWidth;
}

afterAll(() => {
  for (const k of LAYOUT_PROPS) {
    Object.defineProperty(HTMLElement.prototype, k, { configurable: true, get: () => 0 });
  }
});

// The reported board, as the wire sent it.
const ovicopter = {
  index: 0, uid: 2, name: "Ovicopter", kind: "Ovicopter", hp: 127, max_hp: 130, block: 0,
  image: "/assets/monsters/egg_layer.png", rig: "/assets/monster_rigs/ovicopter/ovicopter",
  rig_scale: 0.3, rig_bounds_h: 476, rig_bounds_w: 403,
  intents: [{ type: "Summon" }],
};

function step(): Step {
  const base = fixture as unknown as Step;
  return {
    ...base,
    meta: { ...base.meta, act: 2, floor: 11 },
    state: { ...base.state, enemies: [ovicopter] } as Step["state"],
  };
}

describe("an enemy on an authored marker", () => {
  it("stands inside a tablet's landscape stage", () => {
    const layerW = stubStage(1280, 640);
    const { container } = render(<Board step={step()} />);
    const el = container.querySelector<HTMLElement>(".gs-side [data-enemy-index='0']")
      ?.closest<HTMLElement>(".gs-side");
    expect(el).toBeTruthy();
    // The column's left is the creature's centre line.
    const centre = parseFloat(el!.style.left);
    expect(centre).toBeGreaterThan(layerW / 2);
    expect(centre).toBeLessThan(layerW - 40);
  });
});
