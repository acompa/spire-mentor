"""Build the students this stop measures: teacher data, one behaviour-cloned
student per dataset size, and one DAgger round. Caches everything under
`out/<plan>/`; rerunning skips what exists.

    tour/run stop-7-imitation/prepare.py             # about an hour
    tour/run stop-7-imitation/prepare.py --quick     # the --quick check's plan
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import imitation  # noqa: E402


def main(argv: list[str] | None = None) -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--quick", action="store_true", help="the small plan --quick uses")
    ap.add_argument("--workers", type=int, default=4, help="game processes (default 4)")
    a = ap.parse_args(argv)
    plan = imitation.QUICK if a.quick else imitation.FULL
    imitation.prepare(plan, HERE / "out" / plan.name, a.workers)


if __name__ == "__main__":
    main()
