# Stop 6: Where on-policy bends

## Question

The rows in a training batch were played by a slightly older network. How
does the trainer correct for that, and how far can the correction stretch?

## The idea in brief

PPO is an *on-policy* method: its update assumes the rows were played by the
network being updated. This trainer cannot meet that exactly. Many workers
play while the learner updates, so a row can be played by weights that are a
few updates old. Run rows also wait up to four iterations before they train.

Some terms, each used below:

- **Behaviour policy** (π_behav): the policy that actually chose the action
  in a row. Here it is the network the learner served when the row was played.
- **Target policy**: the policy you want to learn about or improve.
- **Importance weight**: for one row, π_target(a | s) / π_behav(a | s), the
  probability the target gives the action that was taken, over the
  probability the behaviour policy gave it.
- **Effective sample size (ESS)**: (Σw)² / Σw² over n weighted samples. With
  equal weights it is n. When one weight dwarfs the rest it is about 1.

A worked example. In one fight state, the acting network gave "play Bash"
probability 0.40 and sampled it. By update time the network gives Bash 0.50.
The row's importance weight is 0.50 / 0.40 = 1.25: this action is a little
more typical of the current network than of the one that played it, so the
row counts 25% more. If the update-time network gave Bash 0.10, the weight
would be 0.25. If it gave 0.90, the weight would be 2.25, and the trainer
cuts it to 2.

The trainer splits the old single PPO ratio into two pieces ("decoupled PPO",
Hilton et al. 2021):

1. π_prox is the network at the start of this update. A pre-pass computes
   its log-probability for every row's action.
2. The importance weight π_prox / π_behav corrects for staleness. It is
   clamped at 2.
3. The clipped ratio π_θ / π_prox is the usual PPO trust region. It is
   measured against π_prox, not against the old acting network.

So the clip limits how far one update moves, and the weight does the
off-policy correction. Each row gets one weight for one decision. Nothing
multiplies weights along a run.

That last point is what this stop measures. Off-policy evaluation of a whole
run needs the product of every per-decision weight in the run. A run here has
about 700 decisions.

## Read

About 60 to 90 minutes. All paths are from the repository root; the line
ranges were checked on this branch.

1. `docs/plans/2026-11-02-ironclad-e2e-ppo-v0111.md:264` (the Objective row
   of section 6). The whole design in one line. It names `ranks.py`, which is
   the older combat trainer (`sts2-pycli/python/ppo/combat_ppo/ranks.py`);
   the end-to-end trainer implements this row in `algo.py` (item 6).
2. Same file, line 267 (the Returns row). Look for "staleness mean 3.7,
   max 5": run rows wait for their run to end, so they train on weights
   several updates newer than the ones that played them.
3. `sts2-pycli/python/ppo/e2e/learner.py:10-21`. Serving: the learner runs
   the network for every worker, samples the action, and keeps the
   behaviour log-prob and the acting net's version with each row.
4. `sts2-pycli/python/ppo/e2e/actors.py:13-35`. The worker's side of that
   protocol. Notice that the worker never computes a probability: it sends
   states and receives actions.
5. `sts2-pycli/python/ppo/e2e/learner.py:1738-1770`. Where π_behav is
   recorded. Line 1754 samples the action, 1755 takes its log-prob, 1769
   stores it as `logp_behav`. Lines 1738-1746 replace the student's
   log-probs with a teacher's on rows a teacher plays.
6. `sts2-pycli/python/ppo/e2e/learner.py:466-478` and `2498-2499`. Each
   round of rows carries `version`; the version goes up by one each time
   new weights are loaded. Staleness is the difference.
7. `sts2-pycli/python/ppo/e2e/update.py:607-612`. The pre-pass: `lp_prox`
   is the log-prob under the weights at the start of the update.
8. `sts2-pycli/python/ppo/e2e/algo.py:771-775` and `935-949`. The loss.
   Line 940 is the PPO ratio π_θ/π_prox, 941 the importance weight
   π_prox/π_behav, 942 the clamp at 2, 943-949 the clipped surrogate times
   the weight. Ask which factor carries a gradient and which does not.
9. `sts2-pycli/python/ppo/e2e/algo.py:252-262` and `443`. `pi` rows, the
   only rows with a PPO term, are the student-acted ones. A row a teacher
   played is data from a truly different policy: the trainer does not try to
   reweight it at all. It trains only the value head and the teacher term.
10. `sts2-pycli/python/ppo/e2e/update.py:914`, `924-936` and `1053-1067`.
    What the trainer logs about this: the tails of the weight, the share
    above the clamp, and the mean and maximum staleness.
11. `docs/experiments/2026-11-03-ironclad-e2e-v111/PREREG.md:100` (B-T0:
    workers were chosen for the best rate "at staleness ≤ 1") and
    `151-158` (Amendment 5: one epoch per update, so π_θ starts each update
    equal to π_prox).
12. `sts2-rs/crates/sts2-py/src/e2e.rs:121-168` and `295-384`. `decide`
    returns a network's logits at a state; `peek` and `apply` step a run.
    The check uses these to ask two networks about the same state.

## Predict

Two published networks both play Ironclad. **A** is
`ironclad_e2e_d9000_ema0999`. **B** is `allchar_e2e_d4500_ema0999`, trained
on all five characters. A plays 48 seeds of `PANEL_A`, sampling every
decision from its own probabilities. A is the behaviour policy throughout.

- **Small gap**: the target is A moved a tenth of the way towards B (in log
  probability). It stands in for a network a few updates newer than A.
- **Large gap**: the target is B.

