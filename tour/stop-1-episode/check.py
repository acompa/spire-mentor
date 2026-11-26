"""Stop 1: what one decision is, and what one episode is.

Plays whole runs through the env driver's own loop (`sts2_rs.E2eDriver`: peek,
then apply a forced step or the chosen class's action), the loop that
`E2eEnv::run_to_decision` runs in `sts2-rs/crates/sts2-e2e/src/env.rs`. It logs
every step, forced or chosen, so it can count what a run is made of.

Two players:

- random: picks uniformly among the offered classes (seeded per run);
- network: the published Ironclad net, greedy. Its runs are played with
  `sts2_rs.e2e_selfplay` (fast, threaded) and then replayed through the logged
  loop with the same choices. The replay must end where the self-play ended.

    tour/run stop-1-episode/check.py            # default panel
    tour/run stop-1-episode/check.py --quick    # about two minutes
"""

from __future__ import annotations

import argparse
import collections
import random
import statistics
import sys
import time
from collections.abc import Callable, Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import tourlib

HERE = Path(__file__).resolve().parent
NET = "ironclad_e2e_d9000_ema0999.e2ew"
TRACE_DECISIONS = 14


# ── one episode, step by step ───────────────────────────────────────────────


@dataclass(frozen=True)
class Step:
    """One thing the env did: a forced step (no choice) or a decision."""

    forced: bool
    combat: bool
    dtype: str  # decision type, e.g. COMBAT_TURN, MAP; "" for a forced step
    n_classes: int  # options offered; 1 for a forced step
    chosen: int | None  # the class picked; None for a forced step
    floor: int | None
    action: Any = None  # the run-action dict that was applied
    text: str = ""  # the choice in words (only when `play(words=True)`)


@dataclass
class Episode:
    seed: int
    steps: list[Step] = field(default_factory=list)
    end: dict[str, Any] = field(default_factory=dict)

    @property
    def decisions(self) -> list[Step]:
        return [s for s in self.steps if not s.forced]

    @property
    def n_forced(self) -> int:
        return sum(s.forced for s in self.steps)

    @property
    def chosen(self) -> list[int]:
        return [s.chosen for s in self.decisions]


#: A player: (driver, peek, run, decision dict) -> merged-class index.
Chooser = Callable[[Any, Any, Any, dict[str, Any]], int]
#: Called at every decision with the class picked (for the readable trace).
Observer = Callable[[Any, Any, Any, dict[str, Any], int], None]


def play(
    seed: int, choose: Chooser, observe: Observer | None = None, words: bool = False
) -> Episode:
    """One whole run of `seed`, from the first choice to the end.

    The loop is the env's: peek at the position; a forced step is applied as
    the env names it; a decision is answered with a class, and the class's
    representative action is applied. `words` also records each choice in
    words (slower: it reads the position at every decision)."""
    import sts2_rs

    rm = sts2_rs.e2e_new_run(seed)
    drv = sts2_rs.E2eDriver(seed)
    ep = Episode(seed)
    while True:
        p = drv.peek(rm)
        if p.kind == "terminal":
            ep.end = dict(p.terminal)
            return ep
        if p.kind == "forced":
            f = p.forced
            ep.steps.append(Step(True, f["kind"] == "combat", "", 1, None, None, f["action"]))
            drv.apply(rm, f["action"], f["kind"])
            continue
        d = p.decision
        c = int(choose(drv, p, rm, d))
        if observe is not None:
            observe(drv, p, rm, d, c)
        a = p.class_action(c)
        text = class_words(rm, d)[c] if words else ""
        ep.steps.append(
            Step(
                False,
                bool(d["is_combat"]),
                d["decision_type"],
                int(d["n_classes"]),
                c,
                int(d["floor"]),
                a["action"],
                text,
            )
        )
        drv.apply(rm, a["action"], a["kind"])


def random_chooser(seed: int) -> Chooser:
    """Uniform over the offered classes, from an RNG seeded by `seed`."""
    rng = random.Random(seed)
    return lambda drv, p, rm, d: rng.randrange(int(d["n_classes"]))


