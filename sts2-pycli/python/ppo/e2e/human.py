"""Human starts: a share of training runs start where a real player stood mid-run.

Every training run starts on act 1, floor 1, with the starter deck, so the policy only
ever sees the decks and relics its own play reaches. A human start instead takes a
state from the scraped run corpus (`sts2-rs/data/run-corpus`, `just unpack-corpus`):
a real player's deck, relics, potions, HP, max HP and gold entering some floor of
act 1, 2 or 3. The run is parked on the engine map in the same act and row, in
`map_choice`, and the policy plays on from there. The decks, archetypes and relic sets
it meets are then the ones humans build, not only the ones its own picks lead to.

A start is a function of the run seed only::

    start = draw_start(seed, character, cfg)   # None: an ordinary run

Like `ppo.e2e.forced`, the seed hashes to u in [0, 1); u < p makes a human start, and
a second hash picks one of the character's starts in the bank. Every row stays
on-policy: the policy makes every decision from the start on.

The bank (`build_bank`, ``python -m ppo.e2e.human build``) is a gzipped JSONL file of
start records, built once from the corpus. Building replays each run's deck backward
from its final deck (`pilot_human_decks.DeckReplay`), skips floors the replay is
unsure of, and applies every start to an engine run, keeping only the ones the engine
can build (cards and relics of the corpus's game builds that v0.111.0 lacks fail
there).

What a start does not restore: the human's map (the engine's own map for the run
seed is used, standing on a node of the same row), relic counters and quest state,
and the RNG streams. Potions are reconstructed from the per-floor potion log (picks
and purchases minus uses and discards, capped at the belt size), which misses potions
from events. The run's ascension stays the lane's, whatever the human played at.

The goal is a policy that can take over a run a human started (the advisor's
situation), so a tenth of the corpus RUNS (`split_of`, `EVAL_SHARE`) never feed
training: their starts are the held-out set for measuring exactly that.

Human-start runs feed no practice queue (a practice start replays its capture from the
seed and action log). Their action-log record carries the whole start
(`extra.human_start`), so `actionlog` replays them without the bank. The learner
counts them separately (stats row `human`, per start act).

The learner takes it as `--human` (JSON, `HumanCfg.from_dict`), keeps it in its state
and hands it to its workers (`worker_cfg()["human"]`).
"""

from __future__ import annotations

import argparse
import collections
import gzip
import hashlib
import json
import random
import sys
from collections.abc import Iterator, Mapping
from dataclasses import dataclass
from functools import cache
from pathlib import Path
from typing import Any

from . import factory

REPO = Path(__file__).resolve().parents[4]
CORPUS = REPO / "sts2-rs" / "data" / "run-corpus" / "files"
DEFAULT_BANK = REPO / "sts2-pycli" / "data" / "ppo" / "human_starts.jsonl.gz"

CHARACTER_OF = {f"CHARACTER.{c.upper()}": c for c in factory.CHARACTERS}
BELT = 3  # the potion belt's size without relics (`max_potion_slot_count`'s base)


def _bare(x: str) -> str:
    return x.split(".", 1)[1] if "." in x else x


def _hash64(seed: int, salt: bytes) -> int:
    h = hashlib.blake2b(int(seed).to_bytes(8, "little", signed=True), digest_size=8, person=salt)
    return int.from_bytes(h.digest(), "little")


