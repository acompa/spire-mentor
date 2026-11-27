# Chapter 5: Policy gradients and PPO

The chapter is [chapter.html](chapter.html).

Rerun its measurements from the fork's root. In a worktree, point `TOUR_VENV`
at a checkout that has a built `sts2-pycli/.venv`:

```bash
TOUR_DEV=1 TOUR_VENV=~/git/spire-mentor/sts2-pycli tour/run stop-5-ppo-update/check.py --keep /tmp/stop5
```

- `--quick` runs short iterations to check the plumbing.
- `--arms clipped_fast` runs the clipped objective at ten times the learning rate and four epochs.
- `--seed N` and `--workers N` set the learners' seed and the rollout workers per arm.

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
