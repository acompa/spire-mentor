# Stop 1: State, action, episode, return

## Question

What does the agent see at one decision, what can it choose, and where does an
episode begin and end?

Every later stop hangs off the answer. Reward (stop 3) is paid along a
sequence of decisions. Value and advantage (stop 4) score one decision of that
sequence. So first pin down what one decision is and what one sequence is.

### Four terms, by example

A **run** is one game of Slay the Spire 2 with one character, here the
Ironclad. It starts at Neow, an opening event that offers a few gifts. It goes
through about fifty floors of fights, shops, rest sites and events across three
acts. It ends when the character dies or beats the last boss.

A **step** is one action applied to the game: play a card, end the turn, pick
a path on the map, take a card from a reward. Each step moves the game to a new
position.

A **decision** is a step where the agent must choose. Here is one, from the
first fight of a run. The Ironclad holds three Strikes, a Defend and a curse
card. It also has two potions. The game offers seven legal actions: Strike
(three copies), Defend, either potion, or end the turn. The three Strike plays
do exactly the same thing, so the env merges them into one option. Each
distinct option is called a **class**. This decision has five classes. The
agent picks one class, and the env plays that class's first member.

A **forced step** is a step with only one class. That happens when there is
one legal action, or when every legal action merges into one class. The env
applies it without asking the agent and stores nothing for it. A forced step is
part of the run, but it is not a decision.

An **episode** is the sequence of decisions from one run's first decision to
its end. Fights and the choices between fights are all decisions of the same
policy, in the same sequence. There is no separate "fight agent".

## Read

About 75 minutes. Paths are relative to the repository root. Read them in
this order.

1. `docs/plans/2026-11-02-ironclad-e2e-ppo-v0111.md`, line 480 (find
   "Decision:" in Appendix A), then lines 183-191 (§3.2 Decisions). Look for
   the one-line definition of a decision. Then see the plan's rules on
   auto-advance (forced steps), dedupe (merging) and caps. *10 min.*
2. `sts2-pycli/python/ppo/e2e/env.py`, lines 8-21 and 41-60 (module
   docstring). Look for the loop `reset`, then `step` until a terminal. Note
   which steps get no row. Then read how reward is paid per row (lines 41-48)
   and what a cap does to a run (lines 55-60). *10 min.*
3. `sts2-pycli/python/ppo/e2e/env.py`, lines 726-760 (`E2EDecision`) and
   981-1003 (`E2ETerminal`). Look for the fields one decision carries: `dtype`,
   `members`, `classes`, `actions`, `floor`. Then see the outcomes a run can
   end with. *10 min.*
4. `sts2-pycli/python/ppo/e2e/env.py`, lines 1175-1240 (`reset`, `step`,
   `step_action`, `_drive`). Look for how `step(choice)` turns a class index
   into one concrete action, and how `_drive` loops past forced steps to the
   next decision or the end. *10 min.*
5. `sts2-pycli/python/ppo/e2e/env.py`, lines 916-980 (`PolicySource`,
   `PrivSource`). Look for what the policy reads (the *belief view* of a fight)
   and what only the value path may read (the true state). This is the line
   between what the game knows and what the agent sees. *10 min.*
6. `sts2-rs/crates/sts2-e2e/src/env.rs`, the Rust port of the same env, which
   the check drives:
   - lines 948-985 (`Decision`) and 1106-1128 (`Forced`, `Next`). A position
     is one of three things: a forced step, a decision, or the end.
   - lines 1435-1487 (`peek_inner`, out of fights). Look for the two ways a
     step becomes forced: a lone action (1476-1481) and a lone class
     (`ooc`, 1506-1516).
   - lines 1567-1609 (`combat`). Look for the same two forced cases inside a
     fight. Note that classes come from `belief_view`, not from the true state.
   - lines 1861-1929 (`reset`, `step`, `run_to_decision`). This is the whole
     episode loop in about thirty lines. *15 min.*
7. `sts2-rs/crates/sts2-py/src/e2e.rs`, lines 196-288 (`E2ePeek`), 290-384
   (`E2eDriver`) and 401-455 (`e2e_selfplay`). This is the Python surface the
   check calls: `peek`, `class_action`, `apply`, and `decide` (lines 122-168).
   *5 min.*
8. `docs/plans/2026-11-02-ironclad-e2e-ppo-v0111.md`, lines 224-243 (§4
   Observation), with `sts2-pycli/python/ppo/e2e/contracts.py` lines 283-332
   (the section table). Look for what an observation is: a set of tokens, each
   tagged with a section such as HAND, ENEMY or OPT_CARD. There is no screen
   image. Note which sections are PRIV (privileged, value only). *10 min.*