# ── the config ───────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class HumanCfg:
    p: float = 0.1  # share of full runs that start from a human state
    bank: str = ""  # the bank file; "": `DEFAULT_BANK`

    def __post_init__(self) -> None:
        if not 0.0 <= float(self.p) <= 1.0:
            raise ValueError(f"human: p {self.p} outside [0, 1]")
        object.__setattr__(self, "p", float(self.p))
        object.__setattr__(self, "bank", str(self.bank))

    @property
    def path(self) -> Path:
        return Path(self.bank) if self.bank else DEFAULT_BANK

    def to_dict(self) -> dict[str, Any]:
        """Plain JSON types (the learner state loads with `weights_only=True`)."""
        return {"p": self.p, "bank": self.bank}

    @classmethod
    def from_dict(cls, d: Mapping[str, Any]) -> HumanCfg:
        """`--human`'s JSON; unset keys take the defaults, unknown keys raise."""
        if unknown := sorted(set(d) - {"p", "bank"}):
            raise ValueError(f"human: unknown keys {unknown} (known: ['bank', 'p'])")
        return cls(p=float(d.get("p", cls.p)), bank=str(d.get("bank") or ""))


EVAL_SHARE = 0.1  # corpus runs held out of training, to measure continuing unseen runs


def split_of(key: str) -> str:
    """`"train"` or `"eval"` for a start key (`<run>:<floor>`), by a hash of its RUN: every
    start of a held-out run is held out, so an eval start's run was never trained on."""
    run = key.rsplit(":", 1)[0].encode()
    h = hashlib.blake2b(run, digest_size=8, person=b"e2e-human-split").digest()
    return "eval" if int.from_bytes(h, "little") / 2.0**64 < EVAL_SHARE else "train"


@cache
def _bank_lines(path: str, split: str = "train") -> dict[str, tuple[str, ...]]:
    """The bank at `path`, per character: the raw JSON lines of `split`'s starts in file
    order. Every actor holds one, so it keeps the text (~25 MB for the full bank) rather
    than the parsed dicts (~130 MB); `draw_start` parses only the line it draws. Raises
    FileNotFoundError with the build command."""
    p = Path(path)
    if not p.exists():
        raise FileNotFoundError(
            f"human-start bank {p} is missing: build it with "
            "`uv run python -m ppo.e2e.human build` (needs `just unpack-corpus` in sts2-rs)"
        )
    by: dict[str, list[str]] = collections.defaultdict(list)
    with gzip.open(p, "rt") as f:
        for line in f:
            if line := line.strip():
                s = json.loads(line)
                if split == "all" or split_of(s["key"]) == split:
                    by[s["character"]].append(line)
    return {c: tuple(v) for c, v in by.items()}


@cache
def load_bank(path: str, split: str = "train") -> dict[str, tuple[dict[str, Any], ...]]:
    """The bank at `path`, per character, parsed: the starts of `split` (`split_of`;
    "all": every start). Training draws from "train" only. Raises FileNotFoundError with
    the build command."""
    return {c: tuple(json.loads(x) for x in v) for c, v in _bank_lines(path, split).items()}


def clear_bank_cache() -> None:
    """Forget every bank read so far (a rebuilt bank at the same path is read anew)."""
    _bank_lines.cache_clear()
    load_bank.cache_clear()
    eval_starts.cache_clear()


def draw_start(seed: int, character: str, cfg: HumanCfg | None) -> dict[str, Any] | None:
    """The human start the run with `seed` of `character` plays from, or None (module doc)."""
    if cfg is None or cfg.p <= 0.0:
        return None
    if _hash64(seed, b"e2e-human-u") / 2.0**64 >= cfg.p:
        return None
    pool = _bank_lines(str(cfg.path)).get(str(character), ())
    if not pool:
        return None
    return json.loads(pool[_hash64(seed, b"e2e-human-s") % len(pool)])


# ── the held-out eval ────────────────────────────────────────────────────────

#: `ppo.e2e.evaluate run --env` for the held-out eval: DEV seed i plays held-out start i.
EVAL_ENV = "ppo.e2e.human:eval_env"
#: The bank the held-out eval reads (`evaluate`'s clean env passes it on), else the default.
BANK_ENV = "STS2_E2E_HUMAN_BANK"


def bank_path() -> Path:
    import os

    raw = os.environ.get(BANK_ENV, "").strip()
    return Path(raw) if raw else DEFAULT_BANK


