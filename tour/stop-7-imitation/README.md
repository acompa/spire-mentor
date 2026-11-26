# Stop 7: Imitation and warm starts

The chapter is [`chapter.html`](chapter.html). Open it in a browser.

Run these commands from the repository root. `TOUR_VENV` points at a checkout
that has the simulator's Python environment, and `TOUR_WEIGHTS` points at the
published weights.

```bash
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-7-imitation/prepare.py --workers 3
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-7-imitation/figures.py measure
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-7-imitation/figures.py render
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run -m pytest -q tour/stop-7-imitation
```

`prepare.py` builds the datasets and students in `out/full/`, which git
ignores. `figures.py measure` plays and scores them and writes `figures.json`.
`figures.py render` draws the figures and fills the measured numbers into the
chapter.

To score your own guesses, copy `predictions.sample.toml` to
`predictions.toml`, fill it in, and run `check.py --workers 3`.
`check.py --quick` runs a tiny plan end to end without preparation.
