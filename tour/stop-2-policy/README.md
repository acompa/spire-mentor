# Stop 2: Policy as a network

## Question

What does the network output at a decision, and what does greedy play discard?

Stop 1 covered what one decision is: a moment where the run waits for the
agent, with a list of offered options. This stop is about what the network
says at that moment.

### The idea, traced on one decision

Take a made-up fight turn where the game offers four things: play Strike
(two Strikes in hand, merged into one *class* because they are identical),
play Defend, drink a potion, or end the turn. Call each of these an
**offered class**.

1. **Tokens in.** The tokenizer turns the whole situation into a set of
   *tokens*: one for the run's state, some for summaries (draw pile, deck,
   relics), one per enemy and per card in hand, and one per offered class.
   A token is a row of numbers and ids. Stop 1 traced a decision; this is
   how the network sees one.
2. **One score per class.** A shared stack of attention layers (the *trunk*)
   mixes the tokens. Then a small *policy head* reads, for each offered
   class, that class's token together with the state token, and outputs one
   number. That number is the class's **logit**: an unbounded score, higher
   means "more preferred". Four classes in, four logits out.
3. **Masked to what is offered.** The network only scores classes that exist
   at this decision. Padding slots get a logit of minus infinity, so they get
   probability zero.
4. **Softmax.** The logits become probabilities: p_i = exp(z_i) / sum_j
   exp(z_j). Only differences between logits matter. Suppose the logits
   are Strike 1.0, Defend 2.0, potion 3.5, end turn -1.0. The potion is
   exp(3.5 - 2.0) = exp(1.5), about 4.5 times as likely as Defend. The
   exponentials are 2.7, 7.4, 33.1 and 0.37, which sum to 43.6, so the
   probabilities are 0.06, 0.17, 0.76 and 0.01.
5. **Pick.** Two ways:
   - **Sampled** play draws a class at random with those probabilities. This
     is what training does: every class keeps some chance of being tried.
   - **Greedy** play takes the class with the highest logit (the *argmax*).
     This is what the shipped agent and every evaluation do. Whatever
     probability sat on the other classes is thrown away.

Two more terms:

- **Two heads.** The same trunk feeds a *combat head* (decisions inside a
  fight: card plays, potion use, in-fight choices) and an *out-of-combat
  head* (map, rewards, shops, rests, events). The decision's type picks
  which head scores it.
- **Entropy** measures how spread out the probabilities are:
  H = -sum_i p_i log p_i, in nats. One class at probability 1 gives H = 0.
  n equally likely classes give H = log n, the largest possible. Low entropy
  means the policy has made up its mind at that kind of decision.

If you reason from deep Q-learning: a logit looks like a Q-value, but it is
not an estimate of return. It is only a preference. Adding the same constant
to every logit changes nothing. A Q-learner's greedy policy and this
network's greedy policy are both "take the argmax", but this network also
defines a full probability distribution, and training learns from samples of
it.

## Read

About 60 to 90 minutes. Paths are from the repository root.

1. `sts2-pycli/python/ppo/e2e/tokens.py` lines 1-26. What one row of tokens
   holds: the state token `RUNCTX`, the summaries, and one `OPT_*` token per
   merged class out of combat (in combat, one token per legal action, with
   merged duplicates hidden).
2. `sts2-pycli/python/ppo/e2e/net.py` lines 52-70 (the module docstring).
   The trunk, the two policy heads, and the value head. Note that the policy
   path never sees the privileged tokens.
3. `docs/plans/2026-11-02-ironclad-e2e-ppo-v0111.md` lines 245-258 (section
   5). The same network as a table, with parameter counts. Look at the
   "Policy heads" row: "softmax over merged options".
4. `sts2-pycli/python/ppo/e2e/net.py` lines 1417-1448, `PolicyHead`. The
   whole head: `[h_option; h_STATE; e_kind] -> hidden -> 1`. The `-> 1` is
   one logit per class.
5. `sts2-pycli/python/ppo/e2e/net.py` lines 1751-1794. `_logits` runs both
   heads on every row and picks one with `torch.where(b.is_combat, ...)`, then
   sets the logits of absent classes to minus infinity (`masked_fill`).
   `forward` turns them into log-probabilities with `log_softmax`.
6. `sts2-pycli/python/ppo/e2e/net.py` lines 1853-1856, `act_greedy`. The
   whole of greedy play: `argmax(-1)`.
7. `sts2-rs/crates/sts2-e2e/src/net.rs` lines 600-627 and 948-971. The Rust
   port the check actually runs: `logits`, `act_greedy`, `evaluate`, then
   `softmax` (computed in f64 after subtracting the maximum) and `argmax`
   (the first maximum wins ties).
8. `sts2-rs/crates/sts2-py/src/e2e.rs` lines 122-170 (`decide`), 215-275
   (`E2ePeek.decision` and `class_action`) and 401-455 (`e2e_selfplay`).
   `decide` hands Python the logits, probabilities and greedy class of one
   decision. `e2e_selfplay` plays a whole run greedily and keeps only the
   chosen classes, which is why this check drives the run itself.
9. `sts2-pycli/python/ppo/e2e/learner.py` lines 1748-1756, inside `_serve`.
   During training the learner draws actions with `torch.multinomial` from
   the probabilities: sampled, not greedy.
10. `docs/plans/2026-11-02-ironclad-e2e-ppo-v0111.md` line 270 (the
    "Entropy" row) and line 445 ("Settings"). Training adds an entropy bonus
    per decision type and watches for entropy collapse; evaluation uses
    "greedy argmax over merged options".
