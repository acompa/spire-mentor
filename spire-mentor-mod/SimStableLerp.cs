// SimStableLerp.cs -- the game's per-frame eases may not overshoot.
//
// Why (LerpWeight.cs has the arithmetic): at 8x on a busy machine a hand card
// spun forever, because NHandCardHolder.AnimAngle eases with
// Lerp(angle, target, delta * 10) and Godot's delta carries the TimeScale.
// Above a weight of 2 every frame overshoots further than the last.
//
// A transpiler rewrites each Mathf.Lerp(float, float, float) and
// Vector2.Lerp(Vector2, float) call in the methods below to go through
// LerpWeight.Stable, which caps the weight at 1 (lands on the target). It is
// the weight, not delta, that gets capped: the Sovereign Blade's _Process also
// advances its orbit by delta, and that must keep its speed.
//
// The methods are every per-frame `delta * k` ease the game runs in combat
// (v0.113.1 decomp: grep `Lerp(` with `delta`). The hand's three are async
// loops, so the transpiler goes on their state machines' MoveNext. The
// others (map, rewards and scroll containers, the timeline) only ease while
// the player drags or scrolls them.
//
// Each target is an `AccessTools.Method(typeof(T), "name")`, the shape
// tests/compile/RefCheck verifies against every supported build; at runtime a
// missing one logs a skip rather than failing the mod's load.

using System;
using System.Collections.Generic;
using System.Reflection;
using System.Reflection.Emit;
using Godot;
using HarmonyLib;
using MegaCrit.Sts2.Core.Nodes.Cards.Holders;
using MegaCrit.Sts2.Core.Nodes.Combat;
using MegaCrit.Sts2.Core.Nodes.Vfx;

namespace SpireMentor;

public static class SimStableLerp
{
    /// <summary>The eases to stabilise; the hand's are async loops.</summary>
    private static readonly (string Label, Func<MethodBase?> Find)[] Targets =
    {
        ("NHandCardHolder.AnimAngle",
            () => AccessTools.AsyncMoveNext(AccessTools.Method(typeof(NHandCardHolder), "AnimAngle"))),
        ("NHandCardHolder.AnimScale",
            () => AccessTools.AsyncMoveNext(AccessTools.Method(typeof(NHandCardHolder), "AnimScale"))),
        ("NHandCardHolder.AnimPosition",
            () => AccessTools.AsyncMoveNext(AccessTools.Method(typeof(NHandCardHolder), "AnimPosition"))),
        ("NTargetingArrow._Process", () => AccessTools.Method(typeof(NTargetingArrow), "_Process")),
        ("NSovereignBladeVfx._Process", () => AccessTools.Method(typeof(NSovereignBladeVfx), "_Process")),
    };

    private static readonly MethodInfo MathfLerp = AccessTools.Method(
        typeof(Mathf), nameof(Mathf.Lerp), new[] { typeof(float), typeof(float), typeof(float) });

    private static readonly MethodInfo Vector2Lerp = AccessTools.Method(
        typeof(Vector2), nameof(Vector2.Lerp), new[] { typeof(Vector2), typeof(float) });

    /// <summary>Lerps the transpiler rewrote in the method being patched.</summary>
    private static int _rewritten;

    public static void Install(Harmony patcher)
    {
        var transpiler = new HarmonyMethod(typeof(SimStableLerp), nameof(Transpiler));
        foreach ((string label, Func<MethodBase?> find) in Targets)
        {
            try
            {
                MethodBase? target = find();
                if (target == null)
                {
                    Logger.Log($"  SKIP: stable lerp {label} - not found");
                    continue;
                }
                _rewritten = 0;
                patcher.Patch(target, transpiler: transpiler);
                Logger.Log(_rewritten > 0
                    ? $"  Patched: stable lerp {label} ({_rewritten} lerp(s))"
                    : $"  SKIP: stable lerp {label} - no Lerp call found, the ease is unguarded");
            }
            catch (Exception ex)
            {
                Logger.Log($"  SKIP: stable lerp {label} - {ex.Message}");
            }
        }
    }

    private static IEnumerable<CodeInstruction> Transpiler(IEnumerable<CodeInstruction> instructions)
    {
        foreach (CodeInstruction ins in instructions)
        {
            if (ins.Calls(MathfLerp))
            {
                ins.opcode = OpCodes.Call;
                ins.operand = AccessTools.Method(typeof(SimStableLerp), nameof(StableLerp));
                _rewritten++;
            }
            else if (ins.Calls(Vector2Lerp))
            {
                // The instance call takes the Vector2 by address; a `ref`
                // parameter takes the same managed pointer off the stack.
                ins.opcode = OpCodes.Call;
                ins.operand = AccessTools.Method(typeof(SimStableLerp), nameof(StableLerpVector2));
                _rewritten++;
            }
            yield return ins;
        }
    }

    public static float StableLerp(float from, float to, float weight) =>
        Mathf.Lerp(from, to, LerpWeight.Stable(weight));

    public static Vector2 StableLerpVector2(ref Vector2 from, Vector2 to, float weight) =>
        from.Lerp(to, LerpWeight.Stable(weight));
}
