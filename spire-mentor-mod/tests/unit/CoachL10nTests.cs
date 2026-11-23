// CoachL10nTests.cs -- the coach chip / panel copy in Simplified Chinese
// (ZhText.Coach.cs). The English copy is pinned in Program.cs.

using System.Text.Json.Nodes;
using SpireMentor;

internal static class CoachL10nTests
{
    public static void Run(Action<string, string?, string?> check)
    {
        string? savedOverride = L10n.Override;
        Func<string?>? savedProbe = L10n.GameLanguage;
        try
        {
            L10n.Override = null;
            L10n.GameLanguage = () => "zhs";
            var rec = (JsonObject)JsonNode.Parse(
                """{"kind":"turn","id":"t1","verdict":"blunder","turn":3,"you":["Strike"],"ai":["Defend"],"hp_drop":6.4,"win_before":0.8,"win_after":0.6,"peeked":true}""")!;
            ChipText chip = CoachChipText.Format(rec, showGood: false)!;
            check("zh chip: title", chip.Title, "⚠ 失误");
            check("zh chip: turn line", chip.Lines[0], "第3回合 · 你：Strike");
            check("zh chip: cost line", chip.Lines[2], "多损失 ≈ 6 生命 · 胜率 80% → 60%（已看提示）");
            check("zh chip: ordinal", CoachChipText.Ordinal(2), "第2");

            var recap = (JsonObject)JsonNode.Parse(
                """{"kind":"recap","id":"r1","floor":7,"hp_lost":12,"turns_ungraded":2}""")!;
            PanelText panel = CoachPanelText.Format(recap)!;
            check("zh panel: title", panel.Title, "战斗复盘 · 第7层");
            check("zh panel: summary", panel.Summary, "−12 生命 · 2 个回合未评分");
            check("zh estimate: headline",
                InstinctEstimateText.Headline(new InstinctEstimate(0.71, 1.0, null, null, null)), "本局 71% · 战斗 100%");
        }
        finally
        {
            L10n.Override = savedOverride;
            L10n.GameLanguage = savedProbe;
        }
        check("en chip: ordinal after zh", CoachChipText.Ordinal(2), "2nd");
    }
}