11. `sts2-pycli/python/ppo/e2e/algo.py` lines 1079-1100,
    `entropy_by_type`. How training measures entropy per decision type, the
    same quantity Part 2 of the check prints.

## Predict

Fill in `predictions.toml`: a guess and an 80% interval for each. The check
plays 150 seeds of `PANEL_A` with the published Ironclad network
(`ironclad_e2e_d9000_ema0999`).

| Key | Quantity | Unit |
| --- | --- | --- |
| `median_top_p_fight` | Over every decision inside a fight in the greedy runs, the median probability of the top class. | probability, 0 to 1 |
| `median_top_p_between` | The same, for decisions between fights (map, rewards, shop, rest, events). | probability, 0 to 1 |
| `share_top_over_0_9` | Share of all greedy decisions where the top class has more than 0.9. | fraction, 0 to 1 |
| `share_sampled_differs` | In the sampled runs, the share of decisions where the sampled class is not the greedy one. | fraction, 0 to 1 |
| `winrate_sampled_minus_greedy_pp` | Sampled win rate minus greedy win rate. Negative means sampling loses more. | percentage points |
| `share_seeds_disagree` | Share of seeds where one way wins and the other loses. | fraction of seeds, 0 to 1 |

The last one is a share rather than a count so that `--quick` (20 seeds) and
the default (150) score the same quantity. The check prints the counts too.

Hints for thinking, not answers: a median hides the tail, so the median can
be high while a fair share of decisions is uncertain. A run has several
hundred decisions; a small chance of departing from greedy at each one adds
up over a run.

## Run

From the repository root (in a git worktree, add `TOUR_VENV` and
`TOUR_WEIGHTS` as in `tour/README.md`):

```bash
tour/run stop-2-policy/check.py
```

About 10 minutes on an M2 Max with 4 processes. `--quick` plays 20 seeds and
takes about 2 minutes. `--seeds N` and `--workers K` change the size and the
parallelism.

Tests (a few minutes; they play three whole runs both ways):

```bash
tour/run -m pytest tour/stop-2-policy
```

The key test checks that this stop's Python loop, played greedily, chooses
exactly the classes `sts2_rs.e2e_selfplay` chooses on three seeds. So the
greedy half of the check is the shipped agent's own play.

### What the check does

It drives each run itself through the same surface the game mod uses:
`E2eDriver.peek` says what the run is waiting for, `E2eStudent.decide` returns
the logits, `E2ePeek.class_action` names the class's action, and
`E2eDriver.apply` plays it. Forced steps (no choice) are applied as they come.

1. **Part 1** prints two decisions from the greedy run on the first seed: the
   first fight decision with at least three classes, and the first card
   reward. Each line is a class with its logit, its probability and a
   readable action.
2. **Part 2** collects the probabilities at every decision of the 150 greedy
   runs and prints the medians, the share above 0.9, and a table by decision
   type: count, mean number of classes, median top probability, share above
   0.9, mean entropy in nats, and mean entropy divided by log(n classes).
3. **Part 3** replays each seed sampling from the probabilities at
   temperature 1 (the probabilities exactly as the softmax gives them), with
   a random generator seeded by the game seed. It prints the paired win
   table and the share of sampled decisions that differ from greedy.

Substitution: Part 2 measures along greedy runs only, and Part 3 measures
"differs from greedy" along the sampled runs, whose states drift away from
the greedy runs after the first departure. Training does not sample from this
published network; it samples from the network as it was at each iteration,
and sometimes from teachers. The check uses the final network for both, so
it shows what sampling does to a finished policy, not what training saw.

## Reading the result

- **Part 1.** Compare the logits with the probabilities. A logit gap of a few
  units already makes a class almost certain; a gap near zero splits the
  mass. Find the class greedy takes and add up what the others held: that is
  what greedy play discards at that decision.
- **Part 2.** The medians say how decided a typical decision is. The share
  above 0.9 says how many decisions are not that decided. In the table, look
  for decision types whose normalised entropy stands out, and ask whether
  the game makes those choices close calls or whether training saw them too
  rarely to settle.
- **Part 3.** Read the paired table by its off-diagonal cells: seeds where
  only one way won. If both cells are small, the two ways mostly agree on
  results even though they play different actions. If one cell dominates,
  that way is better. The sampled-differs share times the decisions per run
  gives roughly how many off-greedy actions a sampled run takes.
- Win rates on a few hundred seeds have wide error bars. Stop 8 covers when a
  difference is real; for now, a difference of a few points on 150 seeds is
  within noise.

## Note prompt

Answer in your own words in your wiki note:

1. What is a logit, and how does softmax turn the logits of one decision into
   probabilities? Use one decision from Part 1 and do the arithmetic for two
   of its classes.
2. What does greedy play throw away? Use the Part 3 numbers to say how much
   that matters for winning, and why.
3. What does entropy tell you about a decision type? Name one type with low
   and one with high normalised entropy in Part 2, and give a reason for each.
4. Why does training sample while evaluation is greedy? What would go wrong in
   training if the learner always played greedily (think of how a deep
   Q-learner explores, and what the policy gradient needs)?
5. Why one trunk with two heads rather than one head or two networks?

## Going further

- Add a temperature T: divide the logits by T before the softmax. Play with
  T = 0.5 and T = 2 and see how the win rate and the differs share move.
- Run Part 2 on `allchar_e2e_d4500_ema0999.e2ew` and compare its entropy by
  type with the Ironclad net's.
- Read `sts2-pycli/python/ppo/e2e/algo.py` around line 881 for the entropy
  bonus in the loss, `β · H / log(n_classes)`, and think about what it does
  to the logits of a decision type whose entropy is collapsing.
