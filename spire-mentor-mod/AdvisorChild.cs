// AdvisorChild.cs -- the advisor process the mod starts, on any OS.
//
// Windows starts it through WindowsChildProcess (no inherited handles to
// the game's open save files; SimJobObject kills it with the game). Linux
// and macOS start it with a plain Process: .NET opens its files
// close-on-exec there, and the advisor's --exit-with-parent ends it when
// the game is gone (GH #1424/#1425).

using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

namespace SpireMentor;

/// <summary>A started advisor: its process and its output streams.</summary>
internal interface IAdvisorChild : IDisposable
{
    Process Process { get; }
    StreamReader StandardOutput { get; }
    StreamReader StandardError { get; }
}

internal static class AdvisorChild
{
    /// <summary>
    /// The packaged advisor binary for this OS and architecture, in
    /// <c>SimAdvisor/</c> next to the DLL, where every platform's binary
    /// shares one copy of the nets (<c>stage_advisor.py</c> names them).
    /// </summary>
    public static string BinaryName(bool windows, bool macos, Architecture arch)
    {
        if (windows)
        {
            return "SimAdvisor.exe";
        }
        string os = macos ? "macos" : "linux";
        string cpu = arch switch
        {
            Architecture.X64 => "x64",
            Architecture.Arm64 => "arm64",
            _ => arch.ToString().ToLowerInvariant(),
        };
        return $"SimAdvisor-{os}-{cpu}";
    }

    /// <summary>Whether the coach may switch itself on when the game starts:
    /// only where an advisor can start, else the toolbar would just read
    /// "crashed". Windows always ships one; elsewhere it takes the packaged
    /// binary for this platform or a settings-page command.</summary>
    public static bool CanStartAtLaunch(bool windows, bool commandConfigured, bool bundledBinaryExists) =>
        windows || commandConfigured || bundledBinaryExists;

    /// <summary>Start <paramref name="psi"/>'s command with its output
    /// redirected. <paramref name="beforeResume"/> runs on Windows only,
    /// before the suspended process starts (job-object adoption).</summary>
    public static IAdvisorChild Start(ProcessStartInfo psi, Action<Process>? beforeResume = null) =>
        OperatingSystem.IsWindows()
            ? WindowsChildProcess.Start(psi.FileName, psi.Arguments, psi.WorkingDirectory, beforeResume)
            : UnixChildProcess.Start(psi);
}

/// <summary>The Linux/macOS advisor: a plain <see cref="Process"/>. The native
/// advisor writes UTF-8 (and escapes non-ASCII on its console anyway).</summary>
internal sealed class UnixChildProcess : IAdvisorChild
{
    public Process Process { get; }
    public StreamReader StandardOutput => Process.StandardOutput;
    public StreamReader StandardError => Process.StandardError;

    private UnixChildProcess(Process process) => Process = process;

    public static UnixChildProcess Start(ProcessStartInfo psi)
    {
        // Configured in place: a copy would drop psi.ArgumentList (the
        // /bin/sh -c launch of a settings-page command uses it).
        psi.UseShellExecute = false;
        psi.CreateNoWindow = true;
        psi.RedirectStandardOutput = true;
        psi.RedirectStandardError = true;
        psi.StandardOutputEncoding = Encoding.UTF8;
        psi.StandardErrorEncoding = Encoding.UTF8;
        Process process = Process.Start(psi)
            ?? throw new InvalidOperationException($"could not start {psi.FileName}");
        return new UnixChildProcess(process);
    }

    public void Dispose() => Process.Dispose();
}
