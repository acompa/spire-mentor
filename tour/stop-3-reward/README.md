# Stop 3: Reward and return

## Question

Which number does training maximise, and what does that number leave out?

Four different things in this repository get called "reward". This stop is
about the first two only:

1. **Outcome facts.** What happened in a run: the floor it reached, which
   bosses it beat, whether it won, how much HP it had left. Facts are not
   rewards. They are inputs to one.
2. **The reward function.** A rule the designer wrote that turns some of
   those facts into numbers. The environment applies it while the run is
   played.
3. A **value estimate**: the critic's guess of the reward still to come.
   Stop 4.
4. A **learned reward model**: a network trained to predict reward. Instinct
   has none.

The **return** of a run is the sum of all the rewards the function gave it.
Training changes the network so that runs get a higher return on average.
That sum is the number this stop teaches you to compute by hand.

### One example first

Take a run that walks onto floor 1, then floor 2, and so on, beats the
act-1 boss on floor 16, and dies on floor 18. The rule pays:

- 0.002 for each new floor: 18 floors, so 0.036.
- 0.05 for the act-1 boss: 0.05.
- nothing for dying.

Its return is 0.036 + 0.05 = 0.086. No other fact counts. The run could
have ended at full HP or at 1 HP, with a great deck or a bad one, after two
elites or none: the return is the same.

### The rule

| Event | Reward |
| --- | --- |
| Win the run | +1.0 |
| Beat a boss, except the one whose defeat ends the run | +0.05 each |
| Reach a floor you have not been on before | +0.002 each |
| Die, or hit a cap (too many turns or decisions) | 0 |
| HP, gold, cards, relics, upgrades, elites | nothing |

"No discounting" (γ = 1) means a reward counts the same whether it arrives
on the next decision or 700 decisions later. A win at the very end is worth
exactly 1.0 from the first decision on. With discounting, say γ = 0.99, that
same win would be worth 0.99^700, nearly zero, seen from the start.

Notice what earns nothing by itself. Fighting an elite pays no reward. Losing
HP costs no reward. So if the trained network prefers some elites, or
protects its HP, that preference cannot come from the reward of that moment.
It must come from what elites and HP do to the reward that comes later:
whether the run beats the next boss and wins. Stop 4 shows how training
connects a later reward to an earlier choice.

## Read

About 75 minutes. Paths are from the repository root.

1. `docs/plans/2026-11-02-ironclad-e2e-ppo-v0111.md`, line 276, the
   "Reward (pre-registered)" paragraph. The rule as the author wrote it
   before training. Note the reason given for paying nothing for HP.
