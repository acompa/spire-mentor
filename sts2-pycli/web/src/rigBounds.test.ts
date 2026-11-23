import { describe, expect, it } from "vitest";
import { RIG_BOUNDS, rigBox } from "./rigBounds";

describe("rigBox", () => {
  it("takes a build's own re-export over the shared rig of the same name", () => {
    // Bug report wb-1aedd1a1059c2d2b: the v0.113.0 Mawler is ~2x the v0.107.1
    // rig. Sized by the shared box, the phone layout thought it fit and drew
    // it past both edges of a portrait screen.
    const shared = rigBox("/assets/monster_rigs/mawler/mawler");
    const v113 = rigBox("/assets/v0.113.0/monster_rigs/mawler/mawler");
    expect(shared).toEqual(RIG_BOUNDS.mawler);
    expect(v113).toEqual(RIG_BOUNDS["v0.113.0/mawler"]);
    expect(v113!.w).toBeGreaterThan(1.9 * shared!.w);
  });

  it("falls back to the shared rig when the build did not redraw it", () => {
    expect(rigBox("/assets/v0.113.0/monster_rigs/nibbit/nibbit")).toEqual(RIG_BOUNDS.nibbit);
  });

  it("is null for an unknown rig", () => {
    expect(rigBox("/assets/monster_rigs/nope/nope")).toBeNull();
    expect(rigBox(null)).toBeNull();
  });
});
