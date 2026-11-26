# RL Tour 3: Reward and return

The chapter is [chapter.html](chapter.html). Open it in a browser.

```bash
export TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights
tour/run stop-3-reward/check.py --quick           # one reward ledger and the panel breakdown
tour/run stop-3-reward/figures.py collect         # replay 300 seeds per policy into chapter_data.json
tour/run stop-3-reward/figures.py render          # redraw the chapter's data figures
tour/run stop-3-reward/figures.py rescore --gamma 0.99
tour/run -m pytest tour/stop-3-reward
```
