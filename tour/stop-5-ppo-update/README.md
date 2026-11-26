# Stop 5: The PPO update

## Question

What does one update do to the network, and what limits the size of the
change?

Stop 4 gave every choice an **advantage**: how much better the run went
after that choice than the critic expected. This stop is about what the
learner does with those numbers.

### One iteration, in words

1. The current network plays. It samples its choices from its own
   probabilities, so it sometimes picks a choice it rates low. Every choice
   is stored as a row: what the network saw, which option it took, and the
   probability it gave that option.
2. The critic scores the rows, and each row gets an advantage (stop 4).
3. The **update** changes the weights so that choices with a positive
   advantage become more likely and choices with a negative advantage become
   less likely.
4. The new network plays the next batch, and the loop repeats.

### One row, traced

A combat turn offers three options, A, B and C. The network gives them
probabilities 0.5, 0.3 and 0.2. It samples B. The advantage of that row
comes out at +1.5 (advantages are normalised, so +1.5 means "clearly better
than usual").

If you trained a classifier on this row with B as the label, the
cross-entropy gradient would push the probability of B up. The PPO update
does that too, with the push scaled by the advantage: +1.5 pushes B up
firmly, an advantage of -1.0 would push B down, and 0 would leave it alone.
So **one update is supervised learning on the network's own choices, with
each example weighted by its advantage**. Nothing tells the network what the
right answer was. It only learns which of its own choices went better or
worse than expected.

### Why the step has to be limited

The rows were collected by the network as it was before the update. Call
that the **old policy**. The advantage of B was measured on a run where the
old policy made all the other choices too. As soon as the weights change,
the batch describes the new policy less well: the new policy would make
other choices, reach other states, and see other advantages. A small change
keeps the batch roughly valid. A large one trains on data about a network
that no longer exists. This is the core rule of on-policy learning: the data
must come from the policy you are improving, or close to it. (Q-learning
avoids this: its target does not depend on which policy collected the
transition. Stop 6 is about where this repository bends the rule.)

PPO limits the change per row with the **probability ratio**:

    ratio = π_new(B) / π_old(B)

Before any step the ratio is 1. Suppose a step lifts B from 0.30 to 0.36:
the ratio is 1.2. PPO **clips** the ratio to the band [0.8, 1.2]
(`clip = 0.2`). Once a row with a positive advantage has its ratio above
1.2, the loss stops rewarding further increases: that row's gradient becomes
zero. A row with a negative advantage stops pushing once its ratio is below
0.8. The other rows keep training. The **clip fraction** is the share of
rows that hit the band during the update.

### How far did the policy move?

The **KL divergence** from the old policy to the new one, on one state, is

    KL = Σ over options of  π_old(x) · log(π_old(x) / π_new(x))

It is zero when the two agree and grows as they differ. With the old
probabilities (0.5, 0.3, 0.2) and new ones (0.46, 0.36, 0.18), it is
0.5·log(0.5/0.46) + 0.3·log(0.3/0.36) + 0.2·log(0.2/0.18), about 0.008 nats.
The learner averages it over a fixed sample of rows (the *probe*) after each
epoch. KL measures the move. The clip limits it.

### Two more numbers

- **Entropy** says how undecided the policy is: Σ -π(x)·log π(x). For
  (0.5, 0.3, 0.2) it is 1.03 nats. The learner divides by log(number of
  options), so 1.0 means uniform and 0 means certain. A random network starts
  near 1. Training adds an **entropy bonus**, β times the entropy, to what it
  maximises, so the policy does not become certain before it has tried
  enough.
- **Explained variance** (EV) says how well the critic predicts the value
  targets: 1 - Var(target - V) / Var(target). 1 is perfect, 0 is no better
  than predicting the average, and negative is worse than that.

### What this stop runs

The author's **learning check** (plan §7) trains a network from random
weights with no teacher. It passes if the mean floor of the last 10,000
finished runs beats the first 10,000 by at least 1.0. This stop runs the
learner itself (`python -m ppo.e2e.learner train --mode learning_check`) at
a size that fits this Mac, twice at once:

- **clipped**: the learner's loss and schedule, scaled down (below).
- **unclipped**: the clip set to 10⁶ (never reached), a learning rate 10
  times larger, and 4 epochs per batch instead of 2. Nothing in the policy
  loss limits the step. The gradient-norm clip (1.0 per step) and the dual
  clip on negative advantages still apply.

## Read

About 75 minutes. Line numbers are on this branch.

1. `docs/plans/2026-11-02-ironclad-e2e-ppo-v0111.md`, lines 260-276 (§6).
   Read the Objective, Batches, KL control, Entropy and Optimizer rows. Look
   for: the clip (0.2), how many decisions one iteration collects, and when
   the KL limit is switched on.
2. The same file, line 291 (§7, "Learning check"). Look for: what passes,
   and what the check leaves out (a teacher).
3. `sts2-pycli/python/ppo/e2e/update.py`, lines 1-43. The update's protocol
   in six steps. Look for: the pre-pass that records π_prox, and when the KL
   is measured.
4. `update.py`, lines 581-667. The pre-pass and the advantages. Look for:
   `lp_prox`, the log-probability of the taken option under the weights at
   the start of the update. That is the "old policy" of the ratio.
5. `update.py`, lines 780-826. The epochs. Look for: one optimiser step per
   minibatch, the probe KL after each epoch, and `a.kl.stop`.
6. `sts2-pycli/python/ppo/e2e/algo.py`, lines 860-1023, `ppo_losses`. The
   whole loss. Spend most of your time on lines 935-958: the ratio, the
   clip, the dual clip and the entropy bonus. Then lines 990-1005: the `kl`
   and `clipfrac` statistics. The plan names `ranks.py` for this; on this
   branch the clipped objective lives here. Look for: which term is the
   "advantage-weighted cross-entropy", and where `torch.min` drops the
   gradient.
7. `algo.py`, lines 1103-1128 (`probe_kl`, the exact KL on the probe rows),
   465-472 (`explained_variance`) and 1407-1417 (`clip_and_step`, the
   gradient-norm clip).
8. `sts2-pycli/python/ppo/e2e/schedules.py`, lines 337-352 and 545-552. The
   phases and the KL hard stop. Look for: in which phases the KL stop is on,
   and so whether it is on during a learning check.
9. `sts2-pycli/python/ppo/e2e/learner.py`, lines 385-415 (the batch and loss
   settings) and 2922-2958 (`learning_check`), with 2239-2242 (how finished
   runs fill the first and last windows).

`update.py` also handles teachers, distillation, held rows and several
devices. Skip those parts: they are other stops' topics.

## Predict

Fill in `predictions.toml`: a guess and an 80% interval for each.

| Key | Quantity | Unit |
| --- | --- | --- |
| `floor_start` | Mean floor of the clipped arm's first 1,000 finished runs (random weights) | floor |
| `floor_end` | Mean floor of its last 1,000 finished runs | floor |
| `kl_per_update` | Typical KL between the policy before and after one update, clipped arm (median over iterations) | nats |
| `clip_fraction` | Share of rows whose ratio left [0.8, 1.2] during an update, clipped arm (median over iterations) | fraction |
| `entropy_change` | Normalised entropy of the last three policy iterations minus that of the first, clipped arm. Negative means the policy became more decided | fraction of the maximum |
| `decisions_per_second` | The clipped arm's training throughput: decisions stored per second of wall time, start-up included, with the other arm running beside it | decisions/s |
| `unclipped_kl_per_update` | Typical KL per update in the unclipped arm | nats |

A hint for scale, not an answer: one greedy run of the trained network is
about 700 decisions, and the plan says random play reaches floor 2.7.

## Run

```bash
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-5-ppo-update/check.py
```

About 14 minutes. It starts two learners (4 rollout workers and 2 update
threads each), so close other heavy work first: throughput depends on free
cores. `--quick` runs each arm for about a minute; it checks the plumbing
and does not learn. `--keep DIR` keeps each learner's directory
(`stats.jsonl` has every statistic the learner logs per iteration).

### What the check runs instead of the full learning check

The learner, its loss, its advantages and its schedules are the
repository's own. What changes is size, because this Mac has no CUDA GPU:

| Setting | Plan | Here | Why |
| --- | --- | --- | --- |
| Device | CUDA, bf16 | CPU, fp32 | MPS fails inside the learner (the sampler's `torch.multinomial` and the token densifier) |
| Network | 6 layers × 256 | 2 layers × 128 (`SMALL_NET`) | The full net updates at about a tenth of the speed on a CPU |
| Decisions per iteration | 131,072 | 8,192 | Enough iterations in 11 minutes to see a trend |
| Minibatch | 8,192 rows (24 strata) | 1,024 rows (8 strata) | Keeps several optimiser steps per epoch |
| Windows | 10,000 runs | 1,000 runs | Fewer runs finish in 11 minutes |
| Value-only iterations | 3 | 1 | More of the short run trains the policy |
| Learning-rate warm-up | 10 iterations | 3 | Same reason |
| Learning-rate anneal | from Nov 8, 2026 | moved past today | The author's check ran before the anneal; after it the rate is 0.3× |
| Wall time | 45 min (laptop) | about 11.5 min per arm, both at once | The tour's time limit |

Everything else is the learner's default: clip 0.2, 2 epochs, AdamW with
learning rates 1e-4 (trunk) and 3e-4 (value), gradient-norm clip 1.0, the
entropy coefficients, no teacher. The learner collects the next batch while
it updates on the last one, so some rows come from the policy one update
older; the loss corrects for that (stop 6).

## Reading the result

Each arm prints one row per iteration:

- `phase`: `V-warm` trains only the critic. `U0` trains the policy too. In a
  learning check the KL hard stop is off in every iteration (it only runs in
  U1 and U2), so the clip is the only limit in the policy loss.
- `floor`: the mean floor of the runs that finished in that iteration. It is
  noisy; look at the trend over many rows, and at the two windows printed
  under the table.
- `KL`: the exact KL between the weights before and after this update, on
  the probe rows. In the clipped arm, look at whether it stays roughly level
  from iteration to iteration. Compare the two arms' columns directly.
- `clip%`: what share of rows hit the band. A share near zero says the clip
  was not the thing limiting the step; the learning rate was.
- `entropy`: watch its direction. A policy that learns usually becomes more
  decided. A sudden fall to near zero is a **collapse**: the policy stopped
  exploring.
- `EV`: the critic's explained variance on combat rows. It starts near or
  below zero and should rise as the critic learns.
- `dec/s`: collection throughput in that iteration. The table's last line
  gives the throughput over the whole run.

Questions to ask of the output: Did the floor rise in the clipped arm? Did
the unclipped arm's KL differ from the clipped arm's by a small factor or a
large one? What happened to its entropy and its floor? If the clip fraction
in the clipped arm is small, what does that say about which limit was
active?

## Note prompt

Answer in your own words in your wiki note:

1. Describe one PPO update as weighted supervised learning on the network's
   own play. What plays the role of the label, and what the role of the
   weight?
2. Define the probability ratio and the clip. For a row with a positive
   advantage, when does the clip stop its gradient, and why only on one
   side?
3. What does the KL between the old and new policy measure, and how is it
   different from what the clip does? Use your measured KL and clip fraction.
4. Why must the training data come from the current policy (or close to
   it)? Contrast this with how you would reuse a replay buffer in DQN.
5. What does the entropy bonus protect against, and what did your entropy
   column do?

## Going further

- Rerun with `--keep DIR` and plot `update.ploss`, `grad_norm` and
  `update.kl_m` from `stats.jsonl` against the iteration.
- Isolate the clip. Copy `check.py`'s `ARMS` and add an arm with
  `"clip": 0.2, "epochs": 4, "lr_scale": 10.0`. If its KL stays near the
  clipped arm's while the unclipped arm's does not, the clip did the work.
- In the campaign (U1 and U2) the learner also stops an update's remaining
  epochs when the probe KL passes 0.06, and scales the learning rate by 1.5
  up or down to keep KL between 0.005 and 0.02 (`schedules.py`, lines
  545-576). Why would a campaign need this on top of the clip?
- The author's own learning-check results are in
  `docs/experiments/2026-11-03-ironclad-e2e-v111/CAMPAIGN_LOG.md` (search
  "Learning check passes"). Read them after your run: they used the full
  network and about 50 times more decisions.
