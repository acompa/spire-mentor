"""Find the player's profile a recorded run was played on (GH #1286).

A run depends on the player's profile: which timeline epochs are revealed
(card, relic, potion and event pools, the ancients, Underdocks), which bosses
they have met, and whether it is their first run. The log records none of
that, and the host starts with everything unlocked, so a run of a player who
has not unlocked everything parts from the host within a few rooms.

`infer` searches for the profile under which the host follows the log. It
does not read the answer off the log: every candidate is replayed, and the one
the host follows furthest wins. What narrows the search:

- The header's `up_front` counter. The relic bags, the ancients and each
  act's rooms are drawn off that stream at the start, so the counter depends
  on which relic, event and ancient epochs are locked. The host's
  `probe_profiles` runs those draws for every set of such epochs in one
  process; only the sets that end on the header's counter with the log's
  ancients and events are replayed (`Search.g1_sets`), and `Replayer.start`
  refuses a profile whose counter differs.
- Content the log shows is unlocked (`_seen_epochs`).
- `Replayer.start` settles what the log states outright: Neow, the act-1
  variant and each act's boss.

The rest is a coordinate search over small families of epochs, by how many
rows of the log the host agrees with.
"""

from __future__ import annotations

import itertools
import json
import re
import sys
import time
from collections.abc import Iterator
from typing import Any

from host_replay import Replayer, _norm, load
from sts2_client import Game

#: Replays tried for one run before the search gives up.
MAX_TRIALS = 400

#: Seconds spent on one run before the search gives up.
MAX_SECONDS = 1500

#: Sets of start-of-run epochs replayed, of those the probe keeps.
MAX_G1 = 48

#: Crystal Sphere click plans a trial tries (`Replayer.sphere_cap`): each is a
#: replay from the start.
SPHERE_CAP = 3

#: Joint states of the other characters' epochs replayed (`Search.foreign_sets`).
MAX_FOREIGN = 160

CHARACTERS = ("IRONCLAD", "SILENT", "REGENT", "NECROBINDER", "DEFECT")

#: The characters after the first, in the order the timeline unlocks them.
UNLOCK_ORDER = ("SILENT", "REGENT", "NECROBINDER", "DEFECT")

#: Stops another profile can explain: the host offers or meets other content.
#: At any other stop (a pick, a fight, a log that cannot be replayed) the
#: search ends.
PROFILE_STOPS = frozenset(
    {
        "rng_start",
        "ancient",
        "boss",
        "act",
        "encounter",
        "event_options",
        "card_reward_offer",
        "rewards_offer",
        "bundle_options",
        "shop_stock",
        "card_select",
        "card_select_options",
        "floor_state",
        "potion",
        "host_rejected",
        "map_options",
        # Cards a card or a power made in a fight come from the same pools.
        "pick",
        "hand",
    }
)

_EPOCHS: dict[str, Any] = {}
_CARD_POOL: dict[str, str] = {}


def _epochs() -> dict[str, dict[str, list[str]]]:
    if not _EPOCHS:
        g = Game()
        try:
            res = g.epochs()
        finally:
            g.close()
        _EPOCHS.update(res["epochs"])
        for pool, cards in (res.get("card_pools") or {}).items():
            _CARD_POOL.update(dict.fromkeys(cards, pool))
    return _EPOCHS


def _pools(detail: str) -> set[str]:
    """Whose cards a stop's detail names: characters, and COLORLESS."""
    _epochs()
    return {_CARD_POOL[x] for x in re.findall(r"[A-Z][A-Z0-9_]+", detail) if x in _CARD_POOL}


def _g1(character: str) -> list[str]:
    """The epochs that change the start-of-run draws: relics, events, ancients."""
    c = character.upper()
    return [
        *(f"RELIC{i}_EPOCH" for i in range(1, 6)),
        f"{c}3_EPOCH",
        f"{c}6_EPOCH",
        *(f"EVENT{i}_EPOCH" for i in range(1, 4)),
        "DARV_EPOCH",
        "OROBAS_EPOCH",
    ]


