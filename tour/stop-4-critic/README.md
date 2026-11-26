# Stop 4: critic and advantage

## Question

A run of Slay the Spire 2 is about 700 decisions long. The only large reward
comes at the very end: +1 if the run is won. How does that one number, arriving
hundreds of decisions later, tell the agent whether *this* card play, or
*this* choice to fight an elite, was a good one?

### A first example: the elite fork

On the map you can walk through an elite room or around it. An elite fight
pays no reward. It costs HP. Its payoff, a relic and a better card, shows up
only later, as a higher chance of winning the run. If the agent waited for the
final +1 to score the choice, every one of the next 500 decisions would get
the same credit and the same blame, and the elite's effect would drown in that
noise.

The fix is a second opinion about the future, produced at every step. Call it
the **value** V of a state: the network's estimate of how much reward is still
to come from here, if the agent keeps playing as it does now. The part of the
network that produces it is the **critic**. (The part that picks moves is the
**policy**, or actor.)

Now trace the elite. Say the critic says V = 0.90 just before the elite and
V = 0.93 just after it is won, and the fight paid 0.002 of floor reward. The
fight looks good by 0.002 + 0.93 − 0.90 = +0.032. The final +1 did not have to
arrive for that judgement. That difference is a **one-step surprise**, written
δ ("delta"):

    δ_t = r_t + V(next state) − V(this state)

r_t is the reward collected between the two states. A positive δ means things
went better than the critic expected; negative, worse.

### From surprise to advantage

The **advantage** A_t of the action taken at step t is how much better that
action turned out than the critic expected. One surprise is a noisy answer,
because the critic is often wrong. Adding up all later rewards (the
Monte Carlo return) is an unbiased answer, but it is noisy for the opposite
reason: it carries the luck of every later decision. **Generalised advantage
estimation (GAE)** blends the two. It adds the later surprises, each shrunk by
a factor λ ("lambda") per step:

    A_t = δ_t + λ·δ_(t+1) + λ²·δ_(t+2) + …   =   δ_t + λ·A_(t+1)

With λ = 0 the advantage is the one-step surprise alone. With λ = 1 it is the
full return minus V(t). This repository uses no discounting (γ = 1), so λ is
the only thing that shrinks later surprises.

If you think in Q-learning terms: δ is the TD error of a state-value function
evaluated on the policy's own data. A is a sample of Q(s, a) − V(s). GAE is
TD(λ) applied to the advantage instead of to the value target.

### How this repository chains fights into the run

There are two value heads, read from the same network: **V_combat** at combat
decisions and **V_run** at decisions between fights (map, shop, card reward;
"run rows"). Advantages run along two chains:

- **Combat chain**, λ_c = 0.95. Inside a fight, row by row. At the fight's
  last decision the next state is the next run row, so the fight is scored
  against V_run after it: what the fight did to the whole run.
- **Run chain**, λ_r = 0.97. Over run rows only. It steps over each fight in
  one jump: δ = (all reward collected during the fight) + V_run(after) −
  V_run(before).

The elite fork lives in the run chain. The reward across an elite is close to
zero, so the only place its worth can show is in V_run before and after.

### The critic sees more than the policy

The value module also reads **privileged** (PRIV) tokens the policy never
sees: the true order of the draw pile, the enemies' true next moves, and this
act's generated rooms and bosses. The critic is used only while training, to
compute advantages. When the agent plays, it uses the policy alone, so these
inputs are never needed at play time.

## Read

About 75 minutes. Line numbers are on this branch.

1. `docs/plans/2026-11-02-ironclad-e2e-ppo-v0111.md` lines 267-269 (the
   Returns, Values and Advantages rows of section 6) and line 276 (the reward).
   Find λ_c, λ_r and γ, and note the reward is only win, boss and floor: no HP
   term.
2. `sts2-pycli/python/ppo/e2e/algo.py` lines 22-40. The two chains in words.
   Find the sentence that says a fight hands its value over to the next run
   row.
3. `algo.py` line 155. The two λs.
4. `algo.py` lines 304-463, `compute_returns`. Skip the sorting and validation in 355-399.
   Lines 401-407 are the combat chain: `delta[cn] = w_[cn] + vnext[cn] - v_[cn]`
   is δ, and `nxt` continues the chain only into a combat row. Lines 409-426
   are the run chain: `big_r` is the reward summed over the fight, `vend` the
   next run row's value. Line 441 turns advantages into value targets: target = A + V.
5. `algo.py` lines 230-248, `_scan`. The recursion A = δ + λ·A(next) computed
   without a Python loop. You need only its docstring.
6. `algo.py` lines 465-472, `explained_variance`. The statistic the check uses
   to say how much of the return the critic predicts.
7. `sts2-pycli/tests/e2e/test_algo.py` lines 39-79. `ref_gae` and
   `ref_two_chain` are the same arithmetic as plain loops; compare them with
   the vectorised code in item 4.
8. `test_algo.py` lines 407-473, the tests the plan calls T1. Read the
   docstring of each and the identity they check: cutting a chain early and
   bootstrapping from the value at the cut changes the advantage by exactly
   λ^(steps) × (what the cut threw away).
9. `test_algo.py` lines 578-616, T2. Act 3 ends in two boss fights in a row.
   Find where boss 1's advantage continues into boss 2, and the λ = 1 case
   where every target equals the total reward still to come.
10. `sts2-pycli/python/ppo/e2e/net.py` lines 64-70. The value module and the
    rule that PRIV tokens never reach the policy.
11. `net.py` lines 1451-1481 (`ValueModule.forward`) and 1781-1794
    (`E2ENet.forward`). Find where the PRIV tokens are concatenated, where the
    two heads read token 0, and how `value` picks V_combat or V_run.