2. Same file, line 267, the "Returns" row of the section 6 table. Only its
   first sentence and its last one ("r_t sums all reward to the next stored
   row") belong to this stop: γ = 1, and a row's reward is everything that
   arrived before the next decision was stored. Leave the λ and A_t parts
   for stop 4.
3. Same file, line 273, the "Precision" row. A side effect of a small floor
   reward: 0.002 is smaller than the spacing of 16-bit floats near 1.0,
   which is why the value output stays in 32 bits.
4. `docs/experiments/2026-11-03-ironclad-e2e-v111/PREREG.md`, lines 107-109,
   "Reward". The same rule, fixed before the experiment so it could not be
   tuned to the results.
5. `sts2-pycli/python/ppo/e2e/env.py`, lines 41-48. The environment's own
   summary of rows and reward, and the identity
   `sum(rewards) == total_reward` (T4) that this stop's check tests.
6. Same file, lines 117-119. The three constants. This is the whole reward
   function's data.
7. Same file, lines 1431-1446, `_accrue_floors` and `_assign_pending`.
   Reward is collected into a pending sum and credited to the most recent
   stored row. Ask: when does a floor's 0.002 actually reach a row?
8. Same file, lines 1525-1539 (`_terminal`) and 1590-1601 (`_end_fight`).
   Where the win and the boss bonus are added. Find the condition that stops
   the final boss from paying 0.05 on top of the win.
9. `sts2-rs/crates/sts2-e2e/src/train.rs`, lines 42-47, 976-1000, 1095-1097
   and 1171-1181. The same code ported to Rust. The rollout workers ran this
   copy, and so does this stop's check. Compare it line by line with items
   6 to 8.
10. `sts2-pycli/python/ppo/e2e/rsenv.py`, lines 1-9. Why there are two copies
    and how they were held equal.
11. `sts2-pycli/python/ppo/e2e/algo.py`, lines 208-225, `row_rewards`. How
    rewards from steps that were not stored are folded onto stored rows,
    so nothing is lost and the sum is unchanged.
12. The contrast. `sts2-pycli/python/ppo/rewards.py`, lines 1-34 and
    69-137: version 1's reward. It paid for graded progress and added a
    shaping term for HP, upgrades and relics. Then
    `sts2-pycli/python/ppo/shaping_fit.py`, lines 1-14: how its
    coefficients were fitted. Instinct dropped all of this. Note what the
    `rewards.py` docstring claims about why shaping of this form cannot
    change the best policy, and what it says shaping can still change.

## Predict

Fill in `predictions.toml`: a guess and an 80% interval for each. All
returns are in reward units, where a win alone is 1.0. An A10 run that wins
ends on the act-3 second boss, on floor 46 on most maps.

| Key | Quantity | Unit |
| --- | --- | --- |
| `won_run_return` | The return of a run the network wins (the median over its wins). | reward |
| `death_floor_20_return` | The return of a run that dies on floor 20 after beating the act-1 boss. | reward |
| `death_floor_40_return` | The return of a run that dies on floor 40 after beating the act-1 and act-2 bosses. | reward |
| `largest_losing_return` | The largest return a losing run can have. | reward |
| `net_mean_return` | The published network's mean return over the panel. | reward |
| `net_win_share` | The share of that mean that comes from the win bonus. | fraction, 0 to 1 |
| `random_mean_return` | Mean return when every decision is picked uniformly at random. | reward |

The first four you can compute with the rule above. The last three need a
guess about how well each policy plays.

## Run

```bash
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-3-reward/check.py
```

The default plays 300 seeds of `PANEL_A` with the network and the same 300
with random play, on 4 processes, in about 7 minutes on an M2 Max shared
with other jobs. `--quick` plays 24 of each in under a minute. Tests:

```bash
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run -m pytest tour/stop-3-reward
```

What the check does:

1. Plays the first `PANEL_A` seed greedily with the published Ironclad
   network in the training environment and prints a **reward ledger**: one
   line for each time reward arrived, with the stored row it was credited
   to, the act and floor, what triggered it, and the return so far.
2. Plays the panel with the network and with random play. For each policy
   it prints the mean return and how much of it came from floors, from
   bosses and from the win.
3. Scores the hypothetical runs from "Predict" with the same rule.

Every run is checked three ways, and the check stops if any two disagree:
the ledger's sum, the environment's own `total_reward`, and the rule applied
to the run's outcome facts (floor reached, bosses paid, won). The tests keep
that check.

**What is substituted, and why.** The tour's other stops play the `.e2ew`
weights with `sts2_rs.e2e_selfplay`. That loop has no reward in it: the
inference environment it uses never ported the reward code. So this check
plays the checkpoint `ironclad_e2e_d9000_ema0999.pt` in PyTorch, greedily,
on the training environment's own token rows, the way the rollout workers
feed the network. It is the same network in a different file: on the
first 8 seeds of `PANEL_A` both files played to the same outcome, floor and
number of decisions (checked on 2026-11-25, not part of the check). The ledger
splits each increment into floor, boss and win parts for display only; the
split is checked to add back up to the env's increment exactly.

## Reading the result

- In the ledger, most lines are 0.002. Look at where the 0.05 lines fall
  and which floors they are on. Look for a line that pays a floor and a
  boss together, and ask why they arrived at the same row.
- Find the row where the 1.0 arrives. Count how many decisions came before
  it that earned nothing at all.
- In the panel summary, compare the three parts of the network's mean. The
  part from the win is the win rate times 1.0. If that part dominates, then
  for training purposes the return is nearly "did it win", with small
  tie-breakers.
- Random play's return shows what the floor reward pays a policy that knows
  nothing. Compare it to one won run.
- The largest losing return is printed from a won run with its win bonus
  taken away: the same floors and bosses, minus the 1.0. Compare it to a
  win to see how big the gap at the last fight is.

## Note prompt

Answer these in your own words in your wiki note.

1. Name the outcome facts a run produces. Then write the reward function as
   a rule on those facts. Which facts does the rule read, and which does it
   ignore?
2. Define the return of a run. Compute it for one run you make up, and
   say what "undiscounted" (γ = 1) means for a win that arrives 700
   decisions after the first one.
3. What does this reward not pay for? Give at least HP, elites and deck
   quality. Why did the author choose that, given the plan's own reason and
   version 1's shaping that Instinct removed?
4. Fighting an elite pays nothing and losing HP costs nothing. If the
   network still avoids some elites or protects HP, where can that
   preference come from?
5. How much of the network's return is the win? What does that say about
   what "maximise the return" means here in practice?

## Going further

- Version 1's shaping is potential-based: each step pays
  Φ(next state) − Φ(this state), so with γ = 1 the shaping terms telescope
  to a constant per run. Check this on paper for a three-step run. Then ask
  why removing it could still change what training does (the `rewards.py`
  docstring's last paragraph answers this).
- `sts2-pycli/python/ppo/e2e/probe.py`, lines 8-16: the win probe learns
  "P(win)" by subtracting the floor and boss reward still to come from the
  critic's value. That is a value estimate (item 3 in the list at the top),
  not a reward. Keep it for stop 4.
- Run the check with `--runs 1000` to see how stable the network's mean
  return and win share are on a larger panel.
