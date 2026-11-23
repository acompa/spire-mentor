"""Plan Crystal Sphere clicks that dig up what a recorded run dug up (GH #1286).

The Crystal Sphere is a grid under fog with fifteen items beneath it. Each
divination clears one cell (small tool) or a cell and its eight neighbours
(big tool); an item is revealed when all its cells are clear, and the revealed
items become the rewards, in the order they were revealed
(CrystalSphereMinigame.CellClicked, OneOffSynchronizer.OfferCrystalSphereRewards).

A replay log records the rewards, not the clicks. The host, started with
`STS2_CRYSTAL_SPHERE_REVEAL=1`, lists the items under the fog, so the clicks
can be planned: `plans` returns click sequences that use every divination and
reveal exactly as many items of each kind as the log shows.

The order of the reveals matters, because the rewards are rolled in it: the
potions first, then gold, relics and card rewards. The log does not give it,
so `plans` returns one sequence for each order that can differ in what it
rolls; the replayer tries them in turn and keeps the one whose rewards are the
log's. The host also lists the cards a card reward gives after each count of
draws (`card_draws`); the orders that put the log's card rewards where those
say come first, which is nearly always the right one at once.
"""

from __future__ import annotations

import itertools
import json
import time
from collections import Counter
from typing import Any

Click = tuple[int, int, str]

#: Click sets searched for one choice of items, plans returned in all, and
#: seconds spent looking for them.
#: `MAX_SEEN` reveal orders are gathered before the best are kept.
MAX_COVERS = 80
MAX_PLANS = 60
MAX_SEEN = 4000

#: Plans by sphere and wanted rewards: a profile search replays one sphere many times.
_PLANS: dict[str, list[list[Click]]] = {}
MAX_SECONDS = 20.0


def item_class(kind: str) -> str:
    """The reward class of a host item kind (its texture's name)."""
    k = kind.lower()
    if "curse" in k:
        return "curse"
    if "relic" in k:
        return "relic"
    if "potion" in k:
        return "potion"
    if "card" in k:
        return "card"
    if "big_gold" in k:
        return "big_gold"
    if "gold" in k:
        return "gold"
    return k


def item_name(kind: str) -> str:
    """A host item kind with its rarity: `rare_card`, `common_potion`, `relic`."""
    k = kind.lower().removeprefix("crystal_sphere_").removesuffix("_reward")
    return k


def _roll(kind: str) -> str:
    """What an item draws as: its kind, or `gold` for gold of either size."""
    cls = item_class(kind)
    return "gold" if cls in ("gold", "big_gold") else cls if cls == "curse" else kind


def _is(kind: str, key: str) -> bool:
    """Whether a wanted `key`, a class or a class with its rarity, is this item kind."""
    return key in (item_class(kind), item_name(kind))


def _misplaced(kinds: list[str], at: dict[str, int]) -> int:
    """How many card rewards a reveal order puts at another draw than `at` says.

    Gold draws one number and a card reward six. What a potion or a relic
    draws is not known, so the count starts anew at the next placed reward.
    """
    n: int | None = 0 if not any(item_class(k) == "potion" for k in kinds) else None
    bad = 0
    for kind in kinds:
        cls = item_class(kind)
        if cls in ("gold", "big_gold"):
            n = None if n is None else n + 1
        elif cls == "card":
            want = at.get(item_name(kind))
            if want is None:
                n = None if n is None else n + 6
            else:
                bad += n is not None and n != want
                n = want + 6
        elif cls == "relic":
            n = None
    return bad


