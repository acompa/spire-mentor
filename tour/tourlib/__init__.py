"""Shared pieces of the guided tour: where things are, which seeds to play, and
the prediction gate every stop's check goes through."""

from __future__ import annotations

import json
import math
import os
import tomllib
from dataclasses import asdict, dataclass
from pathlib import Path

TOUR = Path(__file__).resolve().parent.parent
REPO = TOUR.parent

#: Seed ranges for tour checks. They overlap none of the campaign's panels
#: (`ppo.e2e.panels`: DEV 160,000,000+, TEST 161,000,000+, SMOKE 162,000,000+),
#: so no check here reads a seed the published networks were selected on.
PANEL_A = range(170_000_000, 170_001_000)
PANEL_B = range(171_000_000, 171_001_000)

#: Set to 1 to run a check on the committed sample predictions. For developing
#: a stop only: it marks every score as a dry run.
DEV_ENV = "TOUR_DEV"


def weights_dir() -> Path:
    """Where the published weights are. A worktree has none of its own, so
    `TOUR_WEIGHTS` points it at the checkout that fetched them."""
    return Path(os.environ.get("TOUR_WEIGHTS") or REPO / "weights")


def weight(name: str) -> Path:
    """One published file, e.g. `ironclad_e2e_d9000_ema0999.e2ew`."""
    path = weights_dir() / name
    if not path.is_file():
        raise SystemExit(f"missing {path}: see tour/README.md, 'Setup'")
    return path


@dataclass(frozen=True)
class Prediction:
    key: str
    question: str
    unit: str
    guess: float
    low: float
    high: float


@dataclass(frozen=True)
class Scored:
    key: str
    unit: str
    guess: float
    low: float
    high: float
    actual: float
    inside: bool
    dry_run: bool


def _number(value: object) -> float | None:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        return None
    return float(value) if math.isfinite(value) else None


def load_predictions(stop_dir: Path) -> tuple[dict[str, Prediction], bool]:
    """The stop's predictions and whether this is a dry run.

    Reads `predictions.toml`. Exits, naming every blank, unless each entry has
    a numeric `guess`, `low` and `high` with low <= guess <= high. With
    `TOUR_DEV=1` it reads `predictions.sample.toml` instead.
    """
    dry = os.environ.get(DEV_ENV) == "1"
    path = stop_dir / ("predictions.sample.toml" if dry else "predictions.toml")
    raw = tomllib.loads(path.read_text(encoding="utf-8"))
    out: dict[str, Prediction] = {}
    problems: list[str] = []
    for key, entry in raw.items():
        vals = {f: _number(entry.get(f)) for f in ("guess", "low", "high")}
        blank = [f for f, v in vals.items() if v is None]
        if blank:
            problems.append(f"  {key}: fill in {', '.join(blank)}")
            continue
        if not vals["low"] <= vals["guess"] <= vals["high"]:
            problems.append(f"  {key}: need low <= guess <= high")
            continue
        out[key] = Prediction(key, entry["question"], entry.get("unit", ""), **vals)
    if problems:
        raise SystemExit(
            f"{path.name} is not complete. Write your predictions before running the check:\n"
            + "\n".join(problems)
        )
    return out, dry


def score(stop_dir: Path, actual: dict[str, float]) -> list[Scored]:
    """Print each prediction beside what the check measured, and append the
    rows to the stop's `scorecard.jsonl`. `actual` must cover every prediction."""
    preds, dry = load_predictions(stop_dir)
    missing = sorted(set(preds) - set(actual))
    if missing:
        raise SystemExit(f"the check measured nothing for: {', '.join(missing)}")
    rows = [
        Scored(p.key, p.unit, p.guess, p.low, p.high, float(actual[k]),
               p.low <= actual[k] <= p.high, dry)
        for k, p in preds.items()
    ]
    width = max(len(r.key) for r in rows)
    print(f"\n{'prediction':<{width}}  {'guess':>10}  {'80% interval':>23}  {'measured':>10}  inside")
    for r in rows:
        interval = f"{r.low:g} to {r.high:g}"
        print(f"{r.key:<{width}}  {r.guess:>10g}  {interval:>23}  {r.actual:>10.4g}  "
              f"{'yes' if r.inside else 'NO'}  {r.unit}")
    hits = sum(r.inside for r in rows)
    print(f"\n{hits} of {len(rows)} measured values fell inside your interval."
          + ("  (dry run on sample predictions)" if dry else ""))
    with (stop_dir / "scorecard.jsonl").open("a", encoding="utf-8") as fh:
        for r in rows:
            fh.write(json.dumps(asdict(r)) + "\n")
    return rows


def selfplay_many(student, seeds, workers: int = 10) -> list[dict]:
    """`sts2_rs.e2e_selfplay` (greedy, whole run) over `seeds`, in seed order.

    The call releases the GIL, so threads give real parallelism. One thread
    plays about 110 decisions per second on an M2 Max, which is about 6 s per run.
    """
    from concurrent.futures import ThreadPoolExecutor

    import sts2_rs

    with ThreadPoolExecutor(max_workers=workers) as pool:
        return list(pool.map(lambda seed: sts2_rs.e2e_selfplay(student, seed), seeds))
