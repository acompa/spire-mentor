// LerpWeight.cs -- the weight rule behind SimStableLerp.
//
// The game eases some nodes toward a target once per frame with
// Lerp(current, target, delta * k): the hand's cards (k = 10 for the angle,
// 8 for the scale, 7 for the position), the targeting arrow (14) and the
// Sovereign Blade (7). Godot's delta is already multiplied by
// Engine.TimeScale, so at 8x one slow frame can push the weight past 1, and
// the step then lands on the far side of the target. Past 2 each frame lands
// further away than the last: the error grows without bound, and a hand card
// spun forever (reported 2026-11-15, at 8x on a busy machine; for the angle
// at 8x that is any frame rate under 40 fps). A weight of 1 lands exactly on
// the target, so capping it there changes nothing below the old cap and turns
// the runaway into a snap.
//
// Game-free on purpose (no Godot, no sts2.dll) so tests/unit can link it.

namespace SpireMentor;

internal static class LerpWeight
{
    /// <summary>The weight a per-frame ease may use: never past the target.</summary>
    public static float Stable(float weight) => weight > 1f ? 1f : weight;
}
