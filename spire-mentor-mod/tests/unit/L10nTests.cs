// L10nTests.cs -- the mod's UI language (L10n.cs) and its Chinese tables.

using System.Runtime.CompilerServices;
using System.Text.RegularExpressions;
using SpireMentor;

internal static class L10nTests
{
    private static string ModDir([CallerFilePath] string here = "") =>
        Path.GetFullPath(Path.Combine(Path.GetDirectoryName(here)!, "..", ".."));

    // L10n.T("...") / L10n.F("...", ...) with a plain or verbatim literal.
    private static readonly Regex Call = new(
        @"\bL10n\.(?:T|F)\(\s*(@?)""((?:[^""\\\n]|\\.|"""")*)""",
        RegexOptions.Compiled);

    private static string Unescape(string body, bool verbatim) =>
        verbatim ? body.Replace("\"\"", "\"") : Regex.Unescape(body);

    private static string Holes(string s) =>
        string.Join(",", Regex.Matches(s, @"\{(\d+)(?:[,:][^}]*)?\}")
            .Select(m => m.Groups[1].Value).Distinct().OrderBy(x => x));

    public static void Run(Action<string, string?, string?> check)
    {
        string? savedOverride = L10n.Override;
        Func<string?>? savedProbe = L10n.GameLanguage;
        try
        {
            L10n.Override = null;
            L10n.GameLanguage = () => "eng";
            check("l10n: eng -> en", L10n.Lang, "en");
            check("l10n: english passthrough", L10n.T("Cancel"), "Cancel");
            L10n.GameLanguage = () => "zhs";
            check("l10n: zhs -> zh", L10n.Lang, "zh");
            check("l10n: zh lookup", L10n.T("Cancel"), "取消");
            check("l10n: zh missing falls back", L10n.T("__no such key__"), "__no such key__");
            check("l10n: F formats invariant", L10n.F("{0} of {1}", 1.5, 2), "1.5 of 2");
            L10n.GameLanguage = () => throw new InvalidOperationException("no game");
            check("l10n: probe throws -> en", L10n.Lang, "en");
            L10n.GameLanguage = () => "zhs";
            L10n.Override = "en";
            check("l10n: override en wins", L10n.Lang, "en");
            check("l10n: parse auto", L10n.ParseOverride("auto") ?? "null", "null");
            check("l10n: parse zh", L10n.ParseOverride(" Chinese ") ?? "null", "zh");
            check("l10n: deu -> en", L10n.FromGameCode("deu"), "en");
        }
        finally
        {
            L10n.Override = savedOverride;
            L10n.GameLanguage = savedProbe;
        }

        // Every literal key has a Chinese entry with the same {n} holes.
        var missing = new List<string>();
        var badHoles = new List<string>();
        foreach (string file in Directory.GetFiles(ModDir(), "*.cs")
                     .Where(f => Path.GetFileName(f) != "L10n.cs"))
        {
            string code = File.ReadAllText(file);
            foreach (Match m in Call.Matches(code))
            {
                string key = Unescape(m.Groups[2].Value, m.Groups[1].Value == "@");
                if (!L10n.Zh.TryGetValue(key, out string? zh))
                {
                    missing.Add($"{Path.GetFileName(file)}: {key}");
                }
                else if (Holes(zh) != Holes(key))
                {
                    badHoles.Add($"{key} -> {zh}");
                }
            }
        }
        check("l10n: every key translated", string.Join("\n", missing), "");
        check("l10n: translations keep their {n}", string.Join("\n", badHoles), "");
    }
}