Fill in `predictions.toml` (a guess and an 80% interval for each):

| Key | Quantity | Unit |
| --- | --- | --- |
| `small_gap_clamped_pct` | Small gap: share of per-decision weights above 2 | % of A's decisions |
| `large_gap_clamped_pct` | Large gap: share of per-decision weights above 2 | % of A's decisions |
| `top_action_agreement_pct` | How often A and B rank the same action first, on the states A visits | % of A's decisions |
| `small_gap_run_ess_pct` | Small gap: ESS of per-run weights (the product of all ratios in a run) | % of runs |
| `large_gap_run_ess_pct` | Large gap: ESS of per-run weights | % of runs |
| `plain_is_log10` | log10 of the plain per-run IS estimate of B's win rate from A's runs | log10 of a probability |
| `snis_error_pp` | Absolute error of the self-normalised per-run IS estimate of B's win rate, against B's measured win rate on the same seeds | percentage points |

The plain estimate is the mean of w·won over A's runs. The self-normalised
estimate is Σ w·won / Σ w. The truth comes from B playing the same 48 seeds,
sampling from its own probabilities.

## Run

```bash
tour/run stop-6-off-policy/check.py            # 48 runs per net
tour/run stop-6-off-policy/check.py --quick    # 8 runs per net
tour/run -m pytest tour/stop-6-off-policy      # the arithmetic on hand-made cases
```

From a worktree, prefix with `TOUR_VENV=~/git/spire-mentor/sts2-pycli
TOUR_WEIGHTS=~/git/spire-mentor/weights`. The check plays in 4 processes;
`--workers 10` is faster on an idle machine. A run where both networks are
asked at every decision takes about 15 s on one core of an M2 Max, and B's
runs about 7 s, so the default takes about 5 minutes on an idle machine.

What runs:

1. A plays each seed through `peek`, `decide` and `apply`, sampling from
   its softmax. At every decision B is also asked for its logits on the same
   state. Asking B does not change what A plays: with greedy choices this
   loop reproduces `sts2_rs.e2e_selfplay` move for move.
2. B plays the same seeds the same way, without A, for the truth.
3. Everything else is arithmetic on the stored log-probabilities, in log
   space so a product of 700 ratios cannot overflow.

Substitutions, and why:

- The trainer's behaviour policy is the same network a few updates earlier.
  Those intermediate weights are not published, so the small gap is made by
  hand: log π_target = log-softmax(0.9·log π_A + 0.1·log π_B). The size of
  the step (0.1) is a choice, not a measurement of real staleness. The sweep
  table shows other sizes.
- Rows here are all played by A. The trainer also has teacher-played rows,
  which it leaves out of the PPO term (Read item 9), so they are left out
  here too.
- This uses the published Rust inference networks (`.e2ew`), not the
  PyTorch module the learner serves during training.

## Reading the result

The check prints, in order:

- How often A and B agree on the top action, and each network's sampled
  win rate. High agreement on the top action can coexist with large
  per-decision weights, because the weight depends on the whole probability,
  not on the ranking.
- For each gap, the spread of per-decision weights (1st percentile, median,
  99th percentile, maximum), the share above the clamp, and two ESS values:
  over single decisions, and over whole runs. Compare the two ESS values.
  Per-decision ESS near 100% with per-run ESS far lower means the
  individual corrections are mild but their product is not.
- The per-run log10 weights: the smallest, median and largest. Their range
  tells you how many orders of magnitude separate the runs.
- A sweep table. Rows are step sizes from A towards B (1 is B itself).
  Columns are the ESS share when you multiply k consecutive ratios, for k =
  1, 10, 100, and the whole run. Read across a row to see the horizon break
  the correction, and down a column to see the gap break it. The last
  column is the share above the clamp.
- The off-policy evaluation. A's own win rate is the estimate with no
  correction. The plain estimate is unbiased in expectation, but look at
  what one sample of it is. The self-normalised estimate is a weighted
  average of 0s and 1s; when the per-run ESS is about one run, it is about
  one run's outcome.

The truth is itself a sample of 48 runs, so it has a standard error of
several points. Do not read a small error as more precise than that.

## Note prompt

Answer these in your own words in your wiki note:

1. Define behaviour policy, target policy, importance weight and effective
   sample size, using a row from this trainer as the example: which network
   is which, and where in the code each number is computed.
2. The trainer clamps π_prox/π_behav at 2. What does the clamp do to bias,
   and what does it do to variance? What did the measured clamp shares tell
   you about how much the clamp matters for nearly current data?
3. Why is a per-decision weight usable when a per-run weight is not? Use your
   sweep table, and say what the per-decision correction gives up by not
   correcting the return that follows the action.
4. Decoupled PPO measures the clip against π_prox and not π_behav. What
   would go wrong with an old actor's rows if the clip were measured against
   π_behav?
5. Why is learning from someone else's logged play (for example, a human's
   or a teacher's) a different problem from this one? Connect it to what the
   trainer does with teacher-played rows.

## Going further

- Run with `--runs 96` and see whether the self-normalised error moves.
- Change `SMALL_EPS` in `check.py` to the step size at which about 1% of
  per-decision weights pass the clamp, and compare with the `is_tails`
  read of the real training logs if you have them.
- Per-decision importance sampling for off-policy evaluation (Precup, Sutton
  and Singh 2000) multiplies only the ratios up to each reward. This
  trainer's reward (plan, the paragraph after section 6's table) pays small
  bonuses per floor and per boss but +1 only for a win, at the end. How much
  would per-decision IS help with the win rate here?
