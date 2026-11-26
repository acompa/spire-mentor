# Chapter 5: Policy gradients and PPO

The chapter is [chapter.html](chapter.html).

Rerun its measurements from the fork's root (a worktree needs `TOUR_VENV`
pointing at a checkout with a built `sts2-pycli/.venv`):

```bash
TOUR_DEV=1 TOUR_VENV=~/git/spire-mentor/sts2-pycli tour/run stop-5-ppo-update/check.py --keep /tmp/stop5
```

- `--quick`: short iterations; checks the plumbing.
- `--arms clipped_fast`: the clipped objective at ten times the learning rate and four epochs.
- `--seed N`, `--workers N`: the learners' seed and rollout workers per arm.

Redraw the figures from the kept runs:

```bash
python3 tour/stop-5-ppo-update/figures.py extract /tmp/stop5
```

```bash
python3 tour/stop-5-ppo-update/figures.py render
```

`TOUR_DEV=1` scores the run against `predictions.sample.toml`. Without it,
`check.py` scores your own guesses in `predictions.toml` and refuses to start
until they are filled in.
