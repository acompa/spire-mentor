# Chapter 1: The decision process

Read [`chapter.html`](chapter.html). It is the whole stop.

Commands, from the repository root:

```bash
tour/run stop-1-episode/check.py --quick --workers 3
tour/run stop-1-episode/figures.py collect --seeds 200 --workers 3
tour/run stop-1-episode/figures.py render
```

`check.py` traces one seed and summarises the panel. `figures.py collect`
writes `chapter_data.json`; `render` redraws the chapter's plots and numbers
from it.
