// SimFrameLauncher.cs -- "Panels in their own window": the mod starts the
// Spire Mentor frame (spire-mentor-frame/, shipped in mods/SpireMentor/
// SpireMentorFrame/) next to the running game. The frame adopts the game's
// window, lays the panels out around it and shows the bundled workbench's
// frame page from the advisor's control plane; FrameLink feeds it.
//
// The player's switch is SpireMentorModConfig.PanelsInOwnWindow (settings, the
// toolbar's Panels menu): on at startup opens the frame once the game window is
// up, ticking it opens it now, unticking it asks the frame to hand the game
// its window back. Closing the frame window does the same and unticks it.
//
// A game the frame LAUNCHED (--spire-mentor-frame-port, the dev lane) is
// framed already; nothing here touches it.
//
// Never throws into the game: every entry point swallows and logs.

using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Reflection;
using Godot;

namespace SpireMentor;

internal static class SimFrameLauncher
{
    private static Process? _frame;
    // Set while the mod itself is closing the frame (unticked): its exit
    // then keeps the setting as the player left it.
    private static bool _closing;

    /// <summary>The frame the mod started is running.</summary>
    public static bool Running => _frame is { HasExited: false };

    /// <summary>A test lane: the advisor_autoplay harness,
    /// --spire-mentor-autoplay / -port and --fastmp (two games on one
    /// desktop). The frame stays shut there.</summary>
    public static bool Harness =>
        SimConfig.Instance.AdvisorAutoplay || SimMp.AutoplayOverride ||
        SimMp.PortOverride != null || SimMp.FastMpLaunch;

    private static bool? _shipped;

    /// <summary>The frame is in the mod folder (checked once).</summary>
    public static bool Shipped => _shipped ??= File.Exists(ExePath());