def scripted_chooser(chosen: Sequence[int]) -> Chooser:
    """Replays a recorded list of class choices, in order."""
    it = iter(chosen)
    return lambda drv, p, rm, d: next(it)


def net_chooser(student: Any) -> Chooser:
    """The network's greedy class (the same `argmax` `e2e_selfplay` takes)."""
    return lambda drv, p, rm, d: int(student.decide(drv, p, rm)["class"])


# ── a panel's numbers ───────────────────────────────────────────────────────


def summarize(eps: Sequence[Episode]) -> dict[str, Any]:
    """What a panel of episodes is made of. Rates pool every decision (or
    step) in the panel; per-run figures are means over runs."""
    decisions = [s for e in eps for s in e.decisions]
    n_dec = len(decisions)
    n_steps = sum(len(e.steps) for e in eps)
    per_run = [len(e.decisions) for e in eps]
    return {
        "runs": len(eps),
        "decisions_per_run": statistics.fmean(per_run) if eps else 0.0,
        "decisions_min_median_max": (min(per_run), statistics.median(per_run), max(per_run))
        if eps
        else (0, 0, 0),
        "combat_share": sum(s.combat for s in decisions) / n_dec if n_dec else 0.0,
        "forced_share": sum(e.n_forced for e in eps) / n_steps if n_steps else 0.0,
        "forced_combat_share": (
            sum(s.combat for e in eps for s in e.steps if s.forced)
            / max(1, sum(e.n_forced for e in eps))
        ),
        "median_classes": statistics.median(s.n_classes for s in decisions) if n_dec else 0,
        "classes_hist": collections.Counter(s.n_classes for s in decisions),
        "by_type": collections.Counter(s.dtype for s in decisions),
        "mean_floor": statistics.fmean(e.end.get("floor") or 0 for e in eps) if eps else 0.0,
        "floors": sorted(int(e.end.get("floor") or 0) for e in eps),
        "outcomes": collections.Counter(e.end.get("outcome") for e in eps),
    }


# ── the readable trace ──────────────────────────────────────────────────────


def describe(
    action: dict[str, Any],
    hand: list[dict] | None = None,
    enemies: list[dict] | None = None,
    potions: list[str] | None = None,
    offered: list[str] | None = None,
) -> str:
    """A class's representative action in words, naming cards, targets, potions
    and offered options where the position shows them."""
    name = action.get("action")
    t = action.get("target_index")
    at = f" at {enemies[t]['id']}" if t is not None and enemies and t < len(enemies) else ""
    if name == "play" and hand is not None:
        i = action["card_index"]
        return f"play {hand[i]['id'] if i < len(hand) else f'card {i}'}{at}"
    if name == "drink_potion" and potions is not None:
        i = action["slot"]
        return f"drink {potions[i] if i < len(potions) else f'potion {i}'}{at}"
    if name == "end_turn":
        return "end turn"
    i = action.get("idx")
    if "idx" in action and i is None:
        return f"{name}: skip"
    if offered is not None and isinstance(i, int) and i < len(offered):
        return f"{name}: {offered[i]}"
    rest = {k: v for k, v in action.items() if k != "action"}
    return f"{name} {rest}" if rest else str(name)


def class_words(rm: Any, d: dict[str, Any]) -> list[str]:
    """Every offered class of decision `d` in words (the run must still be at `d`)."""
    kw: dict[str, Any] = {}
    if d["is_combat"]:
        st = rm.combat_state().get_state()
        kw = {"hand": st.get("hand"), "enemies": st.get("enemies"), "potions": list(rm.potions())}
    elif d["decision_type"] == "CARD_REWARD":
        kw = {"offered": [c.removeprefix("CARD.") for c in rm.card_reward_option_ids()]}
    elif d["decision_type"] in ("EVENT", "NEOW"):
        kw = {"offered": [label for _key, label in rm.event_options()]}
    elif d["decision_type"] == "REST":
        kw = {"offered": [name for name, _ok in rm.rest_site_options()]}
    return [describe(c["actions"][0], **kw) for c in d["classes"]]


