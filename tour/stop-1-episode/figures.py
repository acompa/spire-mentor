"""Data and plots for Chapter 1 ("The decision process").

    tour/run stop-1-episode/figures.py collect [--seeds 200] [--workers 3]
    tour/run stop-1-episode/figures.py render

`collect` plays the first `--seeds` seeds of PANEL_A with random play and with
the published Ironclad net, through `check.play` (the env driver's own loop),
and replays every run's choices in the training env (`RsE2EEnv`) to read the
env's own return. It writes `chapter_data.json`.

`render` reads `chapter_data.json` and rewrites the plots inside
`chapter.html` between `<!-- plot:NAME -->` and `<!-- /plot:NAME -->`.
"""

from __future__ import annotations

import argparse
import collections
import json
import math
import re
import statistics
import sys
import time
from pathlib import Path
from typing import Any

import tourlib

import check

HERE = Path(__file__).resolve().parent
DATA = HERE / "chapter_data.json"
CHAPTER = HERE / "chapter.html"
SHOWCASE_SEED = tourlib.PANEL_A[0]


# ── collect ─────────────────────────────────────────────────────────────────


def env_return(seed: int, chosen: list[int]) -> dict[str, Any]:
    """Replay `chosen` in the training env and read its own bookkeeping: the
    return (sum of rewards), the stored rows and the bosses beaten."""
    from ppo.e2e.rsenv import RsDecision, RsE2EEnv

    env = RsE2EEnv(seed)
    d = env.reset()
    it = iter(chosen)
    while isinstance(d, RsDecision):
        d = env.step(next(it))
    return {
        "G0": float(d.total_reward) + float(d.unassigned_reward),
        "rows": int(d.rows),
        "steps": int(d.steps),
        "bosses": int(d.bosses),
        "outcome": d.outcome,
        "floor": int(d.floor),
    }


def run_record(ep: check.Episode) -> dict[str, Any]:
    decs = ep.decisions
    first_dec_at_floor: dict[int, int] = {}
    for i, s in enumerate(decs):
        first_dec_at_floor.setdefault(int(s.floor), i)
    forced = collections.Counter(
        ("fight " if s.combat else "between ") + str((s.action or {}).get("action"))
        for s in ep.steps
        if s.forced
    )
    return {
        "seed": ep.seed,
        "T": len(decs),
        "forced": ep.n_forced,
        "fight_decisions": sum(s.combat for s in decs),
        "floor": int(ep.end.get("floor") or 0),
        "outcome": ep.end.get("outcome"),
        "classes": dict(collections.Counter(s.n_classes for s in decs)),
        "members": dict(collections.Counter(s.n_members for s in decs)),
        "by_type": dict(collections.Counter(s.dtype for s in decs)),
        "forced_actions": dict(forced),
        "first_decision_at_floor": first_dec_at_floor,
    }


def collect(n: int, workers: int) -> None:
    import sts2_rs
    from ppo.e2e import evaluate

    evaluate.select_campaign_build()
    student = sts2_rs.E2eStudent(str(tourlib.weight(check.NET)))
    seeds = list(tourlib.PANEL_A[:n])
    t0 = time.time()
    out: dict[str, Any] = {"seeds": n, "panel": "PANEL_A", "net": check.NET}
    players: dict[str, list[dict[str, Any]]] = {"random": [], "net": []}

    for seed in seeds:
        ep = check.play(seed, check.random_chooser(seed))
        rec = run_record(ep)
        rec["env"] = env_return(seed, ep.chosen)
        players["random"].append(rec)
    t1 = time.time()
    runs = tourlib.selfplay_many(student, seeds, workers=workers)
    t2 = time.time()
    for seed, r in zip(seeds, runs, strict=True):
        ep = check.play(seed, check.scripted_chooser(r["chosen"]))
        if ep.chosen != list(r["chosen"]) or ep.end.get("outcome") != r["outcome"]:
            raise SystemExit(f"seed {seed}: the logged replay left the self-play run")
        rec = run_record(ep)
        rec["env"] = env_return(seed, ep.chosen)
        players["net"].append(rec)
        if seed == SHOWCASE_SEED:
            out["showcase"] = {
                "seed": seed,
                "steps": [
                    [
                        int(s.forced),
                        int(s.combat),
                        s.floor if s.floor is not None else -1,
                        s.n_classes,
                        s.n_members,
                    ]
                    for s in ep.steps
                ],
            }
    t3 = time.time()
    for name, recs in players.items():
        for rec in recs:
            e = rec["env"]
            if (
                e["rows"] != rec["T"]
                or e["outcome"] != rec["outcome"]
                or e["floor"] != rec["floor"]
            ):
                raise SystemExit(
                    f"{name} seed {rec['seed']}: driver loop and training env disagree"
                )
    out["players"] = players
    out["seconds"] = {
        "random": round(t1 - t0, 1),
        "net_selfplay": round(t2 - t1, 1),
        "net_replay": round(t3 - t2, 1),
        "workers": workers,
    }
    DATA.write_text(json.dumps(out, separators=(",", ":")), encoding="utf-8")
    print(f"wrote {DATA.name}: {n} seeds, {out['seconds']}")


