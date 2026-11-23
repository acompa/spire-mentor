// LinuxSignalRules.cs -- game-free half of LinuxCrashGuard: read /proc/self/maps,
// recognise the runtime's saved signal actions in a block of memory, and pick
// the ones to put back. Unit-tested in tests/unit.

using System;
using System.Collections.Generic;

namespace SpireMentor;

internal static class LinuxSignalRules
{
    // glibc's struct sigaction on x86-64: handler, a 128-byte mask, flags,
    // restorer.
    public const int ActionSize = 152;
    public const int FlagsOffset = 136;
    public const int RestorerOffset = 144;
    public const uint SaSiginfo = 0x00000004;
    public const uint SaRestorer = 0x04000000;
    public const uint SaOnStack = 0x08000000;
    public const int Sigfpe = 8;
    public const int Sigsegv = 11;

    public readonly record struct Range(ulong Lo, ulong Hi)
    {
        public bool Holds(ulong address) => address >= Lo && address < Hi;
    }

    public sealed class Maps
    {
        /// <summary>Executable mappings of libcoreclr.so.</summary>
        public readonly List<Range> Runtime = new();
        /// <summary>Executable mappings of libc.</summary>
        public readonly List<Range> Libc = new();
        /// <summary>The brk heap, where the saved actions were found.</summary>
        public readonly List<Range> Heap = new();
        /// <summary>Anonymous private read-write mappings: the fallback.</summary>
        public readonly List<Range> Anonymous = new();
    }

    /// <summary>One saved action whose handler is in the runtime.</summary>
    public sealed record Candidate(ulong Address, ulong Handler, uint Flags, byte[] Bytes);

    public sealed record Picked(byte[] Segv, byte[]? Fpe);

    public const ulong LargestScannedMapping = 256UL << 20;

    public static Maps ParseMaps(IEnumerable<string> lines)
    {
        var maps = new Maps();
        foreach (string line in lines)
        {
            string[] f = line.Split(' ', 6, StringSplitOptions.RemoveEmptyEntries);
            if (f.Length < 5)
            {
                continue;
            }
            string[] r = f[0].Split('-');
            if (r.Length != 2 || !TryHex(r[0], out ulong lo) || !TryHex(r[1], out ulong hi) || hi <= lo)
            {
                continue;
            }
            string perms = f[1];
            string name = f.Length > 5 ? f[5].Trim() : "";
            string file = name.Substring(name.LastIndexOf('/') + 1);
            var range = new Range(lo, hi);
            if (perms.Contains('x'))
            {
                if (file == "libcoreclr.so")
                {
                    maps.Runtime.Add(range);
                }
                else if (file.StartsWith("libc.so", StringComparison.Ordinal) ||
                         file.StartsWith("libc-", StringComparison.Ordinal))
                {
                    maps.Libc.Add(range);
                }
                continue;
            }
            if (!perms.StartsWith("rw", StringComparison.Ordinal) || !perms.EndsWith('p') ||
                hi - lo > LargestScannedMapping)
            {
                continue;
            }
            if (name == "[heap]")
            {
                maps.Heap.Add(range);
            }
            else if (name.Length == 0)
            {
                maps.Anonymous.Add(range);
            }
        }
        return maps;
    }

    private static bool TryHex(string s, out ulong value) =>
        ulong.TryParse(s, System.Globalization.NumberStyles.HexNumber,
            System.Globalization.CultureInfo.InvariantCulture, out value);

    private static bool In(List<Range> ranges, ulong address)
    {
        foreach (Range r in ranges)
        {
            if (r.Holds(address))
            {
                return true;
            }
        }
        return false;
    }

    /// <summary>
    /// Every 8-aligned position in <paramref name="block"/> that reads as a
    /// sigaction the runtime installed: handler in libcoreclr's code, restorer
    /// in libc's, SA_SIGINFO and SA_RESTORER set.
    /// </summary>
    public static void FindCandidates(ReadOnlySpan<byte> block, ulong blockAddress, Maps maps, List<Candidate> into)
    {
        for (int o = 0; o + ActionSize <= block.Length; o += 8)
        {
            ulong handler = BitConverter.ToUInt64(block.Slice(o, 8));
            if (!In(maps.Runtime, handler))
            {
                continue;
            }
            uint flags = BitConverter.ToUInt32(block.Slice(o + FlagsOffset, 4));
            if ((flags & (SaSiginfo | SaRestorer)) != (SaSiginfo | SaRestorer))
            {
                continue;
            }
            if (!In(maps.Libc, BitConverter.ToUInt64(block.Slice(o + RestorerOffset, 8))))
            {
                continue;
            }
            into.Add(new Candidate(blockAddress + (ulong)o, handler, flags, block.Slice(o, ActionSize).ToArray()));
        }
    }

    /// <summary>
    /// The SIGSEGV action, and SIGFPE's when the layout proves which one it
    /// is. SIGSEGV is the only action the runtime installs with SA_ONSTACK.
    /// The crash reporter keeps the old actions in an array indexed by signal
    /// number, so SIGFPE (8) sits three slots before SIGSEGV (11), the two
    /// slots between (SIGKILL, SIGUSR1) hold nothing of the runtime's and the
    /// five before it (3..7) all do. Null
    /// when the copies found disagree: then nothing is put back.
    /// </summary>
    public static Picked? Pick(IReadOnlyList<Candidate> candidates)
    {
        var byAddress = new Dictionary<ulong, Candidate>();
        foreach (Candidate c in candidates)
        {
            byAddress[c.Address] = c;
        }
        Candidate? segv = null;
        Candidate? fpe = null;
        bool fpeAgreed = true;
        foreach (Candidate c in candidates)
        {
            if ((c.Flags & SaOnStack) == 0)
            {
                continue;
            }
            if (segv != null && segv.Handler != c.Handler)
            {
                return null;
            }
            segv ??= c;
            const ulong slot = ActionSize;
            bool gap = !byAddress.ContainsKey(c.Address - slot) && !byAddress.ContainsKey(c.Address - 2 * slot);
            // SIGQUIT..SIGBUS (3..7) are the runtime's too: all five must be
            // there before the slot at 8 is taken for SIGFPE.
            for (ulong k = 4; gap && k <= 8; k++)
            {
                gap = byAddress.ContainsKey(c.Address - k * slot);
            }
            if (gap && byAddress.TryGetValue(c.Address - 3 * slot, out Candidate? f) &&
                (f.Flags & SaOnStack) == 0 && f.Handler != c.Handler)
            {
                if (fpe != null && fpe.Handler != f.Handler)
                {
                    fpeAgreed = false;
                }
                fpe ??= f;
            }
        }
        return segv == null ? null : new Picked(segv.Bytes, fpeAgreed ? fpe?.Bytes : null);
    }
}
