// CoachPanelText.cs -- the words on the fight-recap and death-autopsy panels,
// from one `recap` / `autopsy` coach record (PROTOCOL.md section 5, v5.1).
// Game-free so tests/unit can pin the copy; SimCoachPanel only lays it out.
//
// Same conventions as the chip (CoachChipText): "≈" before an expected HP
// figure, " → " between moves, "(peeked)" on peeked decisions. A turn is
// never named without its position (hand, energy, intents): the advisor only
// lists turns that carry a `context`.

using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using System.Text.Json.Nodes;

namespace SpireMentor;

/// <summary>One block of a panel; <see cref="LogIdx"/> gives it an "open in
/// workbench" button.</summary>
internal sealed class PanelSection
{
    public string Heading = "";
    public List<string> Lines = new();
    public int? LogIdx;
}

/// <summary>What a recap / autopsy panel says.</summary>
internal sealed class PanelText
{
    public string Id = "";
    public string Kind = "";
    public string Title = "";
    public string Summary = "";
    public List<PanelSection> Sections = new();
    public string Footer = "";
    public string? LogName;
}

internal static class CoachPanelText
{
    /// <summary>The panel for <paramref name="rec"/>, or null for any other
    /// kind or a malformed record.</summary>
    public static PanelText? Format(JsonObject rec)
    {
        try
        {
            return CoachChipText.Str(rec, "kind") switch
            {
                "recap" => Recap(rec),
                "autopsy" => Autopsy(rec),
                _ => null,
            };
        }
        catch (Exception)
        {
            return null; // a malformed record is silence, never a crash
        }
    }

    // ------------------------------------------------------------------

    private static PanelText Recap(JsonObject rec)
    {
        var text = new PanelText
        {
            Id = CoachChipText.Str(rec, "id"),
            Kind = "recap",
            Title = L10n.F("Fight recap · floor {0}", Int(rec, "floor")),
        };
        (text.LogName, _) = CoachChipText.LogPosition(rec);

        string summary = L10n.F("−{0} HP", Int(rec, "hp_lost"));
        if (CoachChipText.Num(rec, "ai_hp_lost") is double ai)
        {
            summary += L10n.F(" (AI line typically −{0})", CoachChipText.Whole(ai));
        }
        int ungraded = Int(rec, "turns_ungraded");
        if (ungraded > 0)
        {
            summary += " · " + TurnsUngraded(ungraded);
        }
        text.Summary = summary;

        if (rec["costliest"] is JsonArray costliest)
        {
            foreach (JsonObject turn in costliest.OfType<JsonObject>())
            {
                List<string> block = TurnBlock(turn);
                text.Sections.Add(new PanelSection
                {
                    Heading = block[0],
                    Lines = block.Skip(1).ToList(),
                    LogIdx = CoachChipText.LogPosition(turn).Idx,
                });
                text.LogName ??= CoachChipText.LogPosition(turn).Name;
            }
        }
        return text;
    }

    private static PanelText Autopsy(JsonObject rec)
    {
        var text = new PanelText
        {
            Id = CoachChipText.Str(rec, "id"),
            Kind = "autopsy",
            Title = L10n.F("Run autopsy · died on floor {0}", Int(rec, "floor")),
            Footer = DecisionsGraded(Int(rec, "decisions_graded")) + " · " +
                     TurnsUngraded(Int(rec, "turns_ungraded")),
        };
        (text.LogName, _) = CoachChipText.LogPosition(rec);

        int n = 0;
        if (rec["items"] is JsonArray items)
        {
            foreach (JsonObject item in items.OfType<JsonObject>())
            {
                if (item["record"] is not JsonObject record)
                {
                    continue;
                }
                string kind = CoachChipText.Str(item, "kind");
                var section = new PanelSection { LogIdx = CoachChipText.LogPosition(record).Idx };
                if (kind == "turn")
                {
                    List<string> block = TurnBlock(record);
                    block[0] = L10n.F("Floor {0} · {1}", Int(record, "floor"), block[0]);
                    section.Heading = L10n.F("{0}. Combat turn", n + 1);
                    section.Lines = block;
                }
                else if (kind == "choice" && CoachChipText.Choice(record, showGood: false) is ChipText chip)
                {
                    section.Heading = $"{n + 1}. {CoachChipText.Str(record, "label_kind")}";
                    section.Lines.Add(chip.Title);
                    section.Lines.AddRange(chip.Lines);
                }
                else
                {
                    continue;
                }
                n++;
                text.Sections.Add(section);
                text.LogName ??= CoachChipText.LogPosition(record).Name;
            }
        }
        text.Summary = n == 0 ? L10n.T("No costly decisions found.") : "";
        return text;
    }

    /// <summary>The spec's turn block: position, then both lines, then the
    /// cost, then the reason when there is one. The first entry is the
    /// "Turn N — hand: ..." line.</summary>
    private static List<string> TurnBlock(JsonObject turn)
    {
        var lines = new List<string>();
        int number = Int(turn, "turn");
        if (turn["context"] is JsonObject ctx)
        {
            string hand = string.Join(", ", CoachChipText.StrList(ctx, "hand"));
            lines.Add(L10n.F("Turn {0} — hand: {1} · {2} energy", number,
                hand.Length > 0 ? hand : L10n.T("(empty)"), Int(ctx, "energy")));
            if (ctx["enemies"] is JsonArray enemies)
            {
                foreach (JsonObject e in enemies.OfType<JsonObject>())
                {
                    string intent = CoachChipText.Str(e, "intent");
                    if (intent.Length > 0)
                    {
                        lines.Add(L10n.F("{0} intends: {1}", CoachChipText.Str(e, "name"), intent));
                    }
                }
            }
        }
        else
        {
            lines.Add(L10n.F("Turn {0}", number));
        }
        lines.Add(L10n.F("You: {0}", string.Join(" → ", CoachChipText.StrList(turn, "you"))));
        lines.Add(L10n.F("AI:  {0}", string.Join(" → ", CoachChipText.StrList(turn, "ai"))));

        string peeked = CoachChipText.Bool(turn, "peeked") ? L10n.T(" (peeked)") : "";
        if (CoachChipText.Num(turn, "hp_drop") is double drop &&
            (long)Math.Round(drop, MidpointRounding.AwayFromZero) >= 1)
        {
            lines.Add(L10n.F("≈ {0} HP more lost", CoachChipText.Whole(drop)) + peeked);
        }
        else
        {
            lines.Add(L10n.F("win {0} → {1}", CoachChipText.Pct(CoachChipText.Num(turn, "win_before")),
                      CoachChipText.Pct(CoachChipText.Num(turn, "win_after"))) + peeked);
        }
        string why = CoachChipText.Str(turn, "why");
        if (why.Length > 0)
        {
            lines.Add(why);
        }
        return lines;
    }

    private static int Int(JsonObject o, string key) =>
        CoachChipText.Num(o, key) is double d ? (int)Math.Round(d, MidpointRounding.AwayFromZero) : 0;

    private static string TurnsUngraded(int n) =>
        n == 1 ? L10n.F("{0} turn ungraded", n) : L10n.F("{0} turns ungraded", n);

    private static string DecisionsGraded(int n) =>
        n == 1 ? L10n.F("{0} decision graded", n) : L10n.F("{0} decisions graded", n);
}
