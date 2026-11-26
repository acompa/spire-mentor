# RL tour, chapter 4: Value functions and advantage

Open [chapter.html](chapter.html) in a browser to read the chapter. The scripts below reproduce its numbers and figures.

Run them from the repository root. They need the published weights and the simulator's Python environment:

```bash
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-4-critic/check.py --quick
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run stop-4-critic/figures.py --runs 200 --workers 3
python3 tour/stop-4-critic/plots.py
TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights tour/run -m pytest tour/stop-4-critic
```

`figures.py` writes `chapter_data.json`, and `plots.py` redraws the chapter's figures from that file. If you want `check.py` to score your guesses, fill in `predictions.toml` before you run it.
