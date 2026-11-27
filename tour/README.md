# RL tour

Chapters on the fundamental ideas of reinforcement learning. Each
chapter teaches one concept with equations, diagrams and measurements, and
uses this repository's end-to-end PPO agent ("Instinct") as its worked
example.

This directory exists only on the `tour` branch of this fork. Nothing outside
`tour/` differs from upstream.

## Chapters

Each chapter is one HTML file. Open it in a browser, for example
`open tour/stop-1-episode/chapter.html`. The page loads MathJax and its fonts
from the network, and has a light and a dark theme.

| Chapter | Concept | What it covers |
| --- | --- | --- |
| [1](stop-1-episode/chapter.html) | The decision process | MDPs, partial observability, state-dependent action sets, episodes, the objective |
| [2](stop-2-policy/chapter.html) | Policies | A policy as a distribution, the softmax over legal actions, entropy, sampling against greedy play |
| [3](stop-3-reward/chapter.html) | Reward and return | Sparse and dense reward, potential-based shaping, discounting, misspecified reward |
| [4](stop-4-critic/chapter.html) | Value functions and advantage | V, Q and A, Bellman equations, Monte Carlo against TD targets, GAE |
| [5](stop-5-ppo-update/chapter.html) | Policy gradients and PPO | The policy gradient theorem, baselines, trust regions, the clipped objective, diagnostics |
| [6](stop-6-off-policy/chapter.html) | Off-policy correction | Importance sampling, effective sample size, truncation, V-trace |
| [7](stop-7-imitation/chapter.html) | Imitation and warm starts | Behaviour cloning, compounding error, DAgger, distillation, warm-starting RL |
| [8](stop-8-evaluation/chapter.html) | Evaluating agents | Binomial intervals, paired seeds, McNemar's test, power, selection bias, pre-registration |

Read them in order. Each chapter refers back to the ones before it, and they
all share one notation.

## What a chapter's directory holds

- `chapter.html`: the chapter.
- `self-test.html`, in chapters 1 to 8: recall questions and two short
  calculations on the chapter, with each answer folded under its question.
  Answer in writing from memory before you open an answer.
- `README.md`: the commands that reproduce the chapter's measurements.
- `check.py`: the chapter's main experiment.
- `figures.py` and a JSON data file: the script that collects the data behind
  the plots, and the data it collected.
- `predictions.toml`, in chapters 2, 4, 5, 6 and 7: optional practice. Fill it
  in and `check.py` scores your guesses against its measurements. Set
  `TOUR_DEV=1` to run on the sample predictions instead.

## Setup

Do this once in the main checkout (`~/git/spire-mentor`):

```bash
brew install rustup just
cd sts2-pycli && uv sync --dev
gh release download weights-2026-11-23 --repo tfriedel/spire-mentor --dir ../weights
```

`uv sync` builds the Rust engine and takes several minutes the first time.
The weights are release assets; `just fetch-weights` needs a manifest that the
public repository does not ship.

## Running an experiment

Run from the repository root:

```bash
tour/run stop-1-episode/check.py --quick
```

`tour/run` uses the simulator's Python environment and puts `tour/` and
`sts2-pycli/python/` on the import path. In a git worktree, point it at the
main checkout:

```bash
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-1-episode/check.py --quick
```

Run a chapter's tests with `tour/run -m pytest tour/stop-1-episode`.

## Smoke test

The published Ironclad network (`ironclad_e2e_d9000_ema0999`) won 18 of the
first 20 seeds of `tourlib.PANEL_A` on an M2 Max on 2026-11-25. A fresh setup
should reproduce that.
