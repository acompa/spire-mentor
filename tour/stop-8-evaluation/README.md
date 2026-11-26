# Stop 8: Evaluation discipline

## Question

Two networks score differently on some seeds. When is that a real
difference, and how many seeds does it take to know?

This stop is last because every later experiment in the roadmap needs it.
Each one ends in a comparison of two policies, and each one has to be paid
for in hours of play before it runs.

### Terms, with one example

- **Seed.** One number that fixes everything random in a run: the map, the
  card rewards, the enemies' rolls. Seed 170,000,000 is the same game every
  time it is played.
- **Win rate.** Wins divided by runs. 160 wins on 200 seeds is 0.80.
- **Sampling error.** Those 200 seeds are a sample from all possible seeds. A
  different 200 would give a different count, so 0.80 is an estimate with a
  margin. A **95% interval** is a range built so that 95 of 100 such ranges
  contain the true rate. The **Wilson interval** is the one this repository
  uses for a single rate; it behaves well near 0 and 1.
- **Paired seeds.** Play both networks on the *same* seeds. Then each seed
  gives a pair of results, and the four kinds of seed are: both win (a),
  only network 1 wins (b), only network 2 wins (c), neither wins (d).
- **Discordant pairs.** The b and c seeds. Seeds that both networks win, or
  both lose, say nothing about which network is better: they only say the
  seed was easy or hard. The difference in win rate is exactly (b − c) / n.
- **McNemar's exact test.** If the networks were equally good, each
  discordant seed would be a fair coin: b or c with probability one half.
  Say b = 30 and c = 14, so 44 discordant seeds. The test asks how often 44
  fair coin flips split 30 to 14 or more unevenly. That probability, doubled
  for both directions, is the p-value (`mcnemar_exact`, below).
- **Power and minimum detectable effect (MDE).** Power is the chance an
  experiment detects an effect that is really there. The MDE80 is the
  smallest true difference an experiment detects 80% of the time. It shrinks
  with the square root of the seed count, so four times the seeds halves it.

## Read

About 75 minutes. Line numbers are on the `tour` branch.

1. `sts2-pycli/python/ppo/e2e/stats.py:33-68`. `wilson`, `mcnemar_exact` and
   `newcombe_paired`. In `mcnemar_exact`, notice that a and d never appear: the
   test sees only b and c. In `newcombe_paired`, find `phi`, the correlation
   between the two networks' results; notice how a positive phi narrows the
   interval.
2. `stats.py:71-144`. `PairedTable` and `paired_bootstrap`. Notice
   `discordance` (the share of seeds that are discordant) and that the
   bootstrap resamples whole pairs, never one network's result alone.
3. `sts2-pycli/python/ppo/e2e/panels.py:26-100`. The campaign's panels as
   code. Notice `TEST` is marked `sealed`, `UNSEAL_AT` is a date before which
   nobody may read it, and `MAX_BAD_FRACTION` turns "a few errored seeds" into
   a number fixed in advance.
4. `panels.py:176-219`. `check_seeds`. Notice that a read stops on a missing,
   duplicate or off-panel seed rather than quietly dropping it.
5. `docs/experiments/2026-11-03-ironclad-e2e-v111/PREREG.md:1-87`. The
   pre-registration: line 3 (when it was written and the rule for changing
   it), then P1 to P5. For each bar in P2, ask what result would pass it,
   written before any result existed. In P3, see that the power figures
   depend on a guessed discordance. In P4, list every action the sealing
   forbids, and why "only `ls` and `wc -l`" is the rule rather than "don't
   look at the win rate".
6. Four amendments in the same file. Read each and notice the item named.
   - Amendment 2 (lines 125-133): a pre-committed rule firing. H3's read
     (line 40 defines GO) came in at +0.4pp, CI [−2.5, +3.2]. Notice the rule
     decided, not the reader's hopes.
   - Amendment 4 (lines 145-149): a recorded deviation. A bar was missed, the
     rule said "fix and relaunch", and the user waived it. Notice that the
     amendment says plainly it was made after seeing the data.
   - Amendment 6 (lines 160-169): the engine changed mid-campaign, so every
     baseline was re-run on the new engine. Notice "one binary per
     comparison", and the old sealed TEST arms that stay unread.
   - Amendment 10 (lines 209-219): the measured variant switched after one
     was seen to score higher. Notice how each old rule is re-pointed so its
     comparison stays paired on shared seeds.
