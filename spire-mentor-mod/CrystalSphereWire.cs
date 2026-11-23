// CrystalSphereWire.cs -- what the `crystal_sphere` message shows of the
// board, and how F7 reads the advisor's answer to it.
//
// The digger (sts2-pycli/python/crystal_sphere_policy.py) needs more than the
// hidden cells: the divinations left and at the start, and the item class
// each cleared cell shows. Only what a player can see goes on the wire: a
// hidden cell's item is never sent, and a cleared cell with no item is not
// listed (it is implied by the cell being neither hidden nor in `revealed`).
//
// F7 asks the advisor only when it advertises `capabilities.crystal_sphere`
// (Capability), and plays CrystalSpherePick whenever the answer is not a
// usable cell (AdvisedIndex): a hand_back, no best, a non-choose action or an
// index outside the cells it was sent. The advisor's answer names the policy
// that decided (AdvisedPolicy) -- its digger, or the #872 rule when the
// digger could not answer -- and the action log says which (ClickSource).
//
// Game-free (System.Text.Json only) so tests/unit can link it; the screen
// side is SimCrystalSphereScreenHandler.BuildState and
// SimScreenAdvice.DescribeCrystalSphere.

using System.Collections.Generic;
using System.Text.Json;

namespace SpireMentor;

public static class CrystalSphereWire
{
    /// <summary>The `advice_for` value and the capability's name.</summary>
    public const string AdviceFor = "crystal_sphere";

    /// <summary>
    /// The wire class of a CrystalSphereItem subclass (the decomp's
    /// CrystalSphereItems namespace), or null for anything else -- the
    /// digger then falls back to its block order rather than guess.
    /// </summary>
    public static string? ItemClass(string? itemTypeName) => itemTypeName switch
    {
        "CrystalSphereRelic" => "relic",
        "CrystalSpherePotion" => "potion",
        "CrystalSphereCardReward" => "card",
        "CrystalSphereCurse" => "curse",
        "CrystalSphereGold" => "gold",
        _ => null,
    };

    /// <summary>
    /// `revealed`: <c>{x, y, item}</c> per cleared cell that shows a known
    /// item, in the given order. A hidden cell is never emitted, whatever it
    /// holds.
    /// </summary>
    public static List<Dictionary<string, object>> Revealed(
        IEnumerable<(int X, int Y, bool Hidden, string? ItemTypeName)> cells)
    {
        var revealed = new List<Dictionary<string, object>>();
        foreach ((int x, int y, bool hidden, string? typeName) in cells)
        {
            if (hidden || ItemClass(typeName) is not string item)
            {
                continue;
            }
            revealed.Add(new Dictionary<string, object> { ["x"] = x, ["y"] = y, ["item"] = item });
        }
        return revealed;
    }

    /// <summary>
    /// `capabilities.crystal_sphere` of an advice reply (top level, else
    /// `advice.capabilities`); 0 when absent -- an older advisor, which F7
    /// never asks.
    /// </summary>
    public static int Capability(JsonElement adviceReply) =>
        PotionSwapAdvice.CapabilityOf(adviceReply, AdviceFor);

    /// <summary>
    /// The cell option the advisor's reply names, or -1 when F7 must play
    /// CrystalSpherePick instead: no `advice`, a `hand_back`, no `best`, a
    /// `best` that is not a `choose`, or an index outside
    /// [0, <paramref name="optionCount"/>).
    /// </summary>
    public static int AdvisedIndex(JsonElement adviceReply, int optionCount)
    {
        if (adviceReply.ValueKind != JsonValueKind.Object ||
            !adviceReply.TryGetProperty("advice", out JsonElement advice) ||
            advice.ValueKind != JsonValueKind.Object)
        {
            return -1;
        }
        if (advice.TryGetProperty("hand_back", out JsonElement handBack) &&
            handBack.ValueKind == JsonValueKind.String &&
            !string.IsNullOrWhiteSpace(handBack.GetString()))
        {
            return -1;
        }
        if (!advice.TryGetProperty("best", out JsonElement best) ||
            best.ValueKind != JsonValueKind.Object ||
            !best.TryGetProperty("action", out JsonElement action) ||
            action.ValueKind != JsonValueKind.String ||
            action.GetString() != "choose" ||
            !best.TryGetProperty("index", out JsonElement indexEl) ||
            indexEl.ValueKind != JsonValueKind.Number ||
            !indexEl.TryGetInt32(out int index))
        {
            return -1;
        }
        return index >= 0 && index < optionCount ? index : -1;
    }

    /// <summary>
    /// `advice.policy` of an advice reply: the policy that actually decided
    /// (`crystal_sphere_digger`, or `crystal_sphere_rule` when the advisor
    /// played the #872 rule because the digger could not answer), or null
    /// when the reply names none.
    /// </summary>
    public static string? AdvisedPolicy(JsonElement adviceReply)
    {
        if (adviceReply.ValueKind == JsonValueKind.Object &&
            adviceReply.TryGetProperty("advice", out JsonElement advice) &&
            advice.ValueKind == JsonValueKind.Object &&
            advice.TryGetProperty("policy", out JsonElement policy) &&
            policy.ValueKind == JsonValueKind.String &&
            !string.IsNullOrWhiteSpace(policy.GetString()))
        {
            return policy.GetString();
        }
        return null;
    }

    /// <summary>
    /// Who decided a played reveal, for the action log: the advisor's digger,
    /// the advisor's #872 rule, another policy the advisor named, or -- when
    /// the advisor's answer was not used (<paramref name="advised"/> false) --
    /// the mod's own block order (CrystalSpherePick).
    /// </summary>
    public static string ClickSource(bool advised, string? advisedPolicy)
    {
        if (!advised)
        {
            return "block order";
        }
        return advisedPolicy switch
        {
            "crystal_sphere_digger" => "advisor's digger",
            "crystal_sphere_rule" => "advisor's #872 rule",
            null => "advisor, policy not named",
            _ => $"advisor's {advisedPolicy}",
        };
    }

    /// <summary>
    /// Who is in charge, sampled when F7 (or F6) starts an ask and again
    /// before its delayed click: Advisor mode, autoplay armed, and
    /// SimAdvisor's autoplay generation (bumped on every arm / disarm).
    /// </summary>
    public readonly record struct Takeover(bool AdvisorMode, bool AutoPlay, int AutoPlayGeneration);

    /// <summary>
    /// The delayed reveal may click only in Advisor mode and only if nothing
    /// about autoplay changed since the ask started: F7 pressed (either
    /// way), autobattle toggled, or Advisor mode left (which also disarms)
    /// all drop it. An F6 one-shot (autoplay off at the ask, still off,
    /// same generation) still clicks.
    /// </summary>
    public static bool MayStillClick(Takeover atAsk, Takeover now) =>
        now.AdvisorMode && atAsk == now;
}
