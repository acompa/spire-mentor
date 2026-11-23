// FrameLaunchRules.cs -- the pure half of SimFrameLauncher: where the frame
// lives, which page it shows and its command line. No Godot, so the unit
// tests (tests/unit) compile it.

using System.Collections.Generic;
using System.IO;

namespace SpireMentor;

internal static class FrameLaunchRules
{
    /// <summary>The frame's folder under the mod's (package.ps1 and the
    /// deploy put it there).</summary>
    public const string FrameDirName = "SpireMentorFrame";

    public const string FrameExeName = "SpireMentorFrame.exe";

    /// <summary>The Linux host (spire-mentor-frame/linux/): one Python file
    /// in the same folder, run by the HOST's python3 (PyGObject, WebKitGTK).</summary>
    public const string LinuxScriptName = "spire_mentor_frame.py";

    /// <summary>The packaged frame next to SpireMentor.dll.</summary>
    public static string ExePath(string modDir) => Path.Combine(modDir, FrameDirName, FrameExeName);

    /// <summary>The packaged Linux host next to SpireMentor.dll.</summary>
    public static string LinuxScriptPath(string modDir) => Path.Combine(modDir, FrameDirName, LinuxScriptName);

    /// <summary>Where a frame can adopt the game's window: Windows, and Linux
    /// while the game runs on X11 or XWayland (a native Wayland window cannot
    /// be reparented).</summary>
    public static bool Supported(bool windows, bool linux, string displayServer) =>
        windows || (linux && displayServer == "X11");

    /// <summary>The Linux host's command line. Under Steam the game runs in
    /// the Steam Linux Runtime container, which has no web view, so the frame
    /// goes through Steam's launcher service and runs on the host beside
    /// Steam (also not a descendant of the game, which Steam would wait for
    /// on quit). Outside the container python3 is started directly.</summary>
    public static (string File, IReadOnlyList<string> Args) LinuxCommand(
        bool inSteamRuntime, string script, IReadOnlyList<string> frameArgs)
    {
        var args = new List<string>();
        if (inSteamRuntime)
        {
            args.Add("--alongside-steam");
            args.Add("--");
            args.Add("python3");
        }
        args.Add(script);
        args.AddRange(frameArgs);
        return (inSteamRuntime ? "steam-runtime-launch-client" : "python3", args);
    }

    /// <summary>The game is inside the Steam Linux Runtime container and
    /// Steam's launcher service is reachable from it.</summary>
    public static bool InSteamRuntime(string? container, string? launcherService) =>
        container == "pressure-vessel" && !string.IsNullOrEmpty(launcherService);

    /// <summary>The Linux host's exit code when the host has no PyGObject /
    /// GTK 3 / WebKitGTK (spire_mentor_frame.py EXIT_NO_TOOLKIT).</summary>
    public const int LinuxExitNoToolkit = 4;

    /// <summary>Open the frame when the game starts: the player wants it (the
    /// default), no frame is there yet, a frame can open here
    /// (<see cref="Supported"/>), and not in a test lane.</summary>
    public static bool OpenAtStartup(bool wanted, bool framed, bool supported, bool harness) =>
        wanted && !framed && supported && !harness;

    /// <summary>The button beside the badge that puts the panels back around
    /// the game, or null for none. Framed fullscreen (the panels in the
    /// game): "Exit fullscreen". Unframed (the frame closed, the panels in
    /// a plain game window): "Panels beside the game" -- otherwise the only
    /// way back is Panels ▾, where players did not find it. Never where the
    /// frame cannot open (not <see cref="Supported"/>, not shipped, a test lane).</summary>
    public static string? PanelsButtonLabel(bool framed, bool uiOutside, bool supported, bool frameShipped, bool harness)
    {
        if (framed)
        {
            return uiOutside ? null : ExitFullscreenLabel;
        }
        return supported && frameShipped && !harness ? PanelsBesideLabel : null;
    }

    public const string ExitFullscreenLabel = "⛶ Exit fullscreen";

    public const string PanelsBesideLabel = "⧉ Panels beside the game";

    /// <summary>The page the frame shows: the configured URL when set (dev:
    /// the Vite server), else the bundled workbench's frame page on the
    /// advisor's control plane.</summary>
    public static string PageUrl(string? configured, int controlPort)
    {
        string c = (configured ?? "").Trim();
        return c.Length > 0 ? c : $"http://127.0.0.1:{controlPort}/frame.html";
    }

    /// <summary>The frame's arguments in attach mode (spire-mentor-frame/README.md):
    /// adopt this game's window, relay on <paramref name="port"/>.</summary>
    public static IReadOnlyList<string> Args(int pid, long hwnd, int port, string url, string? gameDir)
    {
        var args = new List<string>
        {
            "--attach", pid.ToString(System.Globalization.CultureInfo.InvariantCulture),
            "--port", port.ToString(System.Globalization.CultureInfo.InvariantCulture),
            "--url", url,
        };
        if (hwnd != 0)
        {
            args.Add("--attach-hwnd");
            args.Add(hwnd.ToString(System.Globalization.CultureInfo.InvariantCulture));
        }
        if (!string.IsNullOrEmpty(gameDir))
        {
            args.Add("--game-dir");
            args.Add(gameDir!);
        }
        return args;
    }
}