def _cleared(x: int, y: int, tool: str, w: int, h: int) -> list[tuple[int, int]]:
    """The cells a click clears, in the game's order (GetAdjacentCells, then the cell)."""
    if tool == "small":
        return [(x, y)]
    cells = [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
    cells += [(x - 1, y - 1), (x - 1, y + 1), (x + 1, y - 1), (x + 1, y + 1), (x, y)]
    return [(a, b) for a, b in cells if 0 <= a < w and 0 <= b < h]


class _Grid:
    def __init__(self, d: dict[str, Any]) -> None:
        self.w = int(d["grid"]["width"])
        self.h = int(d["grid"]["height"])
        self.hidden = frozenset((c["x"], c["y"]) for c in d["cells"] if c["hidden"])
        self.items: list[dict[str, Any]] = d["items"]
        self.cells: list[frozenset[tuple[int, int]]] = [
            frozenset(
                (it["x"] + i, it["y"] + j) for i in range(it["width"]) for j in range(it["height"])
            )
            for it in self.items
        ]
        #: Items not revealed yet: some cell is still under the fog.
        self.open = [n for n, cs in enumerate(self.cells) if cs & self.hidden]

    def play(self, clicks: list[Click]) -> list[int] | None:
        """The items `clicks` reveal, in order; None when a click hits a clear cell."""
        hidden = set(self.hidden)
        out: list[int] = []
        left = set(self.open)
        for x, y, tool in clicks:
            if (x, y) not in hidden:
                return None
            for cell in _cleared(x, y, tool, self.w, self.h):
                if cell not in hidden:
                    continue
                hidden.discard(cell)
                for n in sorted(left):
                    if cell in self.cells[n] and not (self.cells[n] & hidden):
                        out.append(n)
                        left.discard(n)
        return out

    def covers(self, want: frozenset[int], n: int) -> list[list[Click]]:
        """Click sets of at most `n` clicks that reveal the items `want` and no other."""
        found: list[list[Click]] = []
        others = [k for k in self.open if k not in want]
        target = frozenset().union(*(self.cells[k] for k in want)) if want else frozenset()

        def spoils(hidden: frozenset[tuple[int, int]]) -> bool:
            return any(not (self.cells[k] & hidden) for k in others)

        def dig(hidden: frozenset[tuple[int, int]], clicks: list[Click]) -> None:
            if len(found) >= MAX_COVERS:
                return
            todo = sorted(target & hidden)
            if not todo:
                found.append(list(clicks))
                return
            if len(clicks) >= n:
                return
            cx, cy = todo[0]
            tries: list[Click] = [
                (cx + dx, cy + dy, "big") for dx in (0, -1, 1) for dy in (0, -1, 1)
            ]
            tries.append((cx, cy, "small"))
            for x, y, tool in tries:
                if (x, y) not in hidden:
                    continue  # only a fogged cell takes a click
                after = hidden - set(_cleared(x, y, tool, self.w, self.h))
                if spoils(after):
                    continue
                clicks.append((x, y, tool))
                dig(after, clicks)
                clicks.pop()

        dig(self.hidden, [])
        return found

    def pad(self, clicks: list[Click], n: int, want: frozenset[int]) -> list[Click] | None:
        """`clicks` plus single-cell clicks that reveal nothing, up to `n` clicks."""
        hidden = set(self.hidden)
        for x, y, tool in clicks:
            hidden -= set(_cleared(x, y, tool, self.w, self.h))
        out = list(clicks)
        others = [k for k in self.open if k not in want]
        for cell in sorted(hidden):
            if len(out) >= n:
                break
            if any(self.cells[k] & hidden == {cell} for k in others):
                continue  # the last fogged cell of an item the log does not show
            hidden.discard(cell)
            out.append((cell[0], cell[1], "small"))
        return out if len(out) == n else None


def plans(
    d: dict[str, Any],
    want: Counter[str],
    order: list[str] | None = None,
    at: dict[str, int] | None = None,
) -> list[list[Click]]:
    """Click sequences for the host's `crystal_sphere` decision `d`.

    `want` counts the rewards the log shows, by `item_class` or, where the
    log's reward tells the rarity, by `item_name`. Each sequence uses every
    divination left and reveals exactly those; two sequences differ in the
    order in which they reveal the rewards that draw. `order` is
    the order the log lists those in: the sequences nearest to it come first.
    `at` gives, for a card reward the host could place (`card_draws`), how
    many draws precede it; a sequence that puts it elsewhere comes last.
    """
    key = json.dumps(
        [d["divinations_left"], d["grid"], d["cells"], d["items"], sorted(want.items()), order, at],
        sort_keys=True,
    )
    if key not in _PLANS:
        _PLANS[key] = _plans(d, want, order, at)
    return [list(clicks) for clicks in _PLANS[key]]


def _plans(
    d: dict[str, Any],
    want: Counter[str],
    order: list[str] | None,
    at: dict[str, int] | None,
) -> list[list[Click]]:
    grid = _Grid(d)
    n = int(d["divinations_left"])
    total = sum(want.values())
    picks = []
    for key, count in want.items():
        fits = [k for k in grid.open if _is(grid.items[k]["kind"], key)]
        if len(fits) < count:
            return []
        picks.append(list(itertools.combinations(fits, count)))
    out: list[tuple[int, int, list[Click]]] = []
    seen: set[tuple[tuple[str, ...], tuple[str, ...]]] = set()
    t0 = time.time()
    for combo in itertools.product(*picks):
        if time.time() - t0 > MAX_SECONDS or len(out) >= MAX_SEEN:
            break
        chosen = frozenset(k for group in combo for k in group)
        if len(chosen) < total:
            continue  # one item taken for two of the wanted rewards
        for cover in grid.covers(chosen, n):
            padded = grid.pad(cover, n, chosen)
            if padded is None:
                continue
            for clicks in itertools.permutations(padded):
                revealed = grid.play(list(clicks))
                if revealed is None or set(revealed) != chosen:
                    continue
                # A potion is drawn as its reward is made, and gold, relics
                # and cards when the screen fills, both in the order revealed
                # (OfferCrystalSphereRewards, RewardsSet.GenerateWithoutOffering).
                # A gold reward draws one number, whatever its size; gold after
                # the last relic or card changes nothing. The curse draws nothing.
                kinds = [_roll(grid.items[k]["kind"]) for k in revealed]
                first = tuple(r for r in kinds if item_class(r) == "potion")
                then = [r for r in kinds if r != "curse" and item_class(r) != "potion"]
                while then and then[-1] == "gold":
                    then.pop()
                rolls = (first, tuple(then))
                if rolls in seen:
                    continue
                seen.add(rolls)
                rolled = [r for r in kinds if r not in ("gold", "curse")]
                off = sum(
                    not _is(kind, key) for kind, key in zip(rolled, order or [], strict=False)
                )
                out.append((_misplaced(kinds, at or {}), off, list(clicks)))
                if len(out) >= MAX_SEEN:
                    break
            if len(out) >= MAX_SEEN:
                break
    out.sort(key=lambda t: t[:2])
    return [clicks for _, _, clicks in out[:MAX_PLANS]]
