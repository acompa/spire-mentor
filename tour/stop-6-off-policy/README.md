# Chapter 6: Off-policy correction

Read [chapter.html](chapter.html).

Regenerate the measurements, figures and numbers in the chapter:

```bash
tour/run stop-6-off-policy/figures.py play --out /tmp/runs48.npz --runs 48 --workers 3
tour/run stop-6-off-policy/figures.py data --runs-file /tmp/runs48.npz
tour/run stop-6-off-policy/figures.py svg
```

Optional: score your own predictions of seven of the chapter's quantities
(fill in `predictions.toml` first), and run the arithmetic tests:

```bash
tour/run stop-6-off-policy/check.py --quick
tour/run -m pytest tour/stop-6-off-policy
```

From a worktree, prefix each command with
`TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights`.
