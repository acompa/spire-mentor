// SimLanguage.cs -- wires the game's language into L10n (the mod's UI copy).
//
// LocManager.Instance is created by the game after the mod initializes, and
// the player can switch languages in the settings at any time, so L10n reads
// LocManager.Instance.Language on every lookup (a property read) rather than
// caching it. The config's `language` key ("auto" / "en" / "zh") overrides.

using System;
using MegaCrit.Sts2.Core.Localization;

namespace SpireMentor;

internal static class SimLanguage
{
    public static void Install(SimConfig config)
    {
        L10n.Override = L10n.ParseOverride(config.Language);
        L10n.GameLanguage = static () => LocManager.Instance?.Language;
        Logger.Log($"Mod language: {(L10n.Override ?? "auto (follows the game)")}");
    }
}
