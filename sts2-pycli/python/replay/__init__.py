"""sts2-pycli replay harness — bead `sts2-rs-p0jzo` (W7).

Per-floor verification helpers for driving the Rust ``RunManager``
engine through scraped ``.run`` files. (The corpus driver that shipped,
``act1_replay.py``, does not use this package.)

Modules:

- :mod:`scraped` — parse ``sts2_run_scraper/data/files/*.run`` into a
  typed ``Run`` / ``Floor`` view (one floor per ``map_point_history``
  entry).
- :mod:`checkpoints` — per-floor assertion helpers; produces a typed
  ``Divergence`` list (don't fail-fast — log + continue) so a driver
  can produce a full per-run diagnostic.
"""

from .checkpoints import Divergence, DivergenceKind, check_floor
from .scraped import Floor, Run, parse_run