# ── summary numbers the chapter quotes ──────────────────────────────────────


def summary(recs: list[dict[str, Any]]) -> dict[str, Any]:
    T = [r["T"] for r in recs]
    G = [r["env"]["G0"] for r in recs]
    n = len(recs)
    dec = sum(T)
    steps = dec + sum(r["forced"] for r in recs)
    classes = collections.Counter()
    members = collections.Counter()
    for r in recs:
        classes.update({int(k): v for k, v in r["classes"].items()})
        members.update({int(k): v for k, v in r["members"].items()})
    sd = statistics.stdev(G) if n > 1 else 0.0
    return {
        "runs": n,
        "T_mean": statistics.fmean(T),
        "T_median": statistics.median(T),
        "T_min": min(T),
        "T_max": max(T),
        "fight_share": sum(r["fight_decisions"] for r in recs) / dec,
        "forced_share": (steps - dec) / steps,
        "classes_median": _wmedian(classes),
        "classes_mean": _wmean(classes),
        "members_mean": _wmean(members),
        "floor_mean": statistics.fmean(r["floor"] for r in recs),
        "wins": sum(r["outcome"] == "won" for r in recs),
        "J_hat": statistics.fmean(G),
        "J_se": sd / math.sqrt(n),
        "G_sd": sd,
        "G_win": statistics.fmean([g for g, r in zip(G, recs) if r["outcome"] == "won"] or [0.0]),
        "G_loss": statistics.fmean([g for g, r in zip(G, recs) if r["outcome"] != "won"] or [0.0]),
        "win_rate": sum(r["outcome"] == "won" for r in recs) / n,
        "forced_per_run": statistics.fmean(r["forced"] for r in recs),
        "forced_fight_share": sum(
            v for r in recs for k, v in r["forced_actions"].items() if k.startswith("fight ")
        )
        / max(1, steps - dec),
        "merge_ratio": _wmean(members) / _wmean(classes),
    }


def _wmean(c: collections.Counter) -> float:
    return sum(k * v for k, v in c.items()) / sum(c.values())


def _wmedian(c: collections.Counter) -> float:
    total = sum(c.values())
    run = 0
    for k in sorted(c):
        run += c[k]
        if run * 2 >= total:
            return float(k)
    return 0.0


# ── render ──────────────────────────────────────────────────────────────────

W, H = 560, 300
ML, MR, MT, MB = 56, 16, 18, 46


class Axes:
    """A linear or log10 x axis and a linear y axis inside the plot box."""

    def __init__(self, x0, x1, y0, y1, logx=False, w=W, h=H):
        self.x0, self.x1, self.y0, self.y1, self.logx, self.w, self.h = x0, x1, y0, y1, logx, w, h

    def X(self, x):
        f = (
            (math.log10(x) - math.log10(self.x0)) / (math.log10(self.x1) - math.log10(self.x0))
            if self.logx
            else (x - self.x0) / (self.x1 - self.x0)
        )
        return ML + f * (self.w - ML - MR)

    def Y(self, y):
        return self.h - MB - (y - self.y0) / (self.y1 - self.y0) * (self.h - MT - MB)


def _frame(ax: Axes, xticks, yticks, xlabel, ylabel, xfmt=str, yfmt=str) -> list[str]:
    o = []
    for t in yticks:
        y = ax.Y(t)
        o.append(f'<line class="grid" x1="{ML}" x2="{ax.w - MR}" y1="{y:.1f}" y2="{y:.1f}"/>')
        o.append(
            f'<text class="muted" x="{ML - 6}" y="{y + 4:.1f}" text-anchor="end">{yfmt(t)}</text>'
        )
    for t in xticks:
        x = ax.X(t)
        o.append(
            f'<line class="ink" x1="{x:.1f}" x2="{x:.1f}" y1="{ax.h - MB}" y2="{ax.h - MB + 4}"/>'
        )
        o.append(
            f'<text class="muted" x="{x:.1f}" y="{ax.h - MB + 17}" text-anchor="middle">{xfmt(t)}</text>'
        )
    o.append(f'<line class="ink" x1="{ML}" x2="{ax.w - MR}" y1="{ax.h - MB}" y2="{ax.h - MB}"/>')
    o.append(
        f'<text x="{(ML + ax.w - MR) / 2:.0f}" y="{ax.h - 8}" text-anchor="middle">{xlabel}</text>'
    )
    o.append(
        f'<text x="14" y="{(MT + ax.h - MB) / 2:.0f}" text-anchor="middle" '
        f'transform="rotate(-90 14 {(MT + ax.h - MB) / 2:.0f})">{ylabel}</text>'
    )
    return o


