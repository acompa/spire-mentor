# Spire Bench M0: keyed RNG on the real game DLL (feasibility)

Date: 2026-11-11. Plan: `docs/plans/2026-11-11-spire-bench.md`.
Data: `docs/experiments/2026-11-11-spire-bench-feasibility/`.

## Question

Can the headless host (`sts2-pycli/src/Sts2Headless`, real `sts2.dll` v0.107.1)
serve as a benchmark referee whose randomness a client cannot predict, and is
it fast and small enough to host?

## What was built

- `RefereeRng.cs`: with `STS2_REFEREE=1`, every `Rng` draw is
  `HMAC-SHA256(streamKey, counter)`, `streamKey = HMAC-SHA256(episodeKey, stream seed)`.
  The episode key arrives in `start_run.rng_key`. The range mapping copies
  `MegaRandom`'s.
- Escape audit after every command: a patched stream's game generator must not
  have moved. A violation ends the process with `referee_fault`.
- Referee mode accepts only `start_run`, `action`, `quit` (plus two
  diagnostics); everything that reads or sets hidden state is refused.
- `python/referee_spike.py` (driver) and `tests/test_referee_rng.py` (12 tests).

## Setup

Trivial policy from `play_full_run.py` (first playable card, first option,
random map node). 8 worker processes on the 48-core dev box while it was
already loaded (load average 25 to 43, a training lane was running), so the
timings are pessimistic. Not a paired comparison.

| File | Mode | Ascension | Runs |
|---|---|---|---|
| `referee_a10.jsonl` | keyed | 10 | 500 (100 per character) + 25 replays + 25 other-key |
| `referee_a0.jsonl` | keyed | 0 | 200 (40 per character) + 10 replays + 10 other-key |
| `vanilla_a10.jsonl` | game RNG | 10 | 100 (20 per character) |

## Results

| Check | Result |
|---|---|
| Escaped draws (`referee_fault`) | 0 in 770 keyed runs, about 76,000 actions |
| Audit negative control (one method left unpatched) | faults, as it must (pytest) |
| Same key + same actions reproduce the game | 35 / 35 identical digests |
| Different key, same public seed label | 35 / 35 differ |
| World errors on stderr | 0 runs in 870 |
| Error replies / stuck runs | 0 |
| Keyed draws uniform (chi-square, p = 0.001, 100k samples) | pass (pytest) |
| Peak memory per process | median 144 MB, max 157 MB |
| Process boot | 0.5 s; first `start_run` about 2.5 s (content registration) |
| Per action, keyed | about 16 ms (A10), 18 ms (A0) |
| Per action, game RNG | about 15 ms |

Per-action cost comes from a least-squares fit of play time on action count
across runs; the intercept is the start-up cost.

## What this does not show

- **Depth.** The trivial policy dies in act 1: median floor 6 at A10, 8.5 at
  A0; 3 of 200 A0 runs reached act 2, none act 3, no wins. Acts 2 and 3, their
  events and bosses are not covered for escapes, world errors or timing. A
  competent agent has to play through the referee to close this (milestone 4).
- **Full-length run cost.** Not measured. If a full run is about 1,500 actions
  and the per-action cost holds, one run is about 27 s per worker and 1,000
  runs take about half an hour on 16 workers. Both assumptions are unverified.
- **Hidden-information leaks.** The host still returns the ordered draw pile
  and RNG counters in its state. The player-visible view is milestone 1.
- **v0.111.0.** ~~Referee mode refuses to start there (different `Rng` shape).~~
  Ported (GH #1327, 2026-11-13); see below.

## Port to v0.111.0 and v0.113.0 (GH #1327)

v0.111.0 dropped `Rng.Seed` and `Rng.Counter`, seeds streams with 64-bit
hashes, removed `FastForwardCounter` (saves restore the full Xoshiro state)
and added `NextUnsignedLong`. v0.113.0 moved `Shuffle` to
`ListExtensions.UnstableShuffle`, which seeds a child `Rng` from
`NextUnsignedLong`. The port keys each stream by the seed passed to
`Rng(ulong)`, reads the private `_counter`, patches both `NextUnsignedLong`
overloads, and gives an `Rng` built from a save no label, so its first draw
faults (every save-load path in the game is a menu or lobby screen, and the
referee refuses `load_save`).

Same spike, random policy, 20 runs per character for all five, v0.111.0:

| | runs | escapes / faults | world errors | replays identical | other key differs |
|---|---|---|---|---|---|
| A10 | 100 | 0 | 0 | 15/15 | 15/15 |
| A0 | 100 | 0 | 0 | 10/10 | 10/10 |

The depth gap above is unchanged: no run left act 1. The referee tests
(`tests/test_referee_rng.py`, 37) pass on all three builds, including the
escape-audit negative control and the uniformity checks through the game's
own `Rng` API. Data: `referee_v0111_a10.jsonl`, `referee_v0111_a0.jsonl`.

## Verdict

Go for milestone 1 on what was measured: no draw escaped, replay is exact, and
memory and speed are small enough to host. The depth gap is the main risk
carried forward.
