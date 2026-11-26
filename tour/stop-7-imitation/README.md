# Stop 7: Imitation, then RL

## Question

A student network is trained to copy a teacher. On most decisions it picks
the move the teacher would pick. Why does it win far less often than the
teacher?

Some words first.

- **Teacher.** A policy that already plays well. Here it is the published
  Ironclad network, `ironclad_e2e_d9000_ema0999`.
- **Behaviour cloning (BC).** Supervised learning on the teacher's play.
  Let the teacher play. At every decision, store the state and the move it
  chose. Then train the student to give that move high probability. No
  reward appears anywhere. It is classification, with the teacher's move as
  the label.
- **Agreement.** The share of decisions where the student's greedy move
  equals the teacher's greedy move, at the same state.
- **Distribution shift.** The student is trained on states the teacher
  visited. When the student plays, its own mistakes take it to states the
  teacher rarely or never visited. There it has little or no training data.
- **DAgger** (Dataset Aggregation). One fix for distribution shift. Let the
  student play, have the teacher label the states the student reaches, add
  those rows to the dataset, and retrain.
- **Warm start.** Starting reinforcement learning from a network that
  already plays reasonably, instead of from random weights.

One concrete decision. The teacher is at a card reward with four merged
classes: three cards and "skip". It picks class 2. The dataset gets one row:
the tokenized state, label 2. If the student gives class 2 a probability of
0.25, the loss on that row is `-log 0.25 = 1.39`. Training pushes that
probability up. Repeat over every stored decision, and that is all of
behaviour cloning.

Now the problem. A run is several hundred decisions. Suppose the student
plays a Strike where the teacher would have played Defend, and loses 12 HP.
Every later state in that run differs from anything in the teacher's data:
lower HP, a different fight, perhaps a different deck. The student was never
corrected in those states, so it errs more there, which moves it further
from the teacher's states. Errors compound. Agreement measured on the
teacher's own states does not see this, because those states were never
reached by the student's mistakes.

The project's trainer does not use imitation as the goal. It uses it as a
warm start: the new network imitates older networks first, then PPO takes
over while the pull towards the teachers fades to zero.

## Read

About 60 to 90 minutes.

1. `docs/plans/2026-11-02-ironclad-e2e-ppo-v0111.md`, lines 280-292
   (section 7, "Warm start"). The four phases V-warm, U0, U1 and U2 and the
   K2 gate. Note who acts in each phase, which loss term carries the teacher,
   and that K2 asks for agreement and win rate as two separate checks.
2. `docs/site/training.html`, lines 253-273. The same warm start in plain
   words, and why a random-init learning check runs before each restart.
3. `docs/site/v1/training.html`, lines 259-268 ("Starting points"). The
   author's version 1 clone: how often it matched its teacher, and how its
   win rate compared. This is the effect this stop reproduces.
4. `sts2-pycli/python/ppo/bc.py`, lines 1-28 and 159-211. Version 1's
   behaviour cloning. Look at the held-out split (by run, never by
   decision: why?), the hard cross-entropy on the expert's chosen index, and
   the soft cross-entropy when the teacher's probabilities are available.
5. `sts2-pycli/python/ppo/e2e/teachers.py`, lines 1-46 and 211-228. The
   warm-start teachers of the current trainer, and how a teacher's
   per-option probabilities become probabilities over the student's merged
   classes.
6. `sts2-pycli/python/ppo/e2e/schedules.py`, lines 299-315 and 337-350.
   `beta_act` (how often the teacher acts) and `lam_ks` (the weight on the
   pull towards the teacher). Write down both as functions of decisions.
7. `sts2-pycli/python/ppo/e2e/algo.py`, lines 958-975 and 990. The teacher
   term inside the PPO loss: `λ_KS · KL(π_T ‖ π_S)`. Compare it with the
   cross-entropy in item 4: they differ by a term that does not depend on
   the student.
8. `sts2-pycli/python/ppo/e2e/distill.py`, lines 1-14 and 212-224. A later
   use of the same idea: a frozen teacher scores rows and its probabilities
   are written into the rows the update reads.
9. `tour/stop-7-imitation/imitation.py`, all of it (about 380 lines). This
   stop's code. `play` is the game loop, `train` is behaviour cloning,
   `prepare` builds the datasets and students, `measure` and `quantities`
   produce the numbers you predict.

## Predict

Fill in `predictions.toml`: a guess and an 80% interval for each.

The setup. The teacher plays `PANEL_B` runs. Three students are trained by
behaviour cloning on the teacher's decisions from its first 25, 50 and 100
runs (each dataset contains the smaller ones). A fourth student, the DAgger
student, is described below. "The student" in the questions means the one
trained on 100 runs.

| Key | Quantity | Unit |
| --- | --- | --- |
| `heldout_agreement` | On 10 teacher runs nobody trained on, how often the student picks the teacher's move. | percent of decisions |
| `own_state_agreement` | When the student plays `PANEL_A` runs itself, how often the teacher, shown the same state, would pick the same move. | percent of decisions |
| `student_mean_floor` | The student's mean floor reached on 60 `PANEL_A` runs. A run that is won ends at the last floor. | floor |
| `student_win_rate` | The student's win rate on the same 60 runs. | percent of runs |
| `agreement_gain_from_doubling` | Held-out agreement of the student minus that of the student trained on 50 runs. | percentage points |
| `dagger_floor_change` | The DAgger student's mean floor minus the student's, on the same 60 seeds. | floors |

The DAgger student: the student plays 50 other `PANEL_B` runs, the teacher
labels every state it reaches, and a fresh network is trained on the
100-run teacher data plus those rows.

