# Chapter 8: Evaluating agents

Read [`chapter.html`](chapter.html): win rates as binomial estimates, paired
seeds, McNemar's test, power and cost, selection bias, and pre-registration,
with measurements from this Mac.

Commands, from the repository root (in a worktree, prefix them with
`TOUR_VENV=~/git/spire-mentor/sts2-pycli TOUR_WEIGHTS=~/git/spire-mentor/weights`):

```bash
tour/run stop-8-evaluation/check.py
```

```bash
tour/run stop-8-evaluation/check.py --quick
```

```bash
tour/run stop-8-evaluation/check.py --cost-only
```

```bash
tour/run stop-8-evaluation/check.py --seeds 300 --workers 3 --save tour/stop-8-evaluation/data/runs.json
```

```bash
tour/run stop-8-evaluation/figures.py
```

The fourth command regenerates the chapter's data; the fifth rewrites
`data/summary.json` and the chapter's figures and measured numbers from it.
