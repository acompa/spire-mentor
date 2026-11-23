// LinuxCrashGuard.cs -- keep a null dereference an exception on Linux (GH #1425).
//
// .NET turns a null dereference into a NullReferenceException in its SIGSEGV
// handler (and an integer divide by zero in its SIGFPE handler). On Linux the
// game closes its Sentry crash reporter for a modded session about half a
// second after the mods load (SentryService.AfterGameInit -> Shutdown), and
// that close sets the crash signals back to the default action and disables
// the main thread's alternate signal stack. From then on a null dereference
// anywhere in managed code, the game's or a mod's, in a try/catch or not,
// kills the process. Windows uses SEH and is not affected.
//
// The fix: while the reporter is still up, find the actions the runtime had
// installed (the reporter keeps them, to chain to), and put them back right
// after the shutdown, with a new signal stack for the main thread. Sentry
// stays off, as the game wants for a modded session. Everything here is a
// no-op when anything looks different from what was tested: another OS or
// CPU, no saved action found, or a handler that is not the default.
//
// Self-test: SPIRE_MENTOR_NULL_SELFTEST=1 dereferences null (main thread and
// a worker) and divides by zero eight seconds after startup and logs what was
// caught. Without the guard that run dies, which is the point of the test.
// =native then also crashes in native code, which must still kill the game
// at once, with nothing reporting it.

using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Threading.Tasks;
using Godot;
using HarmonyLib;

namespace SpireMentor;

internal static class LinuxCrashGuard
{
    private const int SsDisable = 2;
    private const int SignalStackBytes = 256 * 1024;

    private static LinuxSignalRules.Picked? _saved;
    private static bool _reported;

    [DllImport("libc.so.6", EntryPoint = "sigaction")]
    private static extern int ReadAction(int signal, IntPtr action, byte[] old);

    [DllImport("libc.so.6", EntryPoint = "sigaction")]
    private static extern int WriteAction(int signal, byte[] action, IntPtr old);

    [DllImport("libc.so.6", EntryPoint = "sigaltstack")]
    private static extern int ReadSignalStack(IntPtr stack, byte[] old);

    [DllImport("libc.so.6", EntryPoint = "sigaltstack")]
    private static extern int WriteSignalStack(byte[] stack, IntPtr old);

    [DllImport("libc.so.6")]
    private static extern nint strlen(IntPtr text);

    public static void Install(Harmony patcher)
    {
        if (!OperatingSystem.IsLinux() || RuntimeInformation.ProcessArchitecture != Architecture.X64)
        {
            return;
        }
        try
        {
            _saved = FindRuntimeActions();
            if (_saved == null)
            {
                Logger.Log("[LinuxCrashGuard] the runtime's signal actions were not found; " +
                           "a null dereference will crash the game once Sentry shuts down");
                return;
            }
            Type? sentry = AccessTools.TypeByName("MegaCrit.Sts2.Core.Debug.SentryService");
            var shutdown = sentry == null ? null : AccessTools.Method(sentry, "Shutdown");
            if (shutdown != null)
            {
                patcher.Patch(shutdown, postfix: new HarmonyMethod(typeof(LinuxCrashGuard), nameof(AfterSentryShutdown)));
            }
            // Backstop on the main thread, should the shutdown reach the
            // native close by another road, or run off the main thread.
            if (Engine.GetMainLoop() is SceneTree tree)
            {
                foreach (double seconds in new[] { 1.0, 5.0, 20.0 })
                {
                    tree.CreateTimer(seconds).Timeout += Restore;
                }
                string? selfTest = System.Environment.GetEnvironmentVariable("SPIRE_MENTOR_NULL_SELFTEST");
                if (selfTest == "1" || selfTest == "native")
                {
                    tree.CreateTimer(8.0).Timeout += SelfTest;
                }
                if (selfTest == "native")
                {
                    tree.CreateTimer(10.0).Timeout += () =>
                    {
                        Logger.Log("[LinuxCrashGuard] self-test: a crash in native code (the game must die here)");
                        strlen(IntPtr.Zero);
                        Logger.Log("[LinuxCrashGuard] self-test: still alive after a native crash");
                    };
                }
            }
            Logger.Log($"[LinuxCrashGuard] ready (SIGSEGV{(_saved.Fpe != null ? ", SIGFPE" : "")})");
        }
        catch (Exception ex)
        {
            Logger.Log($"[LinuxCrashGuard] off: {ex.GetType().Name}: {ex.Message}");
        }
    }

    public static void AfterSentryShutdown() => Restore();

    private static void Restore()
    {
        try
        {
            LinuxSignalRules.Picked? saved = _saved;
            if (saved == null)
            {
                return;
            }
            bool segv = PutBack(LinuxSignalRules.Sigsegv, saved.Segv);
            bool fpe = saved.Fpe != null && PutBack(LinuxSignalRules.Sigfpe, saved.Fpe);
            bool stack = HandlerOf(LinuxSignalRules.Sigsegv) == BitConverter.ToUInt64(saved.Segv, 0) && EnsureSignalStack();
            if ((segv || fpe || stack) && !_reported)
            {
                _reported = true;
                Logger.Log("[LinuxCrashGuard] Sentry's shutdown removed the runtime's crash handlers; put back" +
                           $"{(segv ? " SIGSEGV" : "")}{(fpe ? " SIGFPE" : "")}{(stack ? " and a signal stack for this thread" : "")}");
            }
        }
        catch (Exception ex)
        {
            Logger.Log($"[LinuxCrashGuard] restore failed: {ex.GetType().Name}: {ex.Message}");
        }
    }