@cache
def eval_starts(character: str, path: str) -> tuple[dict[str, Any], ...]:
    """`character`'s held-out starts in a fixed shuffled order (a hash of the key), so
    any prefix is a sample over runs, floors and acts."""
    pool = load_bank(path, "eval").get(str(character), ())

    def order(s: Mapping[str, Any]) -> bytes:
        return hashlib.blake2b(s["key"].encode(), digest_size=8, person=b"e2e-human-ord").digest()

    return tuple(sorted(pool, key=order))


def eval_start(seed: int, character: str, path: Path | None = None) -> dict[str, Any]:
    """The held-out start DEV seed `seed` plays: start i for seed DEV.start + i."""
    from .panels import DEV_2000

    pool = eval_starts(str(character), str(path or bank_path()))
    if not pool:
        raise ValueError(f"no held-out human starts of {character} in {path or bank_path()}")
    i = int(seed) - DEV_2000.start
    if not 0 <= i < len(pool):
        raise ValueError(f"seed {seed}: held-out start {i} outside [0, {len(pool)}) of {character}")
    return pool[i]


def eval_env(seed: int) -> Any:
    """`evaluate --env`: the process character's held-out start for DEV seed `seed`."""
    from .env import E2EEnv, EnvConfig

    start = eval_start(seed, factory.CHARACTER)
    return E2EEnv(int(seed), cfg=EnvConfig(human_start=start, character=start["character"]))


def eval_report(rows: list[Mapping[str, Any]], path: Path | None = None) -> dict[str, Any]:
    """The held-out eval's rows (`evaluate` rows: seed, character, won) joined to their
    starts: per start act and overall, the policy's wins against the humans' own on the
    same starts. Error rows (no `won`) are left out."""
    from . import stats as ST

    by: dict[str, list[tuple[bool, bool]]] = collections.defaultdict(list)
    for r in rows:
        if "won" not in r or r.get("outcome") == "error":
            continue
        s = eval_start(int(r["seed"]), str(r["character"]), path)
        pair = (bool(r["won"]), bool(s["won"]))
        by[f"act{s['act']}"].append(pair)
        by["all"].append(pair)

    def agg(ps: list[tuple[bool, bool]]) -> dict[str, Any]:
        n, w, h = len(ps), sum(p for p, _ in ps), sum(q for _, q in ps)
        lo, hi = ST.wilson(w, n)
        return {"runs": n, "wins": w, "win_rate": w / n, "ci_lo": lo, "ci_hi": hi,
                "human_win_rate": h / n}  # fmt: skip

    return {k: agg(v) for k, v in sorted(by.items())}


def human_stats(run_ends: list[Mapping[str, Any]]) -> dict[str, Any]:
    """A stats row's `human`: the human-start runs among `run_ends` (end records), overall
    and per start act (`act1` ...): runs, wins and mean floor. Empty when none."""
    by: dict[str, list[Mapping[str, Any]]] = collections.defaultdict(list)
    for e in run_ends:
        if e.get("human_key"):
            by[f"act{e.get('human_act')}"].append(e)
    if not by:
        return {}

    def agg(es: list[Mapping[str, Any]]) -> dict[str, Any]:
        n = len(es)
        return {
            "runs": n,
            "won": sum(bool(e.get("won")) for e in es),
            "floor_mean": sum(float(e.get("floor", 0)) for e in es) / n,
        }

    every = [e for es in by.values() for e in es]
    return {"all": agg(every), "acts": {k: agg(v) for k, v in sorted(by.items())}}


# ── applying a start to a run ────────────────────────────────────────────────


def card_spec(card: str) -> dict[str, Any]:
    """A `DeckReplay` card string (`ID[+1][@ENCH:amount]`) as a graft card spec."""
    base, _, ench = card.partition("@")
    upgraded = base.endswith("+1")
    spec: dict[str, Any] = {"id": base.removesuffix("+1"), "upgraded": upgraded}
    if ench:
        eid, _, amount = ench.partition(":")
        spec["enchantment"] = eid
        spec["enchantment_amount"] = int(amount or 1)
    return spec


