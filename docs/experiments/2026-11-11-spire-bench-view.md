# Spire Bench M1: the player-visible view

Date: 2026-11-11. Plan: `docs/plans/2026-11-11-spire-bench.md`.
Protocol draft: `spire-bench/PROTOCOL.md`.
Data: `docs/experiments/2026-11-11-spire-bench-view/`.

## Question

Does the referee host send a client only what the game screen shows, and does
a field nobody classified stay out by default?

## What was built

- `sts2-pycli/src/Sts2Headless/RefereeView.cs`: every referee reply is
  projected onto an allow-list. A deny-list names the known hidden state (RNG
  block, enemy AI internals and move ids, draw-pile positions). A field on
  neither list is dropped and reported through `referee_stats`.
- The draw pile is sent sorted by content, so it does not depend on the order.
- Map decisions carry the whole act map, as the map screen does.
- A potion reward on a full belt is always a decision in referee mode.
- Four more tests in `tests/test_referee_rng.py` (16 in total).

The field inventory came from 60 default-host runs (about 1,800 distinct
paths over 9 decision types); the lists were written from that.

## Results

| Check | Result |
|---|---|
| Hidden state in a real combat payload (RNG block, AI state, move id, draw positions) | removed (pytest, on a payload captured from the default host) |
| View identical when the draw pile is reversed | yes (pytest) |
| Unknown field | dropped and reported (pytest) |
| Unclassified fields over 660 referee runs (A0 and A10, 5 characters) | none |
| Escaped draws / errors / world errors in those runs | 0 / 0 / 0 |
| Exact replays, and other-key runs that differ | 30 / 30 and 30 / 30 |
| Per action with the view | about 18 ms (16 to 18 ms without it, same loaded box) |

The first sweep after the lists were written did find a field: card
`enchantment` and `enchantment_amount` were dropped and reported until they
were added. That is the intended failure direction.

## What this does not show

- **Depth, again.** The trivial policy dies in act 1 (median floor 7 to 8,
  best act 2 floor 7). Fields that only appear later, in shops that are
  actually used, or in rarer events have not been seen. They would be dropped
  and reported, not leaked, but a client would be missing them.
- **Semantic leaks inside allowed fields.** The lists judge field names. A
  public-looking field whose value encodes hidden state (an intent shown
  despite an intent-hiding effect, an event option var that resolves a random
  outcome early) would pass. The six open questions in `PROTOCOL.md` list the
  known candidates.
- **Legal-action lists.** Legality is still derived from the observation by
  the client; nothing enumerates it.