def trace_line(i: int, s: Step, forced_before: int) -> str:
    where = "fight " if s.combat else "between"
    skipped = f"  (+{forced_before} forced before)" if forced_before else ""
    return (
        f"  #{i:<3} floor {s.floor if s.floor is not None else '-':>2}  {where}  "
        f"{s.dtype:<13} {s.n_classes:>2} classes  picked {s.chosen}: "
        f"{s.text or describe(s.action)}{skipped}"
    )


def print_trace(label: str, ep: Episode, n: int = TRACE_DECISIONS) -> None:
    print(f"\n{label}: the first {n} decisions of seed {ep.seed}")
    forced = 0
    shown = 0
    for s in ep.steps:
        if s.forced:
            forced += 1
            continue
        print(trace_line(shown, s, forced))
        forced = 0
        shown += 1
        if shown >= n:
            break
    print(
        f"  ... the run ended '{ep.end.get('outcome')}' on floor {ep.end.get('floor')} "
        f"after {len(ep.decisions)} decisions and {ep.n_forced} forced steps."
    )


def section_counts(row: dict[str, Any]) -> list[tuple[str, int]]:
    """The policy row's tokens grouped by section, in row order."""
    import numpy as np
    from ppo.e2e import contracts as C

    secs = np.frombuffer(row["sec"], np.int8)
    out: dict[str, int] = {}
    for s in secs:
        out[C.SECTIONS[int(s)]] = out.get(C.SECTIONS[int(s)], 0) + 1
    return list(out.items())


def render(student: Any, drv: Any, p: Any, rm: Any, d: dict[str, Any], picked: int) -> str:
    """One decision as a person would see it, then as the policy sees it."""
    out = [f"  decision type {d['decision_type']}, act {d['act']}, floor {d['floor']}"]
    if d["is_combat"]:
        cs = rm.combat_state()
        st = cs.get_state()
        pl = st["player"]
        out.append(
            f"  round {st['round']}: HP {pl['hp']}/{pl['max_hp']}, block {pl['block']}, "
            f"energy {pl['energy']}/{pl['max_energy']}"
        )
        out.append("  hand: " + ", ".join(f"{c['id']} (cost {c['cost']})" for c in st["hand"]))
        out.append("  potions: " + (", ".join(rm.potions()) or "none"))
        for e in st["enemies"]:
            out.append(
                f"  enemy {e['id']}: HP {e['hp']}/{e['max_hp']}, intends "
                f"{e['intent']} {e.get('intent_damage') or ''}x{e.get('intent_hits') or 1}"
            )
        out.append(
            f"  draw pile: {st['draw_pile_count']} cards; discard "
            f"{st['discard_pile_count']}; exhaust {st['exhaust_pile_count']}"
        )
        top = ", ".join(c["id"] for c in cs.true_draw_top(3))
        out.append(f"  true top of the draw pile (hidden from the policy): {top}")
    else:
        out.append(
            f"  HP {rm.current_hp}/{rm.max_hp}, gold {rm.gold}, "
            f"deck of {len(rm.master_deck())} cards"
        )
    words = class_words(rm, d)
    probs = student.decide(drv, p, rm)["probs"]
    out.append("  classes (merged options):")
    for c in d["classes"]:
        n = len(c["actions"])
        members = f"  [{n} identical actions merged]" if n > 1 else ""
        mark = "  <- picked" if c["class"] == picked else ""
        out.append(
            f"    {c['class']}: {words[c['class']]:<40} p={probs[c['class']]:.3f}{members}{mark}"
        )
    row = student.encode(drv, p, rm)
    n_tok = int(row["n_tok"])
    n_cat = len(row["cat"]) // 2 // n_tok
    n_cont = len(row["cont"]) // 2 // n_tok
    out.append(f"  what the policy reads: {n_tok} tokens, each {n_cat} ids + {n_cont} numbers")
    out.append("    " + ", ".join(f"{s} x{k}" for s, k in section_counts(row)))
    return "\n".join(out)