    private static ulong HandlerOf(int signal)
    {
        var action = new byte[LinuxSignalRules.ActionSize + 8];
        return ReadAction(signal, IntPtr.Zero, action) == 0 ? BitConverter.ToUInt64(action, 0) : ulong.MaxValue;
    }

    /// <summary>Only over the default action: never over someone's handler.</summary>
    private static bool PutBack(int signal, byte[] action) =>
        HandlerOf(signal) == 0 && WriteAction(signal, action, IntPtr.Zero) == 0;

    /// <summary>
    /// The runtime's SIGSEGV handler aborts on a thread with no alternate
    /// signal stack, and the main thread's was Sentry's. Per thread; the
    /// memory is never freed.
    /// </summary>
    private static bool EnsureSignalStack()
    {
        var current = new byte[24];
        if (ReadSignalStack(IntPtr.Zero, current) != 0 || (BitConverter.ToInt32(current, 8) & SsDisable) == 0)
        {
            return false;
        }
        IntPtr memory = Marshal.AllocHGlobal(SignalStackBytes);
        var stack = new byte[24];
        BitConverter.GetBytes((ulong)(long)memory).CopyTo(stack, 0);
        BitConverter.GetBytes((ulong)SignalStackBytes).CopyTo(stack, 16);
        if (WriteSignalStack(stack, IntPtr.Zero) != 0)
        {
            Marshal.FreeHGlobal(memory);
            return false;
        }
        return true;
    }

    /// <summary>
    /// Reads this process's own memory through /proc/self/mem, which fails
    /// with an error where a direct read of a page unmapped meanwhile would
    /// fault. The heap first: that is where the reporter keeps them.
    /// </summary>
    private static LinuxSignalRules.Picked? FindRuntimeActions()
    {
        LinuxSignalRules.Maps maps = LinuxSignalRules.ParseMaps(File.ReadLines("/proc/self/maps"));
        if (maps.Runtime.Count == 0 || maps.Libc.Count == 0)
        {
            return null;
        }
        using var memory = new FileStream("/proc/self/mem", System.IO.FileMode.Open, System.IO.FileAccess.Read,
            FileShare.ReadWrite, bufferSize: 1);
        var found = new List<LinuxSignalRules.Candidate>();
        Scan(memory, maps, maps.Heap, found);
        LinuxSignalRules.Picked? picked = LinuxSignalRules.Pick(found);
        if (picked == null && found.Count == 0)
        {
            Scan(memory, maps, maps.Anonymous, found);
            picked = LinuxSignalRules.Pick(found);
        }
        return picked;
    }

    private static void Scan(FileStream memory, LinuxSignalRules.Maps maps, List<LinuxSignalRules.Range> ranges,
        List<LinuxSignalRules.Candidate> found)
    {
        var block = new byte[1 << 20];
        // Blocks overlap by one action, so one that straddles a boundary is
        // still seen whole (and only once: it does not fit the earlier block).
        int step = block.Length - LinuxSignalRules.ActionSize + 8;
        foreach (LinuxSignalRules.Range range in ranges)
        {
            for (ulong at = range.Lo; at < range.Hi; at += (ulong)step)
            {
                int want = (int)Math.Min((ulong)block.Length, range.Hi - at);
                int got = 0;
                try
                {
                    memory.Seek((long)at, SeekOrigin.Begin);
                    while (got < want)
                    {
                        int n = memory.Read(block, got, want - got);
                        if (n <= 0)
                        {
                            break;
                        }
                        got += n;
                    }
                }
                catch (IOException)
                {
                    // unmapped or unreadable: whatever was read still counts
                }
                LinuxSignalRules.FindCandidates(block.AsSpan(0, got), at, maps, found);
            }
        }
    }

    private sealed class Holder
    {
        public int Field = 1;
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static Holder? Nothing() => System.Environment.TickCount64 == long.MinValue ? new Holder() : null;

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static int Zero() => System.Environment.TickCount64 == long.MinValue ? 1 : 0;

    private static void SelfTest()
    {
        Logger.Log("[LinuxCrashGuard] self-test: null dereference on the main thread");
        try { Logger.Log($"[LinuxCrashGuard] self-test: no exception ({Nothing()!.Field})"); }
        catch (NullReferenceException) { Logger.Log("[LinuxCrashGuard] self-test: main thread caught it"); }
        Logger.Log("[LinuxCrashGuard] self-test: null dereference on a worker thread");
        Task.Run(() =>
        {
            try { return Nothing()!.Field.ToString(); }
            catch (NullReferenceException) { return "worker thread caught it"; }
        }).ContinueWith(t => GD.Print("[SpireMentor] [LinuxCrashGuard] self-test: " + t.Result)).Wait();
        Logger.Log("[LinuxCrashGuard] self-test: integer divide by zero");
        try { Logger.Log($"[LinuxCrashGuard] self-test: no exception ({10 / Zero()})"); }
        catch (DivideByZeroException) { Logger.Log("[LinuxCrashGuard] self-test: divide by zero caught"); }
        Logger.Log("[LinuxCrashGuard] self-test: done");
    }
}
