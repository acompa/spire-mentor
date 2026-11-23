// AdvisorBrainRules.cs -- which brain answers the player's advice: Instinct,
// with Search as its fallback.
//
// Instinct is the end-to-end net (no search) playing every decision; Search is
// what the advisor always was: MCTS for the fights, the PPO run policy for the
// screens. Instinct is the only brain a player gets: nothing in game (badge,
// hotkey, settings, the frame) switches it. A request asks for Instinct
// wherever the advisor's `capabilities.instinct` covers the run (or before it
// has said), and Search elsewhere; the ADVISOR still decides per request
// whether Instinct can answer, and answers with Search, saying why
// (`instinct_unavailable`), where it cannot (PROTOCOL.md section 5, v5.2).
//
// Test tooling alone can force a brain for a session: the launch arg
// `--spire-mentor-brain search|instinct`, else SpireMentor.conf's
// `advisor_brain` (written by tools/ascension_climb.py); both land in
// SimConfig.AdvisorBrain. It applies to the player's seat and to the AI
// partners' brain (PartnerDefault) alike. The brain an older version's
// in-game switch saved (BaseLib's SpireMentor.cfg `AdvisorBrain`) is read by
// nothing: BaseLib skips a key no property claims and drops it on the next
// save, so a player who once picked Search gets Instinct.
//
// Game-free (System.Text.Json only) so tests/unit pins the rules: the
// capability parse, the brain each seat asks for, which reply is stale, and
// the HUD's source tag.

using System;
using System.Collections.Generic;
using System.Text.Json;

namespace SpireMentor;

/// <summary>One `capabilities.instinct` entry: a character + game build this
/// advisor's Instinct can play.</summary>
internal sealed record InstinctTarget(string Character, string GameVersion);

internal static class AdvisorBrainRules
{
    public const string Search = "search";
    public const string Instinct = "instinct";

    /// <summary>The wire value for a brain: anything but "instinct"
    /// (case-insensitive, trimmed) is Search, so nothing can send a brain the
    /// advisor does not know.</summary>
    public static string Normalize(string? value) =>
        string.Equals(value?.Trim(), Instinct, StringComparison.OrdinalIgnoreCase) ? Instinct : Search;

    public static bool IsInstinct(string? value) => Normalize(value) == Instinct;

    /// <summary>
    /// A tooling override's value (SpireMentor.conf's <c>advisor_brain</c>,
    /// the <c>--spire-mentor-brain</c> launch arg): "search" / "instinct"
    /// (case-insensitive, trimmed), else null -- absent, blank or a typo.
    /// Unlike <see cref="Normalize"/> a typo is NOT read as Search: an
    /// override nobody meant must not happen (SimConfig logs it instead).
    /// </summary>
    public static string? ParseConf(string? value)
    {
        string v = value?.Trim() ?? "";
        if (v.Equals(Instinct, StringComparison.OrdinalIgnoreCase))
        {
            return Instinct;
        }
        return v.Equals(Search, StringComparison.OrdinalIgnoreCase) ? Search : null;
    }

    /// <summary>The player's brain when no test tooling forces one.</summary>
    public const string PlayerDefault = Instinct;

    /// <summary>
    /// The AI co-op partners' brain when no test tooling forces one: what
    /// every partner advice_request asks for (SimAdvisor.PartnerBrain).
    /// Instinct, as for the player: the partners' advisor answers with
    /// Instinct for the seat's character where it can, else with Search. A
    /// co-op benchmark flips this one value (or forces every seat with the
    /// tooling override); it is no player-visible choice.
    /// </summary>
    public const string PartnerDefault = Instinct;

    /// <summary>The test tooling's brain for this session: the launch arg
    /// (<c>--spire-mentor-brain</c>) over SpireMentor.conf's
    /// <c>advisor_brain</c>; null when neither names a brain (a typo is no
    /// override, see <see cref="ParseConf"/>).</summary>
    public static string? ToolingOverride(string? launchArg, string? conf) =>
        ParseConf(launchArg) ?? ParseConf(conf);

    /// <summary>The player's brain: the tooling override, else
    /// <see cref="PlayerDefault"/>. No saved setting takes part, so a Search
    /// an older version's switch saved cannot keep a player on Search.</summary>
    public static string Player(string? toolingOverride) => ParseConf(toolingOverride) ?? PlayerDefault;

    /// <summary>The AI partners' brain: the tooling override, else
    /// <see cref="PartnerDefault"/>.</summary>
    public static string Partner(string? toolingOverride) => ParseConf(toolingOverride) ?? PartnerDefault;