class Showcase:
    """Captures one fight decision and one between-fight decision of a run."""

    def __init__(self, student: Any) -> None:
        self.student = student
        self.fight: str | None = None
        self.between: str | None = None

    def __call__(self, drv: Any, p: Any, rm: Any, d: dict[str, Any], c: int) -> None:
        if d["is_combat"] and self.fight is None and d.get("turn") == 1:
            self.fight = render(self.student, drv, p, rm, d, c)
        elif not d["is_combat"] and self.between is None and d["decision_type"] == "CARD_REWARD":
            self.between = render(self.student, drv, p, rm, d, c)


# ── main ────────────────────────────────────────────────────────────────────


def print_summary(label: str, s: dict[str, Any]) -> None:
    lo, med, hi = s["decisions_min_median_max"]
    print(f"\n{label} ({s['runs']} runs)")
    print(
        f"  decisions per run: mean {s['decisions_per_run']:.1f}  (min {lo}, median {med}, max {hi})"
    )
    print(f"  share of decisions inside fights: {s['combat_share']:.3f}")
    print(
        f"  share of all steps that were forced: {s['forced_share']:.3f}"
        f"  (of those, inside fights: {s['forced_combat_share']:.3f})"
    )
    print(f"  classes offered per decision: median {s['median_classes']}")
    hist = s["classes_hist"]
    print("    " + "  ".join(f"{k}:{hist[k]}" for k in sorted(hist)))
    print("  decisions by type:")
    total = sum(s["by_type"].values()) or 1
    for t, n in s["by_type"].most_common():
        print(f"    {t:<15} {n:>7}  {n / total:6.1%}")
    floors = s["floors"]
    print(
        f"  floor reached: mean {s['mean_floor']:.2f}, median {statistics.median(floors) if floors else 0}"
    )
    print("  how runs ended: " + ", ".join(f"{k} {v}" for k, v in s["outcomes"].most_common()))


def main(argv: Sequence[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--quick", action="store_true", help="20 seeds instead of the default")
    ap.add_argument("--seeds", type=int, default=None, help="seeds from PANEL_A (default 200)")
    ap.add_argument("--workers", type=int, default=10, help="threads for network self-play")
    args = ap.parse_args(argv)
    n = args.seeds or (20 if args.quick else 200)

    tourlib.load_predictions(HERE)  # refuse to run before the predictions exist

    import sts2_rs

    student = sts2_rs.E2eStudent(str(tourlib.weight(NET)))
    seeds = list(tourlib.PANEL_A[:n])
    t0 = time.time()

    # 1. One seed, both players, logged and shown.
    s0 = seeds[0]
    rand0 = play(s0, random_chooser(s0), words=True)
    show = Showcase(student)
    net0 = play(s0, net_chooser(student), observe=show, words=True)
    print_trace("Random play", rand0)
    print_trace("Network", net0)
    print("\nOne decision inside a fight (the network's first, on seed", s0, ")")
    print(show.fight or "  (none)")
    print("\nOne decision between fights (the network's first card reward)")
    print(show.between or "  (none on this seed)")

    # 2. The panel.
    print(f"\nPlaying {n} seeds of PANEL_A with each player ...", flush=True)
    rand_eps = [play(s, random_chooser(s)) for s in seeds]
    t1 = time.time()
    runs = tourlib.selfplay_many(student, seeds, workers=args.workers)
    net_eps = []
    for seed, r in zip(seeds, runs, strict=True):
        ep = play(seed, scripted_chooser(r["chosen"]))
        if ep.chosen != list(r["chosen"]) or ep.end.get("outcome") != r["outcome"]:
            raise SystemExit(f"seed {seed}: the logged replay left the self-play run")
        net_eps.append(ep)
    t2 = time.time()

    rs, ns = summarize(rand_eps), summarize(net_eps)
    print_summary("Random play", rs)
    print_summary("Network", ns)
    print(f"\ntime: random panel {t1 - t0:.0f}s, network panel {t2 - t1:.0f}s")

    tourlib.score(
        HERE,
        {
            "net_decisions_per_run": ns["decisions_per_run"],
            "random_decisions_per_run": rs["decisions_per_run"],
            "net_fight_share": ns["combat_share"],
            "net_median_classes": ns["median_classes"],
            "random_mean_floor": rs["mean_floor"],
            "net_forced_share": ns["forced_share"],
        },
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
