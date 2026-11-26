# RL tour, chapter 4: Value functions and advantage

Open [chapter.html](chapter.html) in a browser. It is the whole stop.

Run from the repository root, with the published weights and the simulator's Python environment:

```bash
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-4-critic/check.py --quick
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-4-critic/figures.py --runs 200 --workers 3
python3 tour/stop-4-critic/plots.py
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run -m pytest tour/stop-4-critic
```

`figures.py` writes `chapter_data.json`; `plots.py` redraws the chapter's figures from it. `predictions.toml` is optional: fill it in before `check.py` if you want the check to score your guesses.