`rsenv.py` lines 1-29 explain why two envs exist (Python for clarity, Rust for
speed) and how they are held to the same rows. Skim it if the two copies
confuse you.

## Predict

Fill in `predictions.toml`: a guess and an 80% interval (low, high) for each.
"The panel" is the first 200 seeds of `PANEL_A`. "Random play" picks
uniformly among the offered classes at every decision. "The network" is the
published Ironclad net `ironclad_e2e_d9000_ema0999`, playing greedily (always
its most probable class).

| Key | Quantity | Unit |
| --- | --- | --- |
| `net_decisions_per_run` | Mean decisions in one of the network's runs. | decisions |
| `random_decisions_per_run` | Mean decisions in one random-play run. | decisions |
| `net_fight_share` | Share of the network's decisions that are inside fights (pooled over the panel). | fraction, 0 to 1 |
| `net_median_classes` | Median number of classes offered at one of the network's decisions. | classes |
| `random_mean_floor` | Mean floor a random-play run reaches before it ends. Floor 0 is before the first fight; the last boss is on floor 46 (`env.py` lines 50-51). | floor |
| `net_forced_share` | Of all the network's steps (decisions plus forced steps), the share that were forced. | fraction, 0 to 1 |

## Run

From the repository root (in a worktree, set `TOUR_VENV` and `TOUR_WEIGHTS`
as `tour/README.md` shows):

```bash
tour/run stop-1-episode/check.py
```

It takes about 5 minutes on ten threads. `--quick` plays 20 seeds instead of
200 (under a minute). `--workers N` sets the threads for the network's runs.

What it does:

1. Plays the first seed of the panel twice, once at random and once with the
   network. It prints the first 14 decisions of each, with the forced steps
   between them.
2. Shows one fight decision and one card-reward decision of the network's
   run. Each is shown twice: as a person would see it, then as the policy
   reads it (the token count per section).
3. Plays the whole panel with both players and prints, for each: decisions
   per run, the share inside fights, the share of steps that were forced,
   classes offered per decision, decisions by type, floor reached, and how
   runs ended.
4. Scores your predictions.

How it plays. The check drives the env's own loop through the Python
bindings: `E2eDriver.peek` names the position (forced, decision or end), and
`E2eDriver.apply` applies a forced action or the chosen class's action. That is
`run_to_decision` (reading item 6), step for step. The network's panel runs
use `sts2_rs.e2e_selfplay`, which is fast and threaded. Each run is then
replayed through the logged loop with the same choices. The check stops if a
replay leaves its self-play run, so both paths are known to play the same
episode. Nothing in the check substitutes for the repository's mechanism.

## Reading the result

- **The trace.** Each line is one decision: its floor, whether it is inside a
  fight, its type, how many classes were offered, and the pick. "(+k forced
  before)" counts the forced steps the env applied since the previous
  decision. Compare where random play's run ends with where the network's
  ends.
- **The fight decision.** Count the actions in the hand against the classes
  listed. A class marked "[n identical actions merged]" is one option the
  policy scores once. The line "true top of the draw pile" is the game's
  hidden state; check whether any token section carries it.
- **The token rows.** A fight row and a card-reward row share some sections
  (RUNCTX, BOSS, the five SUM_ summaries, RELIC, POTION) and differ in others
  (HAND and ENEMY against DECK and OPT_CARD). One network reads both, so the
  shared sections are what make it one policy.
- **The panel.** Decisions per run is the episode length. It is what reward
  must travel back across in stop 4. The fight share tells you which kind of
  decision dominates the training data. Classes per decision is the width of
  the choice the policy's output must cover. Compare the two players: the
  same env gives very different episodes depending on who plays.
- **Forced share.** If it is small, forced steps barely shorten the
  sequence. If it is large, the env removes much of the game from the
  agent's sequence. The check also reports what share of forced steps happen
  inside fights.

## Note prompt

Answer in your own words, in your wiki:

1. Define **state**, **action**, **decision**, **episode** and **return** for
   this game. Say where an episode begins and the event that ends it. Say what
   the return is the sum of (read `env.py` lines 41-48 again).
2. Which parts of the true game state does the policy *not* see at a decision?
   Name at least two, and say where in the code the line is drawn.
3. A forced step changes the game but makes no row. What does that do to the
   episode an RL algorithm sees, compared with the game as played?
4. Why merge identical actions into one class? What would go wrong in the
   policy's probabilities if three identical Strikes stayed three options?
5. Your prediction that missed by the most: what did you assume that the
   measurement contradicts?

## Going further

- Run with `--seeds 1000` for tighter numbers; it takes about five times as
  long.
- In the fight showcase, the probabilities belong to the network's output.
  Stop 2 asks what greedy play throws away from them.
- Change `random_chooser` to never pick "end turn" while any card is playable.
  How far does that player get?