For scale, the check also prints the teacher's own win rate and mean floor
on the same 60 seeds, and the version 1 numbers are in reading item 3.

## Run

Two commands. Preparation is slow and runs once; the check is quick.

```bash
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-7-imitation/prepare.py
```

This plays the teacher on 110 runs, trains four students on the GPU (MPS),
runs the DAgger round and scores the teacher. It takes about an hour on an
idle M2 Max, longer when other work shares the machine. It writes
everything to `tour/stop-7-imitation/out/full/` (ignored by git) and skips
any step whose output exists, so a stopped run resumes where it was.

```bash
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-7-imitation/check.py
```

This refuses to start until `predictions.toml` is complete, then measures
each student and scores your predictions. It finishes within 15 minutes.

`check.py --quick` needs no preparation. It runs a tiny plan end to end in
about two minutes: datasets of 2 and 4 teacher runs, 4 evaluation runs.
Its numbers only show that the pipeline works; they say nothing about the
answer. `--workers N` sets the number of game processes (default 4).

### What the check builds, exactly

- **The student network** is the repository's own `E2ENet`
  (`ppo/e2e/net.py`) with the teacher's exact configuration: dimension 256,
  4 transformer layers, about 6 million parameters, random weights from
  seed 0. It copies one thing from the teacher: the `prior` buffer, a fixed
  table of card features that is part of the input, not a learned weight.
- **Its inputs** are the repository's own tokenized rows, made by the
  Rust tokenizer (`E2eStudent.encode`), the rows the trainer itself uses.
- **Training** is plain behaviour cloning: cross-entropy between the
  student's policy and the teacher's greedy move (hard labels), 6 passes
  over the data, AdamW, batch 256. The student never sees a reward.
- **Playing.** Every game goes through one loop over the Rust driver
  (`E2eDriver.peek`, `E2eStudent.decide`, `E2eDriver.apply`). With the
  teacher acting it plays exactly the moves `sts2_rs.e2e_selfplay` plays.
  Students are exported to `.e2ew` with the repository's
  `ppo.e2e.export_wb` and played by the same Rust forward as the teacher.
- **Agreement** is computed afterwards from stored rows: a torch forward
  of each network over the rows, comparing greedy classes.

### Substitutions, and why

- **Teacher.** The trainer's warm start used two older networks (one for
  fights, one for the run) as teachers. Here the teacher is the published
  end-to-end net, because it reads the same rows as the student and runs
  on this Mac.
- **Hard labels, not probabilities.** The trainer's teacher term is a KL
  divergence to the teacher's full probability distribution (reading item
  7). This stop trains on the teacher's chosen move only, which is the
  classic form of behaviour cloning and the form version 1 used without a
  search teacher.
- **Scale.** The trainer imitated for 80 million decisions. Here the
  largest dataset is 100 runs, tens of thousands of decisions, because it
  has to train on a laptop GPU in minutes.
- **No PPO.** The trainer moves on to PPO after the warm start. This stop
  stops before that: it measures what imitation alone produces.
- **DAgger** is not in the repository. One round is built here from the
  same pieces: the student plays, the teacher's torch network labels the
  stored rows, and a fresh student trains on the union.

## Reading the result

The check prints a table with one row per policy: `bc025`, `bc050`,
`bc100` (the student), `dagger`, and the teacher for reference. Columns:
held-out agreement, own-state agreement, win rate, mean floor and decisions
per run. Then it prints your predictions beside the measured values.

- Compare the two agreement columns. Held-out agreement is measured where
  the teacher goes. Own-state agreement is measured where the student goes.
  The gap between them is distribution shift, measured.
- Compare agreement with win rate and mean floor. Ask what per-decision
  error rate would let a policy survive several hundred decisions, if even a
  small share of errors cost a run.
- Read down the agreement column. Doubling the data helps some. Ask whether
  the curve looks like it would reach the teacher before the data got
  enormous.
- Decisions per run tells you how far the students get. A short run means
  an early death; the student never reached the later acts, so nothing in
  its play was tested there.
- The DAgger row changed two things at once: where the new labels come from,
  and how much data there is. The check does not separate them. Before you
  credit DAgger with a difference, ask how big a difference the seeds alone
  would produce: each policy plays only 60 runs.
- Everything is greedy and seeded, so a rerun prints the same table. A
  different training seed or panel would not.

## Note prompt

Answer these in your own words in your wiki note.

1. Define behaviour cloning and distribution shift, each with an example
   from this game.
2. Explain why per-decision agreement and win rate can diverge, using both
   agreement columns from your run and the length of a run.
3. What does DAgger change about where the labels come from? What did one
   round do here, and what would you need to run to know whether it was
   DAgger or just more data?
4. The trainer imitates in U0, then fades the teacher's pull to zero by
   500 million decisions (`lam_ks`). Why fade it out instead of keeping it?
   What would a permanent pull do once the student is better than the
   teacher?
5. In U0 the teacher acts with probability `beta_act`, falling from 1 to 0.
   How does that relate to DAgger?

## Going further

- Train with soft labels: store the teacher's `probs` from
  `E2eStudent.decide` and minimise the cross-entropy to them. Does
  agreement rise faster per decision of data?
- Run a second DAgger round from the DAgger student, and a control that
  adds 50 more teacher runs instead of DAgger rows. That separates "more
  data" from "the right data".
- Score agreement separately on combat and out-of-combat rows (`is_combat`
  in the row batch). K2 in the plan set different bars for each; why?
