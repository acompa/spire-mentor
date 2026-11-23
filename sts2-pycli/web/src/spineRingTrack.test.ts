import { describe, expect, it } from "vitest";
import { RINGS_IDLE, ringClipFor } from "./spineRingTrack";

const AEONGLASS = ["idle_loop", "attack_heavy", "attack_double", "hurt", "die", "wither",
  "_track1/rings_normal", "_track1/rings_attack_heavy", "_track1/rings_attack_double", "_track1/rings_die"];

describe("Aeonglass's ring track", () => {
  it("pairs each body clip with the ring clip the game plays on track 1", () => {
    expect(ringClipFor("idle_loop", AEONGLASS, "idle_loop")).toBe(RINGS_IDLE);
    expect(ringClipFor("attack_heavy", AEONGLASS, "idle_loop")).toBe("_track1/rings_attack_heavy");
    expect(ringClipFor("die", AEONGLASS, "idle_loop")).toBe("_track1/rings_die");
    // A clip with no ring twin leaves the rings spinning as they were.
    expect(ringClipFor("hurt", AEONGLASS, "idle_loop")).toBeUndefined();
  });

  it("does nothing on a rig without ring clips", () => {
    expect(ringClipFor("idle_loop", ["idle_loop", "die"], "idle_loop")).toBeUndefined();
  });
});