def _place(rm: Any, start: Mapping[str, Any], seed: int) -> None:
    """Park `rm` in `map_choice` on a node of the start's act, in its row (clamped to the
    engine map's last row before the boss), with a path up to it as the visited nodes."""
    act = int(start["act"])
    try:  # advances the run to `act` and generates its map, then refuses the node
        rm.debug_place_on_map(act, -1, -1, [], 0, 0)
    except ValueError:
        pass
    fm = rm.full_map()
    nodes = {(n["col"], n["row"]): n for r in fm["rows"] for n in r}
    row = max(0, min(int(start["row"]), int(fm["boss"]["row"]) - 1))
    while row > 0 and not any(r == row for _, r in nodes):
        row -= 1
    here = sorted(c for c in nodes if c[1] == row)
    rng = random.Random(_hash64(seed, b"e2e-human-m"))
    node = rng.choice(here)
    path = [node]
    while path[-1][1] > 0:
        col, r = path[-1]
        parents = sorted(
            c
            for c in nodes
            if c[1] == r - 1
            and any((ch["col"], ch["row"]) == (col, r) for ch in nodes[c]["children"])
        )
        if not parents:
            break
        path.append(rng.choice(parents))
    total = max(0, int(start["floor"]) - 1 - act) - (int(start["row"]) - row)
    rm.debug_place_on_map(act, node[0], node[1], path[::-1], row + 1, max(row + 1, total))


def apply_start(rm: Any, start: Mapping[str, Any], seed: int) -> list[str]:
    """Turn `rm` (a fresh run of the start's character) into the human start: placed on
    the map, then relics (their pickup hooks run, then the list is put back in the
    human's order and the choices they queue are dropped), deck, potions, HP and gold.
    Returns the deck cards the engine could not build (left out).

    The belt keeps at most the RUN's slots (`max_potion_slots`, after the relics: the
    lane's ascension and the start's belt relics), the oldest potions first: the
    human's own belt can be bigger (a lower ascension, or the scrape's potion log
    missing a use), and an overfull belt breaks F9's `run.potion_overflow`. HP is
    clamped to [1, max HP] and gold to >= 0 for the same reason (`run.hp_range`,
    `run.gold_negative`)."""
    if str(rm.character) != start["character"]:
        raise ValueError(f"human start of {start['character']} on a {rm.character} run")
    _place(rm, start, seed)
    relics = list(start["relics"])
    rm.debug_set_loadout(None, [])
    for r in relics:
        rm.force_add_relic(r)
    rm.debug_set_loadout(None, relics)
    rm.debug_clear_pending_choices()
    missing = list(rm.debug_set_master_deck([card_spec(c) for c in start["deck"]]))
    rm.debug_set_potion_state(list(start["potions"])[: max(0, int(rm.max_potion_slots()))], [])
    max_hp = min(max(1, int(start["max_hp"])), STAT_MAX)
    rm.set_player(
        min(max(1, int(start["hp"])), max_hp), max_hp, min(max(0, int(start["gold"])), STAT_MAX)
    )
    return missing


# ── building the bank from the corpus ────────────────────────────────────────


def _potions(pts: list[dict[str, Any]], before: int, slots: int) -> list[str]:
    """The potion belt entering floor `before` (1-based), from the per-floor potion log."""
    belt: list[str] = []
    for pt in pts[: before - 1]:
        ps = pt["player_stats"][0]
        for c in ps.get("potion_choices") or []:
            if c.get("was_picked"):
                belt.append(_bare(c["choice"]))
        belt += [_bare(x) for x in ps.get("bought_potions") or []]
        for x in [*(ps.get("potion_used") or []), *(ps.get("potion_discarded") or [])]:
            if _bare(x) in belt:
                belt.remove(_bare(x))
        belt = belt[-slots:]
    return belt