7. `docs/plans/2026-11-02-ironclad-e2e-ppo-v0111.md:111-124` (§1.5). The
   bars with their power. Line 122 gives the paired SE at d = 0.25 on 4,000
   seeds; check it with sqrt(d / n) yourself.
8. Same file, `:336-353` (§9). The rule at line 338: an experiment runs only
   if it can change a decision. Then line 351, the "Moved to §13" paragraph:
   experiments were cut because their MDE80 at the affordable size was
   larger than the effect they could plausibly have.
9. Same file, `:436-458` (§11). Line 446: never compare rates across panels.
   Then `:474` (§13), where a 4,000-seed panel gives "~2pp MDE80".
10. `docs/ppo-champion-training-playbook.md:767-769`. The source of line 446:
    one network, bit-identical weights, 43.1% on one panel and 47.2% on
    another.

## Predict

Fill in `predictions.toml`: a guess and an 80% interval for each. Both
networks are published checkpoints that play the same Ironclad runs at
ascension 10: `ironclad_e2e_d9000_ema0999` (trained on Ironclad only, called
network 1) and `allchar_e2e_d4500_ema0999` (trained on all characters,
network 2). The default plays 200 seeds from `PANEL_A` and 200 from
`PANEL_B`, so 400 paired seeds.

| Key | Quantity | Unit |
| --- | --- | --- |
| `ci_half_width` | Half the width of the 95% Wilson interval on network 1's pooled win rate (400 seeds). | percentage points |
| `discordant_seeds` | How many of the 400 seeds exactly one network wins (b + c). | seeds |
| `win_rate_difference` | Network 1's win rate minus network 2's. Negative if network 2 wins more. | percentage points |
| `slice_spread` | Network 1 on four disjoint 100-seed slices (two per panel): highest win rate minus lowest. | percentage points |
| `mde_at_1000` | The MDE80 of a 1,000-seed paired comparison, at the discordance this run measures. | percentage points |
| `hours_for_4000` | Wall-clock hours to play 4,000 paired seeds (both networks) at the throughput this run measures. | hours |

Two hints, both from the Read list: one greedy run is about 700 decisions,
and ten threads play about 800 decisions per second on an idle machine.

## Run

From the repository root (in a worktree, add the `TOUR_VENV` and
`TOUR_WEIGHTS` settings from `tour/README.md`):

```bash
tour/run stop-8-evaluation/check.py
```

About 12 minutes on ten threads: 800 runs (400 seeds × 2 networks). The
check times itself, so run it with the machine otherwise idle. A busy machine
makes the cost table, and your `hours_for_4000`, wrong.

```bash
tour/run stop-8-evaluation/check.py --quick
```

About 2 minutes: 30 seeds per panel. The intervals will be wide; that is
part of the lesson.

```bash
tour/run stop-8-evaluation/check.py --cost-only
```

The cost table alone, for the roadmap: it times 20 seeds per network and
prints hours and MDE80 at 300, 1,000 and 4,000 seeds for discordance 0.10,
0.20 and 0.30. `--workers N` sets the thread count for any mode.

## Reading the result

The check prints four parts, then your scorecard.

1. **Win rates.** Each network per panel and pooled, with its Wilson
   interval. Compare a network's `A` and `B` rows: same weights, different
   seeds.
