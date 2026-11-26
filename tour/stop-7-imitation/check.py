"""Stop 7: how well does a behaviour-cloned student match its teacher, and
how well does it play?

    tour/run stop-7-imitation/prepare.py     # once, about an hour
    tour/run stop-7-imitation/check.py       # measures, under 15 minutes
    tour/run stop-7-imitation/check.py --quick   # tiny plan end to end, ~2 minutes
"""

from __future__ import annotations

import argparse
import json
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import tourlib  # noqa: E402

import imitation  # noqa: E402


def table(res: dict, teacher: dict) -> None:
    print(f"\n{'policy':<8} {'held-out agree %':>17} {'early held-out %':>17} "
          f"{'own-state agree %':>18} {'win %':>7} {'mean floor':>11} {'decisions/run':>14}")
    for k, r in res.items():
        print(f"{k:<8} {r['heldout_agreement']:>17.1f} {r['heldout_early_agreement']:>17.1f} "
              f"{r['own_agreement']:>18.1f} {r['win_rate']:>7.1f} {r['mean_floor']:>11.2f} "
              f"{r['decisions_per_run']:>14.0f}")
    print(f"{'teacher':<8} {'':>17} {'':>17} {'':>18} {teacher['win_rate']:>7.1f} "
          f"{teacher['mean_floor']:>11.2f} {teacher['decisions_per_run']:>14.0f}")


def main(argv: list[str] | None = None) -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--quick", action="store_true",
                    help="prepare and measure a tiny plan (no prepare.py needed)")
    ap.add_argument("--workers", type=int, default=4, help="game processes (default 4)")
    a = ap.parse_args(argv)
    tourlib.load_predictions(HERE)  # refuse blanks before any slow work
    t0 = time.time()
    plan = imitation.QUICK if a.quick else imitation.FULL
    out = HERE / "out" / plan.name
    if a.quick:
        imitation.prepare(plan, out, a.workers)
    res = imitation.measure(plan, out, a.workers)
    table(res, json.loads((out / "teacher_eval.json").read_text()))
    print(f"\nplan {plan.name!r}: datasets of {', '.join(map(str, plan.sizes))} teacher runs; "
          f"{plan.evaluate} PANEL_A runs per policy; {time.time() - t0:.0f}s")
    tourlib.score(HERE, imitation.quantities(plan, res))


if __name__ == "__main__":
    main()