#: No real run gets near this much gold, HP or max HP. A corpus run that does was edited,
#: and its gold breaks the tokenizer's float16 features (run 12623776…: ~960M gold).
STAT_MAX = 10_000


def run_rejection(d: Mapping[str, Any]) -> str | None:
    """Why a corpus run gives no starts, or None."""
    players = d.get("players") or []
    if d.get("game_mode") != "standard":
        return "not_standard"
    if len(players) != 1:
        return "not_single_player"
    if players[0].get("character") not in CHARACTER_OF:
        return "character"
    if any(any(k.startswith("save_dict") for k in c) for c in players[0].get("deck") or []):
        return "modded_card"
    for act in d.get("map_point_history") or []:
        for pt in act:
            for st in pt.get("player_stats") or []:
                if any(
                    abs(int(st.get(k) or 0)) > STAT_MAX
                    for k in ("current_gold", "current_hp", "max_hp")
                ):
                    return "implausible_stats"
    return None


def starts_from_run(d: dict[str, Any], key: str, per_run: int) -> tuple[list[dict[str, Any]], str]:
    """Up to `per_run` start records of one corpus run (floors drawn by a hash of `key`),
    and why there are none ("" when there are some)."""
    from ppo.pilot_human_decks import DeckReplay, relic_timeline

    if why := run_rejection(d):
        return [], why
    p = d["players"][0]
    pts = [pt for act in d["map_point_history"] for pt in act]
    try:
        replay = DeckReplay(d)
    except (KeyError, IndexError, TypeError, ValueError):
        return [], "deck_replay"
    relics_at = relic_timeline(p.get("relics") or [], pts, True)
    slots = int(p.get("max_potion_slot_count") or BELT)
    # (act, act-local row stood on, floor entered): point k >= 2 of an act is entered
    # from row k - 2 (point 0 is the act's ancient, point k >= 1 sits on row k - 1).
    cands = []
    floor = 0
    for a, act in enumerate(d["map_point_history"], start=1):
        for k in range(len(act)):
            floor += 1
            if k >= 2:
                cands.append((a, k - 2, floor))
    sure = [c for c in cands if not replay.notes_from(c[2])]
    if not sure:
        return [], "deck_unsure"
    rng = random.Random(
        _hash64(int.from_bytes(key.encode()[:8].ljust(8, b"\0"), "little"), b"e2e-human-f")
    )
    # Spread over the acts the run reached (round robin), not uniform over floors, which
    # would make most starts act 1 (most runs end before act 3).
    by_act: dict[int, list[tuple[int, int, int]]] = collections.defaultdict(list)
    for c in sure:
        by_act[c[0]].append(c)
    for v in by_act.values():
        rng.shuffle(v)
    picked = []
    while len(picked) < per_run and any(by_act.values()):
        for a in sorted(by_act):
            if by_act[a] and len(picked) < per_run:
                picked.append(by_act[a].pop())
    picked.sort(key=lambda c: c[2])
    out = []
    for act, row, f in picked:
        prev = pts[f - 2]["player_stats"][0]
        if int(prev.get("current_hp") or 0) <= 0:
            continue
        deck, _ = replay.deck_at(f)
        out.append(
            {
                "key": f"{key}:{f}",
                "character": CHARACTER_OF[p["character"]],
                "build": d.get("build_id"),
                "ascension": int(d.get("ascension") or 0),
                "won": bool(d.get("win")),
                "act": act,
                "row": row,
                "floor": f,
                "hp": int(prev["current_hp"]),
                "max_hp": int(prev["max_hp"]),
                "gold": int(prev.get("current_gold") or 0),
                "deck": deck,
                "relics": relics_at(f),
                "potions": _potions(pts, f, slots),
            }
        )
    return out, "" if out else "dead"