2. **Paired comparison.** The 2×2 table, the difference, the discordant
   count, and side by side the paired and the unpaired analysis of the *same*
   400 results: p-values, 95% intervals and standard errors. The unpaired
   column is what you would conclude if the two networks had played
   different seeds. The gap between the two standard errors is what pairing
   buys. A paired bootstrap (the campaign's cross-check) should roughly agree
   with the Newcombe interval.
3. **Panel variance.** Each network's win rate on four disjoint slices,
   with the spread and the binomial standard deviation of one slice. If the
   spread is several standard deviations, seed panels differ by more than
   coin-flip luck; if it is about two, luck alone explains it.
4. **Cost.** Throughput, decisions per run, and for 300, 1,000 and 4,000
   paired seeds: hours, MDE80 at several discordances (including the
   measured one), and the unpaired MDE80 for comparison. Pairing helps
   exactly when d is smaller than 2p(1 − p), the discordance two networks
   at rate p would have if their results on a seed were unrelated. Check
   which side of that line your two networks fall on.

How to read a difference: look at the interval, not just the p-value. An
interval that includes zero says this run cannot tell the networks apart.
It does not say they are equal; the interval's ends say how large a
difference is still possible.

### The cost formula and its assumptions

For n paired seeds with discordance d (share of seeds where the networks
disagree), the check uses

    MDE80 = (z_0.975 + z_0.80) × sqrt(d / n) = 2.80 × sqrt(d / n)

    hours = n × 2 networks × decisions per run / (decisions per second) / 3600

Trace one case: d = 0.25, n = 4,000. sqrt(0.25 / 4000) = 0.0079, the 0.79pp
paired SE of plan line 122. Times 2.80 is 2.2pp, the "~2pp MDE80" of line 474.

Assumptions, each worth checking before you trust a number:

- **Normal approximation.** McNemar's exact test is replaced by a normal
  one. Fine when b + c is above about 30; optimistic below.
- **d does not move with the effect.** The variance of the paired
  difference is (d − Δ²) / n; the formula drops Δ², which is small while Δ
  is a few points and d is tenths.
- **d is known.** It is the least certain input. Two close networks have a
  small d; a new recipe against an old one may have a larger one. The plan
  guessed d from earlier pairs (PREREG P3); the table shows a range.
- **Two-sided test at 5%.** The campaign's Bar1 used p < 0.01, which needs
  about 3.4 instead of 2.8 in front of the root, so 20% more effect or 45%
  more seeds.
- **Seeds are independent and like the future ones.** Each panel is a
  random draw of seeds; the panel-variance section tests how true that is.
- **Throughput is this machine, now.** Hours scale with the measured rate
  and the thread count, both printed. Decisions per run change as a policy
  improves (a winning run is longer).
- **Unpaired column:** two separate samples of n seeds each, at the average
  of the two measured rates.

### What this check substitutes

Nothing in the statistics: it calls the campaign's own `wilson`,
`mcnemar_exact`, `newcombe_paired`, `PairedTable` and `paired_bootstrap`. Two
additions are the check's own: the unpaired Newcombe interval (which is
`newcombe_paired` with phi set to 0, and a test asserts it) and the
two-proportion z-test. The panels are the tour's `PANEL_A` and `PANEL_B`, not
the campaign's DEV or TEST, and 400 seeds rather than 2,000 or 4,000, to fit
15 minutes. Both networks are greedy and play the same binary.

## Note prompt

Answer in your own words in your wiki.

1. Explain paired seeds to someone who knows A/B testing. Why do only the
   discordant seeds carry information about which network is better, and
   what did pairing do to the standard error in your run?
2. The same weights scored 43.1% and 47.2% on two panels. Using your
   panel-variance section, say whether that gap is luck or something about
   the panels, and what rule it implies for comparing numbers from
   different reads.
3. Why is a sealed test panel read once? Describe a concrete way that a
   panel read three times (to pick a checkpoint, then a variant, then to
   report) stops measuring what it claims to, even if nobody cheats.
4. What does pre-registration protect against? Use one of the four
   amendments you read: what would the campaign's conclusion look like if
   that change had been made silently?
5. You hope a change gains 2 points of win rate. Turn that into a seed
   count and a number of hours on this Mac, say which discordance you
   assumed and why, and say what you would do if the answer is
   unaffordable.

## Going further

- Rerun with `--seeds 500` (about 30 minutes) and watch the interval
  half-widths shrink. Did they shrink by the factor you expect?
- Compute the exact power of McNemar at your measured d and n = 1,000 by
  simulation (draw b + c from a binomial, then b given b + c), and compare
  it with the normal formula.
- Read P5 of the PREREG (line 87) and plan line 454: picking the best of
  nine variants on DEV makes the winner's DEV score optimistic. Estimate by
  how much, from the slice spread you measured.
