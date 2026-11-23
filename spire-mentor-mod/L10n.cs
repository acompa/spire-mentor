// L10n.cs -- the mod's own UI language (English, Simplified Chinese).
//
// The mod speaks the game's language: LocManager.Instance.Language is "zhs"
// in a game set to Simplified Chinese (SimLanguage.cs wires that probe in at
// startup), and every other language reads English, as before. Game content
// (card, relic, monster names) already comes from the game in its own
// language; this file covers only the strings the mod writes itself.
//
// Strings are keyed by their English text, gettext-style:
//   L10n.T("End turn")                 -> "End turn" / "结束回合"
//   L10n.F("Turn {0} · you: {1}", n, s) -> string.Format over the translation
// A string missing from the Chinese table falls back to its English key, so an
// untranslated label reads English, never blank. Formatting is always
// InvariantCulture (numbers keep their "." and no grouping), as the copy was.
//
// The tables are ZhText.*.cs, one per area of the mod, each a
// `static readonly (string En, string Zh)[]` field of the partial class
// ZhText; they are found by reflection, so a new table needs no
// registration. tests/unit checks every literal T()/F() key has an entry
// with the same {n} placeholders.
//
// Game-free on purpose (tests/unit links it).

using System;
using System.Collections.Generic;
using System.Globalization;
using System.Reflection;

namespace SpireMentor;

internal static class L10n
{
    /// <summary>The game's language code (LocManager.Language: "eng",
    /// "zhs", "deu"...), or null before the game has one. Set by
    /// SimLanguage at startup; tests set it directly.</summary>
    public static Func<string?>? GameLanguage;

    /// <summary>A forced language ("en" / "zh") that wins over the game's,
    /// from the config's <c>language</c> key; null = follow the game.</summary>
    public static string? Override;

    /// <summary>"zh" when the mod should speak Simplified Chinese, else "en".</summary>
    public static string Lang
    {
        get
        {
            if (Override is "en" or "zh")
            {
                return Override;
            }
            string? code;
            try
            {
                code = GameLanguage?.Invoke();
            }
            catch (Exception)
            {
                code = null;
            }
            return FromGameCode(code);
        }
    }

    public static bool IsZh => Lang == "zh";

    /// <summary>The mod's language for a game language code ("zhs" -> zh;
    /// "zht" too, should a build ship it: the Simplified table beats
    /// English for a Traditional reader). Everything else is English.</summary>
    public static string FromGameCode(string? code)
    {
        string c = (code ?? "").Trim().ToLowerInvariant();
        return c is "zhs" or "zht" or "zh" || c.StartsWith("zh-", StringComparison.Ordinal) ||
               c.StartsWith("zh_", StringComparison.Ordinal)
            ? "zh"
            : "en";
    }

    /// <summary>A config value ("auto", "en", "zh", "zhs", "chinese", ...)
    /// as an <see cref="Override"/>: null for auto / unknown.</summary>
    public static string? ParseOverride(string? value)
    {
        string v = (value ?? "").Trim().ToLowerInvariant();
        return v switch
        {
            "en" or "eng" or "english" => "en",
            "zh" or "zhs" or "zh-hans" or "zh-cn" or "chinese" or "中文" => "zh",
            _ => null,
        };
    }

    /// <summary>BCP 47 tag for the mod's language ("en" / "zh"), e.g. for the
    /// workbench URL's <c>?lang=</c>.</summary>
    public static string Tag => Lang;

    /// <summary><paramref name="en"/> in the mod's language.</summary>
    public static string T(string en)
    {
        if (!IsZh)
        {
            return en;
        }
        return Zh.TryGetValue(en, out string? zh) ? zh : en;
    }

    /// <summary><see cref="T"/> then <c>string.Format</c> (InvariantCulture)
    /// with <paramref name="args"/> for {0}, {1}, ...</summary>
    public static string F(string en, params object?[] args)
    {
        string template = T(en);
        try
        {
            return string.Format(CultureInfo.InvariantCulture, template, args);
        }
        catch (FormatException)
        {
            // A broken translation never costs the line: fall back to English.
            return string.Format(CultureInfo.InvariantCulture, en, args);
        }
    }

    private static Dictionary<string, string>? _zh;

    /// <summary>Every ZhText table merged (English -> Chinese).</summary>
    internal static Dictionary<string, string> Zh => _zh ??= LoadZh();

    private static Dictionary<string, string> LoadZh()
    {
        var map = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (FieldInfo f in typeof(ZhText).GetFields(
                     BindingFlags.Static | BindingFlags.NonPublic | BindingFlags.Public))
        {
            if (f.GetValue(null) is (string En, string Zh)[] rows)
            {
                foreach ((string en, string zh) in rows)
                {
                    map[en] = zh;
                }
            }
        }
        return map;
    }
}

/// <summary>The Chinese tables (ZhText.*.cs). Each file adds one
/// <c>static readonly (string En, string Zh)[]</c> field.</summary>
internal static partial class ZhText
{
}
