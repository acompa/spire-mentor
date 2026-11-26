# Guided tour

Eight stops through the reinforcement-learning ideas in this repository's
end-to-end PPO agent ("Instinct"). Each stop teaches one idea by having you
predict a number and then measure it.

This directory exists only on the `tour` branch of this fork. Nothing outside
`tour/` differs from upstream.

## How a stop works

1. **Read** the stop's `README.md`. It names the question, and the files and
   line ranges to read. It never states a result.
2. **Predict.** Fill in the stop's `predictions.toml`: a guess and an 80%
   interval for each quantity. The check refuses to run while any value is
   blank.
3. **Run** the check. It measures each quantity, prints it beside your
   prediction, and appends the rows to the stop's `scorecard.jsonl`.
4. **Write the note.** Each README ends with a note prompt. Answer it in your
   own words in your wiki. The stop is done when the note exists.

Agents build and run the checks. The predictions and the notes are yours.

## Stops

| Stop | Idea | Question |
| --- | --- | --- |
| [1](stop-1-episode/) | State, action, episode, return | What is one decision, and what is one episode? |
| [2](stop-2-policy/) | Policy as a network | What does the network output, and what does greedy play discard? |
| [3](stop-3-reward/) | Reward and return | Which number does training maximise? |
| [4](stop-4-critic/) | Critic and advantage | How does a result at the end of a run score one card play? |
| [5](stop-5-ppo-update/) | The PPO update | What does one update change, and what limits the change? |
| [6](stop-6-off-policy/) | Where on-policy bends | How is data from an older policy corrected? |
| [7](stop-7-imitation/) | Imitation, then RL | Why does a clone that mostly agrees with its teacher win less? |
| [8](stop-8-evaluation/) | Evaluation discipline | When is a difference between two policies real? |

Do them in order. Later stops assume the earlier notes.

## Setup

Done once in the main checkout (`~/git/spire-mentor`):

```bash
brew install rustup just
cd sts2-pycli && uv sync --dev
gh release download weights-2026-11-23 --repo tfriedel/spire-mentor --dir ../weights
```

`uv sync` builds the Rust engine and takes several minutes the first time.
The weights are release assets; `just fetch-weights` needs a manifest that the
public repository does not ship.

## Running a check

```bash
tour/run stop-1-episode/check.py
```

`tour/run` uses the simulator's Python environment and puts `tour/` and
`sts2-pycli/python/` on the import path. In a git worktree, point it at the
main checkout:

```bash
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-1-episode/check.py
```

## Conventions for every stop

- **Files.** `README.md`, `predictions.toml` (blank), `predictions.sample.toml`
  (filled, for dry runs), `check.py`, and `test_check.py` for any logic worth
  a test.
- **Prediction gate.** `check.py` calls `tourlib.load_predictions` before any
  slow work and `tourlib.score` at the end. `TOUR_DEV=1` runs on the sample
  predictions and marks the scores as a dry run.
- **No spoilers.** READMEs, sample predictions and committed files state no
  measured result. Sample guesses are deliberately rough.
- **Seeds.** Use `tourlib.PANEL_A` and `tourlib.PANEL_B`. They overlap none of
  the campaign's panels.
- **Time.** The default check finishes in 15 minutes or less on an M2 Max. A
  `--quick` flag finishes in about two. One greedy run takes about 6 s on one
  thread, and `tourlib.selfplay_many` plays about 800 decisions per second on
  ten threads.
- **Upstream code first.** A check calls the repository's own functions where
  they exist, so it measures the real mechanism. Where a check must substitute
  something smaller, the README says what and why.
- **Scope.** A stop changes files only inside its own directory.

## Measured on 2026-11-25

The published Ironclad network (`ironclad_e2e_d9000_ema0999`) won 18 of the
first 20 seeds of `PANEL_A` on an M2 Max. This is the tour's smoke test, not
a stop's answer.