def _svg(body: list[str], label: str, w=W, h=H) -> str:
    return (
        f'<svg viewBox="0 0 {w} {h}" role="img" aria-label="{label}">\n  '
        + "\n  ".join(body)
        + "\n</svg>"
    )


def _legend(items, x, y) -> list[str]:
    o = []
    for i, (cls, text) in enumerate(items):
        yy = y + i * 18
        o.append(f'<rect class="{cls}" x="{x}" y="{yy - 9}" width="12" height="10"/>')
        o.append(f'<text x="{x + 18}" y="{yy}">{text}</text>')
    return o


def plot_lengths(d) -> str:
    """Histogram of episode length T on a log axis, both players."""
    edges = [10 ** (0.6 + i * 0.1) for i in range(25)]  # ~4 to 1000
    ax = Axes(edges[0], edges[-1], 0, 1)
    counts = {}
    for name in ("random", "net"):
        T = [r["T"] for r in d["players"][name]]
        c = [sum(edges[i] <= t < edges[i + 1] for t in T) / len(T) for i in range(len(edges) - 1)]
        counts[name] = c
    top = max(max(c) for c in counts.values())
    ax.y1 = math.ceil(top * 10) / 10
    o = _frame(
        ax,
        [10, 30, 100, 300, 1000],
        [i / 10 for i in range(int(ax.y1 * 10) + 1)],
        "decisions in the episode, T (log scale)",
        "share of runs",
        yfmt=lambda t: f"{t:.1f}",
    )
    for name, cls in (("random", "f2"), ("net", "f1")):
        for i, v in enumerate(counts[name]):
            if v:
                x0, x1 = ax.X(edges[i]) + 1, ax.X(edges[i + 1]) - 1
                o.append(
                    f'<rect class="{cls}" x="{x0:.1f}" y="{ax.Y(v):.1f}" '
                    f'width="{x1 - x0:.1f}" height="{ax.Y(0) - ax.Y(v):.1f}" opacity="0.85"/>'
                )
    o += _legend([("f2", "random play"), ("f1", "network")], ML + 14, MT + 14)
    return _svg(o, "Histogram of decisions per episode for random play and the network")


def plot_survival(d) -> str:
    """Share of runs still alive at each floor."""
    ax = Axes(0, 50, 0, 1)
    o = _frame(
        ax,
        range(0, 51, 10),
        [0, 0.25, 0.5, 0.75, 1],
        "floor",
        "share of runs reaching it",
        yfmt=lambda t: f"{t:g}",
    )
    for name, cls in (("random", "l2"), ("net", "l1")):
        floors = [r["floor"] for r in d["players"][name]]
        pts = []
        for f in range(0, 50):
            v = sum(x >= f for x in floors) / len(floors)
            pts.append(f"{ax.X(f):.1f},{ax.Y(v):.1f}")
            pts.append(f"{ax.X(f + 1):.1f},{ax.Y(v):.1f}")
        o.append(f'<polyline class="{cls}" stroke-width="2.2" points="{" ".join(pts)}"/>')
    o += _legend([("f2", "random play"), ("f1", "network")], ax.w - MR - 130, MT + 14)
    return _svg(o, "Share of runs that reach each floor, random play against the network")


