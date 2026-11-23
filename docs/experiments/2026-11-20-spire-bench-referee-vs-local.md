# Spire Bench: our search agent through the referee vs. with the full state

2026-11-20, #1636 (M4). Data and scripts: `2026-11-20-spire-bench-referee-vs-local/`.

## Question

Does an honest view of the game cost a search bot win rate? The referee shows
only what a player sees: telegraphed intents (no enemy move state), the draw
pile sorted (no order), no seed or RNG cursors. The bot's own engine needs
the hidden parts; it has to infer them.

## Setup

One agent, two arms. The agent is `headless_eval.play_one_headless_session`:
PPO run champion for the non-combat decisions, MCTS (`beliefdraw`, c_puct 2.5,
draw widening 2.0) for fights, the persistent combat mirror under OBSERVE.

| | local | referee |
|---|---|---|
| world | plain headless host (real `sts2.dll`, v0.111.0) | same host in referee mode, behind the gateway |
| agent sees | full state: RNG, enemy move state, draw order | the referee observation only |
| hidden state | read | inferred: enemy moves (`bench_agents.move_infer`), act map, game version |
| driver | `search_agent.py --local` | `search_agent.py` (`RefereeGame`) |

Ironclad A10, MCTS 1000 sims (4000 at bosses), 150 episodes per arm (10
processes × 15). Local seeds 20000–20914; referee episodes keyed by a dev
season key. The arms are **unpaired**: the referee keys its own dice, so no
episode exists in both.

Run at `e6b2fc74f` (branch `feat/spire-bench-m4`), after the #1639 fix. A
first attempt played A10 with the agent at A0 (both arms) and was discarded.

## Result

| arm | wins | win rate | Wilson 95% | errors | losses act 1 / 2 / 3 | mean floor | mirror desyncs / reconciles |
|---|---|---|---|---|---|---|---|
| local | 13/150 | 8.7% | 5.1–14.3% | 0 | 21 / 40 / 76 | 38.7 | 2,092 / 55,162 (3.8%) |
| referee | 12/150 | 8.0% | 4.6–13.5% | 0 | 18 / 40 / 80 | 38.2 | 14,490 / 53,792 (26.9%) |

Referee − local = −0.7 pp (z −0.21, two-sided p 0.84). At this n the 95%
interval on the difference is about ±6 pp: the honest view costs this agent
no more than a few points, and possibly nothing; a small cost cannot be ruled
out.

The referee arm's mirror disagrees with the world at 27% of its board checks
(local 4%), as expected: it cannot know the draw order or the referee's dice,
so OBSERVE re-seeds the fight from each observation. That did not show in the
win rate.

Median wall time per episode: 56 s local, 62 s referee.

## What the run found

- **Transcripts did not all replay.** `spire_bench.verify` over the 150
  referee transcripts: 136 verified (each receipt equal to the replayed
  fingerprint), 14 failed, all at a `select_map_node` into a Trial event. The
  game draws Trial's entrant number from `Rng.Chaotic`, which the referee
  had keyed like a gameplay stream, labelled by its own wall-clock seed and
  counted, while VFX timers draw from it too. Fixed in `60794e035`: Chaotic
  (cosmetics only in the game) is now a fixed function of the episode key.
  Re-check below. The win rates are unaffected: the number is display text.
- **#1639**: `headless_eval` played an A>0 run with A0 search boards and an A0
  PPO envelope. Fixed (`e6b2fc74f`).
- **#1640**: the combat mirror predicts a Taunt at cost 0 where the game
  shows 1 (headless seed 1000, A0). Open.
- The adapter needs `index` on draw-pile cards; the referee omits it (it
  would reveal order), so before the fix the search planned every fight with
  an empty draw pile. `RefereeGame` now numbers them in the published sorted
  order.

## Open

- **The absolute level is low.** About 8% IC A10 for both arms, where this
  agent family measured 20–28% IC A10 in `run_arena` (Rust engine) at
  comparable budgets. Both arms share it, so it lies in the headless rung
  (the DLL world, the mirror under OBSERVE, or the v0.111.0 champion), not
  in the referee. Not investigated.
- A paired design is impossible against the referee by construction. A
  tighter bound on the information cost needs about 4× the episodes.

## Determinism re-check after the fix

50 new referee episodes on `60794e035` (Ironclad A10, 50 sims; `recheck/`):
all 50 transcripts verify and every receipt equals its replayed fingerprint.
Only one of them reached a Trial event, so this run alone is thin evidence
for the fix. The direct evidence is
`test_referee_rng.py::test_the_cosmetic_generator_depends_on_the_key_alone`:
two hosts started in different seconds, one after extra draws, draw the same
values. It fails on the old keying (`[741, 775, 599] != [380, 966, 572]`).