12. `sts2-pycli/python/ppo/e2e/contracts.py` lines 285, 312 and 313. What each
    PRIV section holds.
13. `sts2-pycli/python/ppo/e2e/rsenv.py` lines 226-233. The training env is
    built with `priv_=True`; this check builds it the same way.
14. This stop's `critic.py`. `trace_run` plays a run and reads both values;
    `hand_two_chain` is the two chains as loops.

## Predict

Fill in `predictions.toml`: a guess and an 80% interval for each. Values and
rewards are in reward units: a won run collects about 1.24 in total (1.0 for
the win, 0.05 for each of three bosses, 0.002 per floor).

| Key | Quantity | Unit |
| --- | --- | --- |
| `start_value` | The critic's value at the first decision of a run, averaged over the runs played | reward units |
| `boss_won_dv` | Across a won act 1 or act 2 boss fight: V_run at the run row after it minus V_run at the run row before it (the median over fights) | reward units |
| `elite_minus_ordinary_dv` | The same change in V_run, averaged over elite fights, minus its average over ordinary (monster) fights | reward units |
| `win_rate_at_half` | Among decisions where the critic's value is between 0.4 and 0.6, the share that belong to runs that were won | fraction |
| `weight_20_later` | With λ = 0.95, the weight that a surprise 20 decisions later gets in this decision's advantage | fraction |
| `explained_in_fights` | Inside fights, the share of the variance of the return still to come that the critic's value explains (1 is perfect, 0 is no better than a constant) | fraction |

Two things to think about first. A boss fight pays a 0.05 bonus: does V_run
after the boss still count that bonus? And the return still to come includes
everything after the fight, so ask how much of that a combat value could know.

## Run

From your worktree (about 10 minutes; `--quick` plays 40 runs instead of 300,
about 1-2 minutes):

```bash
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-4-critic/check.py
```

The tests, including the hand-versus-repository comparison on a real run:

```bash
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run -m pytest tour/stop-4-critic
```

What the check does:

- Plays the first 300 seeds of `PANEL_A` with the Ironclad checkpoint
  `ironclad_e2e_d9000_ema0999.pt` on 4 processes. Each run is played on the
  Rust training env with privileged tokens on, as the rollout workers do. At
  every decision it runs `E2ENet.forward`, takes the greedy move from the
  policy logits, and records V_combat, V_run, the room type and HP. Greedy
  play from the `.pt` makes the same moves as the published `.e2ew` network.
- Prints the value trace of the first run, one line per fight or stretch of
  run decisions; elites and bosses are marked `**`.
- Groups every decision by its value and prints the return that actually
  followed it.
- Prints the last eight decisions of the first run's first elite: r, V, δ,
  and A computed by hand beside A from `compute_returns`. Then the run chain's
  single step over that fight, and the largest hand-versus-repository gap over
  the whole run.
- Prints V_run before and after every fight by room type and by HP at the
  fight's start.
- Reads the weight of a later surprise off `compute_returns` itself, by
  feeding it a chain with a single unit of reward.

## Reading the result

- **The trace.** V moves little inside an ordinary fight and jumps at the
  run row after it. Look at where the big moves are. Are they inside fights,
  at fight ends, or on the map? A loss shows as V falling toward 0 during the
  fatal fight.
- **Calibration.** If the critic is calibrated, the "mean G" column tracks
  the "mean V" column in each band. Where they part, the critic is over- or
  under-confident. The "won" column is not the same thing as V: V counts the
  boss and floor rewards too, and the critic sees hidden information you do
  not.
- **GAE by hand.** "A hand" and "A repo" should agree to rounding. Note how
  small each δ is inside a fight, and what decides the advantage of the
  fight's last decision.
- **The elite fork.** "mean dV" is the change in value; "mean delta" adds the
  reward collected, which makes it the run chain's surprise. The two columns
  differ most for bosses: why? Compare elites with ordinary fights, then
  compare elites at low HP with elites at high HP. A negative mean δ for a
  room type says the critic, on average, expected those fights to go better
  than they did.
- **Explained variance.** Low numbers inside fights are not necessarily a
  bad critic. The target is the return to the end of the run, much of which
  depends on fights that have not been generated yet.

## Note prompt

Answer these in your own words in your wiki note.

1. Define value, advantage and GAE without symbols first, then with them.
   Use the elite fork as your example.
2. What does λ trade off? Say what goes wrong at λ = 0 and at λ = 1 in a game
   where most of the reward comes at the end of ~700 decisions, and relate the
   weight you measured for a surprise 20 decisions later to the length of a
   fight.
3. Why may the critic see the draw order and the upcoming rooms when the
   policy may not? What would break if the policy saw them? What would the
   critic lose if it did not?
4. Re-answer the elite question with what you measured: how does the agent
   learn whether an elite was worth fighting, and what did the before-and-after
   values say about elites against ordinary fights, and about HP?
5. Your calibration table: where is the critic right, and where would its
   errors push the policy's advantages in a consistent direction?

## Going further

- Turn the privileged tokens off. In `critic.trace_run`, pass a `row_batch`
  whose PRIV fields are emptied (or zero `e_priv` in a copy of
  `E2ENet.forward`) and compare V_combat inside fights. The policy's moves
  cannot change (test T14), but the critic's view should. How much of the
  explained variance came from information the policy never has?
- Rerun `gae_by_hand` with `lam_c=0` and `lam_c=1` in `compute_returns` and
  see how the advantages of the fight's early decisions change.
- Run the check on the `allchar_e2e_d4500_ema0999.pt` checkpoint (change
  `CKPT` in `check.py`) and compare calibration. Its runs are not all
  Ironclad, so `STS2_E2E_CHARACTER` and the "chars" fill may need setting.