    /// <summary>
    /// `capabilities.instinct` of an advice reply: top level, next to
    /// `advice`, as the advisor sends it; `advice.capabilities` is accepted
    /// too (the same two places as `capabilities.coach`). Empty when absent,
    /// malformed, or the advisor has no Instinct net -- an older advisor.
    /// Entries without both fields are skipped.
    /// </summary>
    public static IReadOnlyList<InstinctTarget> ParseCapability(JsonElement root)
    {
        var targets = new List<InstinctTarget>();
        if (root.ValueKind != JsonValueKind.Object)
        {
            return targets;
        }
        if (!TryInstinctArray(root, out JsonElement arr) &&
            !(root.TryGetProperty("advice", out JsonElement advice) && TryInstinctArray(advice, out arr)))
        {
            return targets;
        }
        foreach (JsonElement e in arr.EnumerateArray())
        {
            if (e.ValueKind != JsonValueKind.Object)
            {
                continue;
            }
            string? character = ReadString(e, "character");
            string? version = ReadString(e, "game_version");
            if (!string.IsNullOrWhiteSpace(character) && !string.IsNullOrWhiteSpace(version))
            {
                targets.Add(new InstinctTarget(character.Trim(), version.Trim()));
            }
        }
        return targets;
    }

    private static bool TryInstinctArray(JsonElement owner, out JsonElement arr)
    {
        arr = default;
        return owner.ValueKind == JsonValueKind.Object &&
               owner.TryGetProperty("capabilities", out JsonElement caps) &&
               caps.ValueKind == JsonValueKind.Object &&
               caps.TryGetProperty("instinct", out arr) &&
               arr.ValueKind == JsonValueKind.Array;
    }

    /// <summary>
    /// The advisor's Instinct can play this run: its capability names the
    /// run's character and game build. Case-insensitive on both (the run
    /// says "IRONCLAD", the advisor "Ironclad"); the build compares in wire
    /// form ("v0.111.0", GameVersionFormat) so "0.111.0" matches too.
    /// </summary>
    public static bool Eligible(IReadOnlyList<InstinctTarget> targets, string? character, string? gameVersion)
    {
        if (targets.Count == 0 || string.IsNullOrWhiteSpace(character))
        {
            return false;
        }
        string build = GameVersionFormat.Normalize(gameVersion) ?? GameVersionFormat.Default;
        foreach (InstinctTarget t in targets)
        {
            // A hotfix plays on its base build, so v0.113.1 matches a v0.113.0 target.
            if (string.Equals(t.Character, character.Trim(), StringComparison.OrdinalIgnoreCase) &&
                GameVersionFormat.EngineBuild(build, new[] { t.GameVersion }) != null)
            {
                return true;
            }
        }
        return false;
    }

    /// <summary>
    /// The brain the player's request asks the advisor for:
    /// <paramref name="brain"/> (<see cref="Player"/>, Instinct unless tooling
    /// forces Search), except that Instinct is asked for only in a run it
    /// plays. Elsewhere the search answers without the "Search (Instinct:
    /// ...)" tag on every reply. <paramref name="known"/>: the advisor has
    /// said which runs Instinct plays (its capability); until then Instinct
    /// is asked for and the advisor falls back where it must.
    /// </summary>
    public static string Requested(string? brain, bool known, bool eligible) =>
        IsInstinct(brain) && (eligible || !known) ? Instinct : Search;

    /// <summary>
    /// A reply that must not be rendered because the brain to ask for changed
    /// after it was asked for (the advisor's capability arrived and says
    /// Instinct does not play this run): <paramref name="requested"/> is the
    /// brain the request was stamped with, <paramref name="current"/> the
    /// brain a request would ask for now.
    ///
    /// Deliberately NOT keyed on the reply's own `advice.brain`: a Search
    /// answer to an Instinct request is the advisor falling back
    /// (`instinct_unavailable`), the right thing to show; and an Instinct
    /// answer to a Search request can only be an advisor bug, where dropping
    /// it would leave the decision with no advice at all. The stamp is what
    /// tells an outdated reply, and it cannot loop: the re-request is
    /// stamped with the brain asked for now.
    /// </summary>
    public static bool IsStale(string requested, string current) => Normalize(requested) != Normalize(current);

    /// <summary>
    /// The HUD's source tag for a reply: "Instinct" when the net answered,
    /// "Search (Instinct: reason)" when Instinct was asked for and the search
    /// answered instead, null for a plain search reply (the common case,
    /// which says nothing).
    /// </summary>
    public static string? SourceTag(string? answered, string? instinctUnavailable)
    {
        if (answered != null && IsInstinct(answered))
        {
            return "Instinct";
        }
        if (!string.IsNullOrWhiteSpace(instinctUnavailable))
        {
            return $"Search (Instinct: {instinctUnavailable.Trim()})";
        }
        return null;
    }

    /// <summary>
    /// The advice note as the toolbar's status line shows it, with the source
    /// in front when it is not the plain search: "Instinct · note", or the
    /// fallback tag when the advisor's note does not already carry the reason
    /// (the advisor puts it there too; it must not read twice).
    /// </summary>
    public static string? NoteWithSource(string? note, string? answered, string? instinctUnavailable)
    {
        string? tag = SourceTag(answered, instinctUnavailable);
        if (tag == null)
        {
            return note;
        }
        if (string.IsNullOrWhiteSpace(note))
        {
            return tag;
        }
        if (!IsInstinct(answered) && note.Contains(instinctUnavailable!.Trim(), StringComparison.OrdinalIgnoreCase))
        {
            return note;
        }
        return $"{tag} · {note}";
    }

    private static string? ReadString(JsonElement el, string name) =>
        el.TryGetProperty(name, out JsonElement v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;
}