def _log_ids(rows: list[dict[str, Any]]) -> set[str]:
    """Every id-like string in the log."""
    ids: set[str] = set()

    def walk(v: Any) -> None:
        if isinstance(v, dict):
            for x in v.values():
                walk(x)
        elif isinstance(v, list):
            for x in v:
                walk(x)
        elif isinstance(v, str) and len(v) < 120:
            ids.add(_norm(v))
            ids.add(v.rsplit(".", 1)[-1])

    for r in rows:
        walk(r)
    return ids


def _seen_epochs(rows: list[dict[str, Any]]) -> set[str]:
    """Epochs whose content the log shows: they are revealed."""
    ids = _log_ids(rows)
    seen = {
        epoch
        for epoch, content in _epochs().items()
        if any(i in ids for items in content.values() for i in items)
    }
    # A character's card in the log: the character is unlocked, and so is
    # every one before it (each character's first epoch is won as the one
    # before: Neow, Silent1, Regent1 and Necrobinder1 `GetTimelineExpansion`).
    last = max(
        (
            UNLOCK_ORDER.index(c)
            for c in {str(rows[0]["character"]).upper(), *(_CARD_POOL.get(i) for i in ids)}
            if c in UNLOCK_ORDER
        ),
        default=-1,
    )
    return seen | {f"{c}1_EPOCH" for c in UNLOCK_ORDER[: last + 1]}


def _in_unlock_order(locked: frozenset[str]) -> bool:
    """Whether no character is unlocked behind a locked one."""
    shut = [f"{c}1_EPOCH" in locked for c in UNLOCK_ORDER]
    return shut == sorted(shut)


#: Stops at the start of the run, before the first decision.
START_STOPS = frozenset({"act", "host_start", "rng_start", "ancient", "boss"})


def _drift(r: dict[str, Any]) -> list[str]:
    """The pool-fed RNG streams a replay found off the log's count at a fight's start.

    A potion or a card that makes cards draws until it has distinct ones, so
    the count depends on the pool's size. The log names only the card picked,
    and a pool of another size can offer that one too: the count is then the
    only trace of the wrong pool, until a shuffle or a thief draws on it.
    """
    out = []
    for stream in ("combat_card_generation", "combat_potion_generation"):
        if any(
            n.startswith("rng@") and not n.startswith("rng@start") and stream in n
            for n in r.get("notes") or []
        ):
            out.append(stream)
    return out


def _score(r: dict[str, Any]) -> list[int]:
    """How far a replay got: a run that does not start is behind one that does."""
    if r.get("desync") in START_STOPS:
        return [0, 0, 0, 0]
    # Stopping at the same place with less amiss is nearer (`Desync.size`).
    return [*r["progress"], -r.get("mismatch", 0), -len(_drift(r))]


def _subsets(epochs: list[str]) -> Iterator[frozenset[str]]:
    for n in range(len(epochs) + 1):
        for combo in itertools.combinations(epochs, n):
            yield frozenset(combo)