    /// <summary>The frame for this OS: the Windows exe, or the Linux host's
    /// script (FrameLaunchRules).</summary>
    private static string ExePath()
    {
        string? modDir = Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);
        if (modDir == null)
        {
            return "";
        }
        return OperatingSystem.IsWindows()
            ? FrameLaunchRules.ExePath(modDir)
            : FrameLaunchRules.LinuxScriptPath(modDir);
    }

    private static bool? _supported;

    /// <summary>A frame can adopt this game's window (checked once, on the
    /// main thread): Windows, or Linux with the game on X11 / XWayland.</summary>
    public static bool Supported => _supported ??= FrameLaunchRules.Supported(
        OperatingSystem.IsWindows(), OperatingSystem.IsLinux(), DisplayServer.GetName());

    /// <summary>The frame's log, for the notes that point at it.</summary>
    private static string FrameLogHint => OperatingSystem.IsWindows()
        ? "%LOCALAPPDATA%\\SpireMentor\\frame\\frame.log"
        : "~/.local/share/SpireMentor/frame/frame.log";

    /// <summary>At startup: open the frame when the player wants it, once
    /// the game window has settled (the game re-applies its display settings
    /// during the first frames).</summary>
    public static void OpenAtStartupIfWanted()
    {
        // On by default, so quiet where it cannot or should not open: off
        // Windows (no note at every start), and in the test lanes -- the
        // advisor_autoplay harness, --spire-mentor-autoplay / -port and
        // --fastmp (two games on one desktop).
        if (!FrameLaunchRules.OpenAtStartup(SpireMentorModConfig.PanelsInOwnWindow, FrameLink.Framed,
                Supported, Harness))
        {
            return;
        }
        Guard(() =>
        {
            SceneTree? tree = Engine.GetMainLoop() as SceneTree;
            if (tree == null)
            {
                return;
            }
            tree.CreateTimer(3.0).Timeout += () => Guard(() => Open("startup"));
        });
    }

    /// <summary>The player's switch (settings, toolbar): remember it and
    /// open or close the frame now.</summary>
    public static void SetWanted(bool wanted, string source)
    {
        Guard(() =>
        {
            if (SpireMentorModConfig.PanelsInOwnWindow != wanted)
            {
                SpireMentorModConfig.PanelsInOwnWindow = wanted;
                SpireMentorModConfig.SaveSoon();
            }
            if (wanted)
            {
                Open(source);
            }
            else
            {
                Close(source);
            }
        });
    }

    private static void Open(string source)
    {
        if (FrameLink.Framed)
        {
            // Framed with the panels inside: the frame is there, move them out.
            FrameLink.SetUiOutside(true);
            return;
        }
        if (!Supported)
        {
            SimActionLog.Note(OperatingSystem.IsLinux()
                ? L10n.T("Panels in their own window: on Linux the game must run on X11 or XWayland.")
                : L10n.T("Panels in their own window: Windows and Linux only."));
            return;
        }
        string exe = ExePath();
        if (!File.Exists(exe))
        {
            Logger.Log($"[SimFrameLauncher] no frame at {exe}");
            SimActionLog.Note(L10n.T("Panels in their own window: the frame is missing from the mod folder (reinstall the mod)."));
            return;
        }
        int port = FreeLoopbackPort();
        int controlPort = SimAdvisorProcess.ControlPort(BridgeServer.Instance.Port);
        // The panel page speaks the mod's language (?lang=zh; nothing in English).
        string url = WorkbenchUrl.WithLang(FrameLaunchRules.PageUrl(SpireMentorModConfig.FrameUrl, controlPort), L10n.Tag);
        // A fullscreen window fights being made a child; the frame is a
        // window, so the game is too while it sits inside.
        DisplayServer.WindowMode mode = DisplayServer.WindowGetMode();
        if (mode is DisplayServer.WindowMode.Fullscreen or DisplayServer.WindowMode.ExclusiveFullscreen)
        {
            DisplayServer.WindowSetMode(DisplayServer.WindowMode.Windowed);
        }
        long hwnd = DisplayServer.WindowGetNativeHandle(DisplayServer.HandleType.WindowHandle);
        string? gameDir = Path.GetDirectoryName(OS.GetExecutablePath());
        IReadOnlyList<string> frameArgs = FrameLaunchRules.Args(System.Environment.ProcessId, hwnd, port, url, gameDir);
        string file = exe;
        if (!OperatingSystem.IsWindows())
        {
            bool inSteamRuntime = FrameLaunchRules.InSteamRuntime(
                System.Environment.GetEnvironmentVariable("container"),
                System.Environment.GetEnvironmentVariable("SRT_LAUNCHER_SERVICE_ALONGSIDE_STEAM"));
            (file, frameArgs) = FrameLaunchRules.LinuxCommand(inSteamRuntime, exe, frameArgs);
        }
        var psi = new ProcessStartInfo(file)
        {
            UseShellExecute = false,
            WorkingDirectory = Path.GetDirectoryName(exe),
        };
        foreach (string a in frameArgs)
        {
            psi.ArgumentList.Add(a);
        }
        if (!OperatingSystem.IsWindows())
        {
            // The game's own library overrides (Steam's overlay, its bundled
            // FMOD) are not for the host's python.
            psi.Environment.Remove("LD_PRELOAD");
            psi.Environment.Remove("LD_LIBRARY_PATH");
        }
        Process frame = Process.Start(psi) ?? throw new InvalidOperationException($"could not start {exe}");
        frame.EnableRaisingEvents = true;
        frame.Exited += (_, _) => Callable.From(() => Guard(() => OnFrameExited(frame))).CallDeferred();
        _frame = frame;
        _closing = false;
        Logger.Log($"[SimFrameLauncher] started the frame ({source}): pid {frame.Id}, relay :{port}, page {url}");
        // Asked for just now (not the startup open): the panels go around
        // the game even when the frame was last left fullscreen.
        FrameLink.Attach(port, playerAsked: source != "startup");
    }

    private static void Close(string source)
    {
        if (!Running)
        {
            if (FrameLink.Framed && !FrameLink.UiOutside)
            {
                return; // a frame-launched game with the panels inside already
            }
            if (FrameLink.Framed)
            {
                // The frame launched the game: it cannot give the window
                // back, so just bring the panels inside.
                FrameLink.SetUiOutside(false);
            }
            return;
        }
        _closing = true;
        Logger.Log($"[SimFrameLauncher] closing the frame ({source})");
        if (!FrameLink.RequestClose())
        {
            // No link to ask on (the frame never connected): it holds no
            // game window yet either, so ending it is safe.
            try
            {
                _frame!.Kill();
            }
            catch (Exception ex)
            {
                Logger.Log($"[SimFrameLauncher] could not end the frame: {ex.Message}");
            }
        }
    }

    private static void OnFrameExited(Process frame)
    {
        if (!ReferenceEquals(frame, _frame))
        {
            return;
        }
        int code = SafeExitCode(frame);
        _frame = null;
        FrameLink.Detach();
        FrameFullscreen.AfterFrameClosed();
        if (_closing)
        {
            _closing = false;
            return;
        }
        // The player closed the frame window (or it failed): panels back in
        // the game, and the setting says so, so the next start does not
        // bring it back unasked.
        Logger.Log($"[SimFrameLauncher] the frame exited (code {code}): panels back in the game");
        if (SpireMentorModConfig.PanelsInOwnWindow)
        {
            SpireMentorModConfig.PanelsInOwnWindow = false;
            SpireMentorModConfig.SaveSoon();
        }
        if (code != 0)
        {
            SimActionLog.Note(code == FrameLaunchRules.LinuxExitNoToolkit && !OperatingSystem.IsWindows()
                ? L10n.T("Panels in their own window needs python3 with PyGObject, GTK 3 and WebKitGTK (webkit2gtk-4.1) installed; the panels are back in the game.")
                : L10n.F("The panels' window closed unexpectedly (code {0}); they are back in the game. {1} says why.", code, FrameLogHint));
        }
    }

    private static int SafeExitCode(Process p)
    {
        try
        {
            return p.ExitCode;
        }
        catch (InvalidOperationException)
        {
            return -1;
        }
    }

    /// <summary>A loopback port free right now (the frame binds it a moment
    /// later; a lost race fails its start, logged, and the panels stay in).</summary>
    private static int FreeLoopbackPort()
    {
        var l = new TcpListener(IPAddress.Loopback, 0);
        l.Start();
        try
        {
            return ((IPEndPoint)l.LocalEndpoint).Port;
        }
        finally
        {
            l.Stop();
        }
    }

    private static void Guard(Action action)
    {
        try
        {
            action();
        }
        catch (Exception ex)
        {
            Logger.Log($"[SimFrameLauncher] ignoring error (never breaks gameplay): {ex.Message}");
        }
    }
}
