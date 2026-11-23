// ZhText.Common.cs -- shared mod UI words in Simplified Chinese (L10n.cs).
// Game terms follow the game's own Chinese localization where it has one.

namespace SpireMentor;

internal static partial class ZhText
{
    private static readonly (string En, string Zh)[] Common =
    {
        ("On", "开"),
        ("Off", "关"),
        ("OK", "确定"),
        ("Cancel", "取消"),
        ("Close", "关闭"),
    };
}