def plot_classes(d) -> str:
    """Distribution of options per decision before and after merging (network)."""
    recs = d["players"]["net"]
    cl, me = collections.Counter(), collections.Counter()
    for r in recs:
        cl.update({int(k): v for k, v in r["classes"].items()})
        me.update({int(k): v for k, v in r["members"].items()})
    ncl, nme = sum(cl.values()), sum(me.values())
    xmax = 30
    ax = Axes(0.5, xmax + 0.5, 0, 0.25)
    top = max(max(v / ncl for v in cl.values()), max(v / nme for v in me.values()))
    ax.y1 = math.ceil(top * 20) / 20
    o = _frame(
        ax,
        [2, 5, 10, 15, 20, 25, 30],
        [i / 20 for i in range(int(ax.y1 * 20) + 1)],
        "options at the decision",
        "share of decisions",
        yfmt=lambda t: f"{t:.2f}",
    )
    bw = (ax.X(2) - ax.X(1)) / 2 - 1
    for k in range(1, xmax + 1):
        for counter, n, cls, dx in ((me, nme, "f4", -bw), (cl, ncl, "f1", 0)):
            v = counter.get(k, 0) / n
            if v:
                o.append(
                    f'<rect class="{cls}" x="{ax.X(k) + dx:.1f}" y="{ax.Y(v):.1f}" '
                    f'width="{bw:.1f}" height="{ax.Y(0) - ax.Y(v):.1f}"/>'
                )
    tail_c = sum(v for k, v in cl.items() if k > xmax) / ncl
    tail_m = sum(v for k, v in me.items() if k > xmax) / nme
    o.append(
        f'<text class="muted" x="{ax.w - MR}" y="{MT + 50}" text-anchor="end">'
        f"beyond 30: {tail_m:.1%} of legal-action counts, {tail_c:.1%} of class counts</text>"
    )
    o += _legend(
        [("f4", "legal actions |A(s)|"), ("f1", "merged classes")], ax.w - MR - 170, MT + 10
    )
    return _svg(o, "Distribution of legal actions and merged classes per network decision")


def plot_types(d) -> str:
    """Decisions per run by decision type (network), horizontal bars."""
    recs = d["players"]["net"]
    c = collections.Counter()
    for r in recs:
        c.update(r["by_type"])
    rows = c.most_common()
    n = len(recs)
    h = 30 + 20 * len(rows) + 40
    left = 150
    w = W
    vmax = max(v for _, v in rows) / n
    scale = 10 ** math.floor(math.log10(vmax))
    o = []
    # log-scale bars so the rare between-fight types stay visible
    lo, hi = 0.1, 10 ** math.ceil(math.log10(vmax))

    def X(v):
        return left + (math.log10(max(v, lo)) - math.log10(lo)) / (
            math.log10(hi) - math.log10(lo)
        ) * (w - left - 60)

    t = lo
    while t <= hi * 1.001:
        x = X(t)
        o.append(f'<line class="grid" x1="{x:.1f}" x2="{x:.1f}" y1="20" y2="{h - 40}"/>')
        o.append(f'<text class="muted" x="{x:.1f}" y="{h - 26}" text-anchor="middle">{t:g}</text>')
        t *= 10
    for i, (name, v) in enumerate(rows):
        y = 26 + 20 * i
        per = v / n
        cls = "f1" if name.startswith("COMBAT") else "f2"
        o.append(f'<text x="{left - 8}" y="{y + 11}" text-anchor="end">{name}</text>')
        o.append(
            f'<rect class="{cls}" x="{left}" y="{y}" width="{X(per) - left:.1f}" height="14"/>'
        )
        o.append(f'<text class="muted" x="{X(per) + 5:.1f}" y="{y + 11}">{per:.3g}</text>')
    o.append(
        f'<text x="{(left + w - 60) / 2:.0f}" y="{h - 8}" text-anchor="middle">'
        f"decisions per run (log scale)</text>"
    )
    _ = scale
    return _svg(o, "Network decisions per run by decision type", w, h)


def plot_returns(d) -> str:
    """Per-run return G_0 for both players, as a strip of points with the mean."""
    ax = Axes(-0.05, 1.25, 0, 1, w=W, h=200)
    o = _frame(
        ax, [0, 0.25, 0.5, 0.75, 1, 1.25], [], "return of the run, G₀", "", xfmt=lambda t: f"{t:g}"
    )
    for row, (name, cls, fcls) in enumerate((("random", "l2", "f2"), ("net", "l1", "f1"))):
        y0 = 50 + row * 60
        o.append(
            f'<text x="{ML}" y="{y0 - 18}">{"random play" if name == "random" else "network"}</text>'
        )
        G = sorted(r["env"]["G0"] for r in d["players"][name])
        for i, g in enumerate(G):
            jitter = ((i * 37) % 23 - 11) * 1.0
            o.append(
                f'<circle class="{fcls}" cx="{ax.X(g):.1f}" cy="{y0 + jitter:.1f}" r="2.4" opacity="0.55"/>'
            )
        m = statistics.fmean(G)
        o.append(
            f'<line class="ink" stroke-width="2" x1="{ax.X(m):.1f}" x2="{ax.X(m):.1f}" '
            f'y1="{y0 - 16}" y2="{y0 + 16}"/>'
        )
        o.append(f'<text x="{ax.X(m) + 6:.1f}" y="{y0 - 6}">Ĵ = {m:.3f}</text>')
    return _svg(o, "Per-run returns for random play and the network with their means", W, 200)