def _corpus_files(root: Path) -> Iterator[Path]:
    return iter(sorted(root.rglob("*.run")))


def check_start(start: dict[str, Any], seed: int) -> str | None:
    """Apply `start` to a fresh engine run: None if it builds, else why not."""
    rm = factory.new_run(seed, character=start["character"])
    try:
        missing = apply_start(rm, start, seed)
    except (ValueError, RuntimeError) as e:
        return f"engine: {str(e)[:80]}"
    if missing:
        return "cards: " + ",".join(sorted(set(missing)))
    if rm.phase() != "map_choice" or not rm.available_actions():
        return f"phase {rm.phase()}"
    # The env's F9 invariants (`ppo.e2e.invariants`), out of combat: a start that
    # breaks one would stop the lane (exit 87).
    from .invariants import check

    if bad := check(rm):
        return "invariant: " + ",".join(sorted({v.code for v in bad}))
    return None


def build_bank(
    root: Path, out: Path, per_run: int, limit: int | None = None
) -> collections.Counter:
    """Write the bank of every buildable start of the corpus under `root` to `out`."""
    why: collections.Counter = collections.Counter()
    n = 0
    out.parent.mkdir(parents=True, exist_ok=True)
    tmp = out.with_suffix(".tmp")
    with gzip.open(tmp, "wt") as f:
        for i, path in enumerate(_corpus_files(root)):
            if limit is not None and i >= limit:
                break
            try:
                d = json.loads(path.read_text())
            except (OSError, ValueError):
                why["run/unreadable"] += 1
                continue
            starts, reason = starts_from_run(d, path.stem, per_run)
            if reason:
                why[f"run/{reason}"] += 1
            for s in starts:
                bad = check_start(s, _hash64(n, b"e2e-human-b") % 2**31)
                if bad is not None:
                    why[f"start/{bad.split(':')[0]}"] += 1
                    why[f"detail/{bad}"] += 1
                    continue
                f.write(json.dumps(s, separators=(",", ":")) + "\n")
                why[f"kept/{s['character']}/act{s['act']}"] += 1
                n += 1
    tmp.replace(out)
    return why


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(prog="python -m ppo.e2e.human", description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build", help="build the bank of human starts from the run corpus")
    b.add_argument("--corpus", type=Path, default=CORPUS)
    b.add_argument("--out", type=Path, default=DEFAULT_BANK)
    b.add_argument("--per-run", type=int, default=8, help="starts per corpus run (default 8)")
    b.add_argument("--limit", type=int, default=None, help="only the first N corpus runs")
    r = sub.add_parser("report", help="the held-out eval of an evaluate arm, against the humans")
    r.add_argument("arm_dir", type=Path, help="<evals>/DEV/<arm>")
    r.add_argument("--bank", type=Path, default=None)
    args = ap.parse_args(argv)
    if args.cmd == "report":
        rows = [
            json.loads(x)
            for f in sorted((args.arm_dir / "shards").glob("*.jsonl"))
            for x in f.read_text().splitlines()
            if x.strip()
        ]
        rep = eval_report(rows, args.bank)
        for k, v in rep.items():
            print(
                f"{k:5} n {v['runs']:5}  policy {v['win_rate']:.3f} "
                f"[{v['ci_lo']:.3f}, {v['ci_hi']:.3f}]  humans {v['human_win_rate']:.3f}"
            )
        return 0
    if not args.corpus.is_dir():
        print(f"no corpus at {args.corpus}: run `just unpack-corpus` in sts2-rs", file=sys.stderr)
        return 2
    why = build_bank(args.corpus, args.out, args.per_run, args.limit)
    kept = sum(v for k, v in why.items() if k.startswith("kept/"))
    print(f"{kept} starts -> {args.out}")
    for k, v in sorted(why.items()):
        if not k.startswith("detail/") or v >= 20:
            print(f"  {k}: {v}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
