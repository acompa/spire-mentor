# Stop 7: Imitation and warm starts

The chapter is [`chapter.html`](chapter.html). Open it in a browser.

Commands, from the repository root (the first prefix points at a checkout with
the simulator's Python environment and the published weights):

```bash
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-7-imitation/prepare.py --workers 3
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-7-imitation/figures.py measure
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-7-imitation/figures.py render
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run -m pytest -q tour/stop-7-imitation
```

`prepare.py` builds the datasets and students in `out/full/` (ignored by git).
`figures.py measure` plays and scores them and writes `figures.json`;
`render` draws the figures and fills the measured numbers into the chapter.

Optional: `check.py --workers 3` scores your own guesses in `predictions.toml`
(copy `predictions.sample.toml`) against the measurements. `check.py --quick`
runs a tiny plan end to end without preparation.