def plot_showcase(d) -> str:
    """One network episode: floor against decision index, fights and between-fight
    decisions coloured, forced steps ticked below."""
    st = d["showcase"]["steps"]
    decs = [s for s in st if not s[0]]
    T = len(decs)
    ax = Axes(0, T, 0, 50, h=260)
    o = _frame(ax, range(0, T + 1, 100), range(0, 51, 10), "decision index t", "floor")
    t = 0
    for s in st:
        forced, combat, floor = s[0], s[1], s[2]
        if forced:
            x = ax.X(t)
            o.append(
                f'<line class="l4" x1="{x:.1f}" x2="{x:.1f}" y1="{ax.h - MB + 22}" '
                f'y2="{ax.h - MB + 28}"/>'
            )
            continue
        o.append(
            f'<circle class="{"f1" if combat else "f2"}" cx="{ax.X(t):.1f}" '
            f'cy="{ax.Y(floor):.1f}" r="{1.6 if combat else 2.6}"/>'
        )
        t += 1
    o += _legend(
        [
            ("f1", "decision inside a fight"),
            ("f2", "decision between fights"),
            ("f4", "forced step (tick under the axis)"),
        ],
        ML + 10,
        MT + 10,
    )
    return _svg(o, "Floor against decision index for one network episode", W, 260)


def table_forced(d) -> str:
    """The most common forced steps of the network, per run."""
    recs = d["players"]["net"]
    c = collections.Counter()
    for r in recs:
        c.update(r["forced_actions"])
    total = sum(c.values())
    rows = []
    for k, v in c.most_common(8):
        where, act = k.split(" ", 1)
        rows.append(
            f"<tr><td>{where}</td><td><code>{act}</code></td>"
            f'<td class="num">{v / len(recs):.2f}</td><td class="num">{v / total:.1%}</td></tr>'
        )
    return (
        '<table><thead><tr><th>where</th><th>forced action</th><th class="num">per run</th>'
        '<th class="num">share of forced steps</th></tr></thead><tbody>'
        + "".join(rows)
        + "</tbody></table>"
    )


PLOTS = {
    "forced": table_forced,
    "lengths": plot_lengths,
    "survival": plot_survival,
    "classes": plot_classes,
    "types": plot_types,
    "returns": plot_returns,
    "showcase": plot_showcase,
}


def render() -> None:
    d = json.loads(DATA.read_text(encoding="utf-8"))
    html = CHAPTER.read_text(encoding="utf-8")
    for name, fn in PLOTS.items():
        pat = re.compile(rf"(<!-- plot:{name} -->)(.*?)(<!-- /plot:{name} -->)", re.S)
        if not pat.search(html):
            print(f"no marker for {name}", file=sys.stderr)
            continue
        html = pat.sub(lambda m, s=fn(d): m.group(1) + "\n" + s + "\n" + m.group(3), html)
    sums = {name: summary(d["players"][name]) for name in ("random", "net")}
    st = d["showcase"]["steps"]
    sums["showcase"] = {
        "T": sum(1 for x in st if not x[0]),
        "forced": sum(x[0] for x in st),
        "seed": d["showcase"]["seed"],
    }
    sums["panel"] = {"runs": d["seeds"]}

    def fill(m: re.Match) -> str:
        who, key = m.group(2).split(".")
        v = sums[who][key]
        fmt = m.group(3)
        if fmt == "%":
            txt = f"{v:.1%}"
        elif fmt:
            txt = format(v, fmt)
        else:
            txt = str(v)
        return f"{m.group(1)}{txt}</span>"

    html = re.sub(
        r'(<span class="v" data-k="([a-z]+\.[A-Za-z_]+)"(?: data-f="([^"]*)")?>)[^<]*</span>',
        fill,
        html,
    )
    CHAPTER.write_text(html, encoding="utf-8")
    for name in ("random", "net"):
        print(
            name,
            json.dumps(
                {
                    k: round(v, 4) if isinstance(v, float) else v
                    for k, v in summary(d["players"][name]).items()
                }
            ),
        )


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("collect")
    c.add_argument("--seeds", type=int, default=200)
    c.add_argument("--workers", type=int, default=3)
    sub.add_parser("render")
    sub.add_parser("summary")
    a = ap.parse_args(argv)
    if a.cmd == "collect":
        collect(a.seeds, a.workers)
    elif a.cmd == "render":
        render()
    else:
        d = json.loads(DATA.read_text(encoding="utf-8"))
        for name in ("random", "net"):
            print(name, summary(d["players"][name]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
