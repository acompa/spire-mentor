# Chapter 2: Policies

The chapter is [`chapter.html`](chapter.html). Open it in a browser.

Commands, from the repository root (in a git worktree, prefix each with
`TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights`):

```bash
TOUR_DEV=1 tour/run stop-2-policy/check.py --quick
```

```bash
tour/run stop-2-policy/figures.py collect --seeds 120 --workers 3
```

```bash
tour/run stop-2-policy/figures.py svg
```

```bash
tour/run -m pytest tour/stop-2-policy
```

`check.py` prints two decisions with their logits, the policy's certainty by
decision type, and greedy against sampled play. Without `TOUR_DEV=1` it first
asks for predictions in `predictions.toml`; that is optional practice.
`figures.py collect` writes `figures.json`, the data behind the chapter's
plots; `figures.py svg` redraws the plots inside `chapter.html`.