class Search:
    def __init__(self, path: str, *, verbose: bool = False) -> None:
        self.path = path
        self.verbose = verbose
        self.rows = load(path)
        self.character = str(self.rows[0]["character"]).upper()
        self.trials = 0
        self.spheres: dict[str, int] = {}
        self.cache: dict[str, dict[str, Any]] = {}

    def log(self, *a: Any) -> None:
        if self.verbose:
            print(*a, file=sys.stderr, flush=True)

    def replay(
        self, locked: frozenset[str], first_run: bool, *, sphere_cap: int | None = SPHERE_CAP
    ) -> dict[str, Any]:
        profile: dict[str, Any] = {"locked_epochs": sorted(locked)}
        if first_run:
            profile["number_of_runs"] = 0
            if locked >= set(_epochs()) - {"NEOW_EPOCH", "UNDERDOCKS_EPOCH"}:
                # A save with nothing unlocked at all: the game then hands
                # out its tutorial rewards (RewardsSet.TryGenerateTutorialRewards
                # asks for no unlocked epoch), so Underdocks is locked as well
                # even where the act roll does not show it.
                profile["locked_epochs"] = sorted(locked | {"UNDERDOCKS_EPOCH"})
        key = json.dumps([profile, sphere_cap, self.spheres], sort_keys=True)
        if key not in self.cache:
            self.trials += 1
            # The click plans the best replay so far got through its Crystal
            # Spheres with are where this one starts.
            self.cache[key] = Replayer(
                self.path,
                profile=profile,
                sphere_cap=sphere_cap,
                sphere_attempts={int(k): v for k, v in self.spheres.items()},
            ).run()
            r = self.cache[key]
            self.log(
                f"  trial {self.trials}: progress {r['progress']} "
                f"{r.get('desync') or r.get('end')} locked {sorted(locked)}"
                + (" first-run" if first_run else "")
            )
        return self.cache[key]

    def families(self) -> list[list[str]]:
        c = self.character
        others = [x for x in ("SILENT", "REGENT", "NECROBINDER", "DEFECT") if x != c]
        return [
            [f"{c}2_EPOCH", f"{c}5_EPOCH", f"{c}7_EPOCH"],
            ["POTION1_EPOCH", "POTION2_EPOCH", f"{c}4_EPOCH"],
            [f"{o}1_EPOCH" for o in others],
            [f"COLORLESS{i}_EPOCH" for i in range(1, 6)],
            # The other characters' cards (Kaleidoscope, Prismatic Gem, Splash).
            *(
                [f"{o}2_EPOCH", f"{o}5_EPOCH", f"{o}7_EPOCH"]
                for o in ("IRONCLAD", *others)
                if o != c
            ),
        ]

    def foreign_sets(self, revealed: set[str]) -> list[frozenset[str]]:
        """The other characters' card epochs, locked together, likeliest first.

        A card that offers the other characters' cards (Splash, Kaleidoscope's
        rewards) draws from all of them at once, so one character's epochs at a
        time do not explain its offer. A character's epochs come in a usual
        order (ProgressSaveManager: 2 for its first act-1 boss, 5 for fifteen
        elites, 7 for a win at ascension 1), which ranks the states to try.
        """
        c = self.character
        # A character not unlocked yet comes second: with one locked, Neow
        # offers other relics, so a run that needs it stops at once without.
        states = [
            (),
            ("1", "2", "5", "7"),
            ("7",),
            ("5", "7"),
            ("2", "5", "7"),
            ("5",),
            ("2",),
            ("2", "7"),
            ("2", "5"),
        ]
        per_char: list[list[tuple[int, frozenset[str]]]] = []
        for o in ("IRONCLAD", "SILENT", "REGENT", "NECROBINDER", "DEFECT"):
            if o == c:
                continue
            mine = []
            for rank, state in enumerate(states):
                epochs = frozenset(f"{o}{n}_EPOCH" for n in state)
                if o == "IRONCLAD" and "1" in state:
                    continue  # the first character is never locked
                if not epochs & revealed:
                    mine.append((rank, epochs))
            per_char.append(mine)
        joint = [
            (sum(rank for rank, _ in combo), frozenset().union(*(e for _, e in combo)))
            for combo in itertools.product(*per_char)
        ]
        joint.sort(key=lambda t: (t[0], sorted(t[1])))
        return [e for _, e in joint if _in_unlock_order(e)][:MAX_FOREIGN]

    def g1_sets(
        self, revealed: set[str], fixed: dict[str, Any]
    ) -> list[tuple[int, int, frozenset[str]]]:
        """Sets of start-of-run epochs under which the host draws what the log shows.

        Kept: the sets that end on the header's `up_front` counter, give each
        act the log reaches its ancient, and hold every event the log visits.
        Each comes with the number of the log's fights it gives (each act's
        monster and elite rooms in order, and its boss), and with how deep in
        its relic bag the log's relics lie (the sum of their distances from
        the end they leave by: a run hands relics out from the front and a
        shop from the back, RelicFactory, so the right bag has them there).
        Sets locking the same number of relics end on the same counter with
        the bag in another order.
        """
        h = self.rows[0]
        free = [e for e in _g1(self.character) if e not in revealed]
        sets = list(_subsets(free))
        maps = {r["act"]: r for r in self.rows if r["t"] == "map" and r.get("act")}
        visited: dict[int, set[str]] = {}
        fights: dict[tuple[int, str], list[str]] = {}
        for r in self.rows:
            if r["t"] != "room" or not r.get("id") or r.get("depth", 1) != 1:
                continue
            act = r.get("act") or 1
            if r.get("room_type") == "event" and r.get("point_type") != "ancient":
                visited.setdefault(act, set()).add(r["id"])
            elif r.get("room_type") in ("monster", "elite"):
                fights.setdefault((act, r["room_type"]), []).append(r["id"])
        if (h.get("rng_state") or {}).get("up_front") is None:
            return []  # nothing to tell the sets apart by
        g = Game()
        try:
            res = g.send(
                {
                    "cmd": "probe_profiles",
                    "character": self.character.capitalize(),
                    "ascension": h.get("ascension") or 0,
                    "seed": h["seed"],
                    "profiles": [
                        {**fixed, "locked_epochs": sorted({*fixed["locked_epochs"], *x})}
                        for x in sets
                    ],
                    "up_front": (h.get("rng_state") or {}).get("up_front"),
                }
            )
        finally:
            g.close()
        if res.get("type") != "profiles":
            raise RuntimeError(f"probe_profiles: {json.dumps(res)[:300]}")
        ranked: list[tuple[int, int, frozenset[str]]] = []
        seen = _log_ids(self.rows)
        # Which end of the bag a relic left by: a shop's stock, a reward's offer.
        sold = {x.get("id") for r in self.rows if r["t"] == "shop" for x in r.get("relics") or []}
        given = {
            o.get("option_id")
            for r in self.rows
            if r["t"] == "decision" and r.get("decision_type") in ("relic_reward", "treasure")
            for o in r.get("options") or []
        } - sold
        bagged = {x for r in res["results"] for xs in (r.get("relics") or {}).values() for x in xs}
        # Events some set gives an act: the Architect's room after the last
        # boss is an event room of no act's list.
        pooled = {e for r in res["results"] for act in r["acts"] for e in act.get("events") or []}
        for r in res["results"]:
            same = 0
            place = {
                x: (len(xs) - 1 - i if x in sold else i if x in given else min(i, len(xs) - 1 - i))
                for xs in (r.get("relics") or {}).values()
                for i, x in enumerate(xs)
            }
            # A relic the log shows that this bag lacks: the set locks it.
            ok = all(x in place for x in seen & bagged)
            depth = sum(place.get(x, 0) for x in seen)
            for i, act in enumerate(r["acts"]):
                m = maps.get(i + 1)
                if m is None:
                    continue
                if m.get("ancient") and act.get("ancient") != m["ancient"]:
                    ok = False
                if not visited.get(i + 1, set()) & pooled <= set(act.get("events") or []):
                    ok = False
                same += act.get("boss") == m.get("boss")
                for kind, key in (("monster", "normal"), ("elite", "elite")):
                    for mine, theirs in zip(
                        act.get(key) or [], fights.get((i + 1, kind), []), strict=False
                    ):
                        if mine != theirs:
                            break
                        same += 1
            if ok:
                ranked.append((-same, depth, sets[r["index"]]))
        ranked.sort(key=lambda t: (t[0], t[1], len(t[2]), sorted(t[2])))
        self.log(
            f"start-of-run epochs: {len(sets)} sets, {len(res['results'])} on the counter, "
            f"{len(ranked)} with the log's ancients and events; fights matched "
            f"{[(-t[0], t[1]) for t in ranked[:6]]}"
        )
        return [(-t[0], t[1], t[2]) for t in ranked]

    def run(self, hints: list[dict[str, Any]] | None = None) -> dict[str, Any]:
        """`hints`: profiles to try first, such as the same player's other runs'."""
        t0 = time.time()
        best = Replayer(self.path, sphere_cap=SPHERE_CAP).run()
        if "desync" not in best:
            return best
        plain = best
        revealed = _seen_epochs(self.rows)
        first_run = False
        locked: frozenset[str] = frozenset()
        # What `Replayer.start` settles from the log (Neow, the act-1 variant)
        # holds for every candidate.
        best = self.replay(locked, first_run)
        fixed = {
            "locked_epochs": [
                e
                for e in best["profile"].get("locked_epochs", [])
                if e in ("NEOW_EPOCH", "UNDERDOCKS_EPOCH")
            ],
            "undiscovered_acts": best["profile"].get("undiscovered_acts", []),
        }
        self.log(f"all unlocked: progress {best['progress']} {best.get('desync')}; fixed {fixed}")
        tries = list(hints or [])
        if "NEOW_EPOCH" in fixed["locked_epochs"]:
            # No Neow at the start: possibly the player's first run ever, with
            # nothing unlocked and its rooms in the tutorial's order.
            tries.append({"locked_epochs": sorted(_epochs()), "number_of_runs": 0})
        for hint in tries:
            cand = (
                frozenset(hint.get("locked_epochs", [])) - {"NEOW_EPOCH", "UNDERDOCKS_EPOCH"},
                hint.get("number_of_runs") == 0,
            )
            r = self.replay(*cand)
            if _score(r) > _score(best):
                (locked, first_run), best = cand, r
        if "desync" not in best:
            return {**best, "profile_trials": self.trials}
        g1 = _g1(self.character)
        g1_sets = self.g1_sets(revealed, fixed)
        fixed_ids = {"NEOW_EPOCH", "UNDERDOCKS_EPOCH"}
        own = f"{self.character}1_EPOCH"

        joint = [f"{o}{n}_EPOCH" for o in CHARACTERS for n in "1257" if o != self.character]
        joint_sets = self.foreign_sets(revealed)

        def options(family: list[str]) -> Iterator[tuple[frozenset[str], bool]]:
            if family is joint:
                for s in joint_sets:
                    yield (locked - set(joint)) | s, first_run
            elif family is g1:
                # Most of the log's fights first, then nearest the current set.
                mine = locked & set(g1)
                near = sorted(g1_sets, key=lambda t: (-t[0], t[1], len(t[2] ^ mine), sorted(t[2])))
                for _, _, s in near[:MAX_G1]:
                    yield (locked - set(g1)) | s, first_run
            else:
                free = [e for e in family if e not in revealed]
                for s in _subsets(free):
                    yield (locked - set(family)) | s, first_run

        cards, potions, others, colorless, *foreign = self.families()
        every: list[list[str]] = [cards, potions, g1, others, colorless, *foreign, joint]
        # The families most likely behind each kind of stop come first.
        first: dict[str, list[list[str]]] = {
            "rng_start": [g1],
            "ancient": [g1],
            "rewards_offer": [potions, cards],
            "potion": [potions],
            "encounter": [g1],
            "event_options": [g1, others],
        }
        # Each climb is greedy and can end on a profile no single step
        # betters. The next one starts elsewhere, with its own bar to beat: a
        # new player has nearly everything locked ("all locked"), and a hint
        # from another run can lead away from this run's profile ("all
        # revealed" again).
        restarts = [
            frozenset(e for e in _epochs() if e not in revealed and e not in fixed_ids and e != own)
        ]
        if locked or first_run:
            restarts.append(frozenset())
        top = best
        # Each climb has the whole budget: one that leads nowhere must not
        # use up the next one's.
        t0, n0 = time.time(), self.trials
        while "desync" in best:
            spent = self.trials - n0 >= MAX_TRIALS or time.time() - t0 >= MAX_SECONDS
            drift = _drift(best)
            if spent or not (best["desync"] in PROFILE_STOPS or drift):
                if not restarts:
                    break
                top = max(top, best, key=_score)
                locked, first_run = restarts.pop(), False
                t0, n0 = time.time(), self.trials
                self.log(f"restart from {len(locked)} locked")
                best = self.replay(locked, first_run)
                continue
            lead = list(first.get(best["desync"], []))
            if "combat_card_generation" in drift:
                lead += [colorless, joint]
            if "combat_potion_generation" in drift:
                lead.append(potions)
            # The cards the stop names say whose pool is off.
            pools = _pools(str(best.get("detail", "")))
            if "COLORLESS" in pools:
                lead.append(colorless)
            if pools - {"COLORLESS", self.character}:
                lead.append(joint)
            if self.character in pools or not lead:
                lead.append(cards)
            detail = str(best.get("detail", ""))
            if best["desync"] == "event_options" and "NEOW." in detail:
                # Neow offers Kaleidoscope once every character is unlocked.
                lead = [others, g1]
            if best["desync"] in ("shop_stock", "rewards_offer", "floor_state"):
                if "relic" in detail:
                    lead.insert(0, g1)
                if "potion" in detail:
                    lead.insert(0, potions)
            pick: tuple[frozenset[str], bool] | None = None
            for family in [*lead, *(f for f in every if not any(f is x for x in lead))]:
                for cand in options(family):
                    if cand == (locked, first_run) or self.trials - n0 >= MAX_TRIALS:
                        continue
                    if not _in_unlock_order(cand[0]):
                        continue
                    if time.time() - t0 > MAX_SECONDS:
                        break
                    r = self.replay(*cand)
                    if _score(r) > _score(best):
                        pick, best = cand, r
                        break
                if pick is not None:
                    break
            if pick is None and best.get("sphere_capped"):
                # Stuck at a Crystal Sphere with click plans left: try them all.
                r = self.replay(locked, first_run, sphere_cap=None)
                if _score(r) > _score(best):
                    best = r
                    self.spheres = dict(r.get("sphere_attempts") or {})
                    continue
            if pick is None:
                if not restarts:
                    break
                top = max(top, best, key=_score)
                locked, first_run = restarts.pop(), False
                t0, n0 = time.time(), self.trials
                self.log(f"restart from {len(locked)} locked")
                best = self.replay(locked, first_run)
                continue
            locked, first_run = pick
            self.log(
                f"-> progress {best['progress']} {best.get('desync') or best.get('end')} "
                f"{str(best.get('detail', ''))[:300]}"
            )
        best = max(top, best, key=_score)
        # A profile that gets no further than the host's defaults explains nothing.
        if _score(best) <= _score(plain):
            # No profile helps; the host's defaults with every click plan.
            best = Replayer(self.path).run() if plain.get("sphere_capped") else plain
        out = dict(best)
        out["profile_trials"] = self.trials
        return out


def infer(
    path: str, *, verbose: bool = False, hints: list[dict[str, Any]] | None = None
) -> dict[str, Any]:
    """The replay of `path` under the profile the host follows furthest.

    The result is a `Replayer.run` result; `profile` is the profile it was run
    with (absent when the run replays with everything unlocked) and
    `profile_trials` the number of replays the search took.
    """
    try:
        return Search(path, verbose=verbose).run(hints)
    except Exception as e:
        return {
            **Replayer(path).run(),
            "profile_error": f"{type(e).__name__}: {e}"[:300],
        }
