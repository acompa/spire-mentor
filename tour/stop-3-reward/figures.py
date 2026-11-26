"""Data and plots for chapter.html.

    tour/run stop-3-reward/figures.py collect   # play the panel, write chapter_data.json
    tour/run stop-3-reward/figures.py render    # redraw the plots inside chapter.html
    tour/run stop-3-reward/figures.py rescore --gamma 0.99   # other reward rules, offline

`collect` plays the first 300 seeds of PANEL_A with the published network and
with random play, through `check.play`, so every number is the training env's
own reward. `render` reads only the JSON and rewrites each region of
chapter.html between `<!-- fig:NAME -->` and `<!-- /fig:NAME -->`.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import math
import statistics
import sys
import time
from collections import Counter
from dataclasses import asdict
from pathlib import Path

STOP = Path(__file__).resolve().parent
DATA = STOP / "chapter_data.json"
CHAPTER = STOP / "chapter.html"

_spec = importlib.util.spec_from_file_location("stop3_check", STOP / "check.py")
check = importlib.util.module_from_spec(_spec)
sys.modules[_spec.name] = check
_spec.loader.exec_module(check)


# ── collect ─────────────────────────────────────────────────────────────────


def summary_row(policy: str, r) -> dict:
    return {
        "policy": policy,
        "seed": r.seed,
        "outcome": r.outcome,
        "won": r.won,
        "floor": r.floor,
        "bosses": r.bosses,
        "decisions": r.decisions,
        "ret": round(r.ret, 6),
        "floors_part": round(r.part("floors"), 6),
        "boss_part": round(r.part("boss"), 6),
        "win_part": round(r.part("win"), 6),
        "reward_rows": len(r.ledger),
        "last_reward_row": r.ledger[-1].row if r.ledger else None,
        "hp": r.hp,
        "max_hp": r.max_hp,
        "relics": r.relics if r.floor > 46 else None,
    }


def collect(n: int, workers: int) -> None:
    seeds = list(check.tourlib.PANEL_A[:n])
    t0 = time.time()
    check._select_build()
    net = check.play_many(check.play_net, seeds, workers)
    rnd = check.play_many(check.play_random, seeds, workers)
    for r in net + rnd:
        check.verify(r)
    lost = [r for r in net if not r.won and r.outcome != "error"]
    ledgers = {
        "net_won": net[0] if net[0].won else next(r for r in net if r.won),
        "net_lost": max(lost, key=lambda r: r.floor) if lost else None,
        "random": max(rnd, key=lambda r: r.floor),
    }
    out = {
        "panel": "PANEL_A",
        "seeds": [seeds[0], seeds[-1]],
        "constants": dict(zip(("R_WIN", "R_BOSS", "R_FLOOR"), check.constants())),
        "runs": [summary_row("net", r) for r in net] + [summary_row("random", r) for r in rnd],
        "ledgers": {
            k: None if r is None else {"seed": r.seed, "outcome": r.outcome, "decisions": r.decisions,
                                       "ledger": [asdict(e) for e in r.ledger]}
            for k, r in ledgers.items()
        },
        "seconds": round(time.time() - t0),
        "workers": workers,
    }
    DATA.write_text(json.dumps(out, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {DATA.name}: {len(out['runs'])} runs in {out['seconds']} s")


# ── render: SVG helpers ─────────────────────────────────────────────────────

W, H = 560, 300
ML, MR, MT, MB = 58, 16, 18, 46


def esc(s: str) -> str:
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def axes(xmin, xmax, ymin, ymax, xticks, yticks, xlabel, ylabel, fx=None, fy=None, xlog=False):
    """An x/y frame. Returns (svg parts, x(), y())."""
    pw, ph = W - ML - MR, H - MT - MB

    def x(v):
        if xlog:
            return ML + pw * (math.log10(v) - math.log10(xmin)) / (math.log10(xmax) - math.log10(xmin))
        return ML + pw * (v - xmin) / (xmax - xmin)

    def y(v):
        return MT + ph * (1 - (v - ymin) / (ymax - ymin))

    fx = fx or (lambda v: f"{v:g}")
    fy = fy or (lambda v: f"{v:g}")
    p = []
    for v in yticks:
        p.append(f'<line class="grid" x1="{ML}" x2="{W - MR}" y1="{y(v):.1f}" y2="{y(v):.1f}"/>')
        p.append(f'<text class="muted" x="{ML - 6}" y="{y(v) + 4:.1f}" text-anchor="end">{esc(fy(v))}</text>')
    for v in xticks:
        p.append(f'<text class="muted" x="{x(v):.1f}" y="{H - MB + 16}" text-anchor="middle">{esc(fx(v))}</text>')
    p.append(f'<line class="ink" x1="{ML}" x2="{W - MR}" y1="{H - MB}" y2="{H - MB}"/>')
    p.append(f'<line class="ink" x1="{ML}" x2="{ML}" y1="{MT}" y2="{H - MB}"/>')
    p.append(f'<text x="{ML + pw / 2:.0f}" y="{H - 8}" text-anchor="middle">{esc(xlabel)}</text>')
    p.append(f'<text x="14" y="{MT + ph / 2:.0f}" text-anchor="middle" '
             f'transform="rotate(-90 14 {MT + ph / 2:.0f})">{esc(ylabel)}</text>')
    return p, x, y


def svg(parts, label):
    body = "\n".join(parts)
    return (f'<svg viewBox="0 0 {W} {H}" role="img" aria-label="{esc(label)}">\n{body}\n</svg>')


def path(points, x, y):
    return "M" + " L".join(f"{x(a):.1f} {y(b):.1f}" for a, b in points)


def step_points(ledger, start=0.0, skip_win=False):
    """Cumulative return as a step function of the stored row."""
    pts, run = [(0, start)], start
    for e in ledger:
        amt = e["amount"] - (e["win"] if skip_win else 0.0)
        pts.append((e["row"], run))
        run += amt
        pts.append((e["row"], run))
    return pts


# ── render: the plots ───────────────────────────────────────────────────────


def fig_ledger(d) -> str:
    L = d["ledgers"]
    won, lost, rnd = L["net_won"], L["net_lost"], L["random"]
    xmax = max(won["decisions"], lost["decisions"] if lost else 0) + 20
    xmax = int(math.ceil(xmax / 100) * 100)
    p, x, y = axes(0, xmax, 0, 0.30, range(0, xmax + 1, 100), [0, 0.05, 0.10, 0.15, 0.20, 0.25, 0.30],
                   "stored decision (row) in the run", "return so far, win bonus left out",
                   fy=lambda v: f"{v:.2f}")
    series = [("l1", won, f"network, seed {won['seed']}, won"),
              ("l2", lost, f"network, seed {lost['seed']}, lost" if lost else ""),
              ("l3", rnd, f"random, seed {rnd['seed']}, lost")]
    for i, (cls, run, name) in enumerate(series):
        if run is None:
            continue
        pts = step_points(run["ledger"], skip_win=True)
        end = run["decisions"]
        pts.append((end, pts[-1][1]))
        p.append(f'<path class="{cls}" stroke-width="2" d="{path(pts, x, y)}"/>')
        p.append(f'<line class="{cls}" stroke-width="2" x1="{ML + 12}" x2="{ML + 32}" '
                 f'y1="{MT + 12 + 16 * i}" y2="{MT + 12 + 16 * i}"/>')
        p.append(f'<text x="{ML + 38}" y="{MT + 16 + 16 * i}">{esc(name)}</text>')
    w = won["ledger"][-1]
    p.append(f'<text x="{x(w["row"]) - 4:.1f}" y="{y(0.262):.1f}" text-anchor="end">+1.0 here →</text>')
    return svg(p, "Return so far against decision number for three runs.")


def fig_discount(d) -> str:
    wins = sorted(r["last_reward_row"] for r in d["runs"] if r["policy"] == "net" and r["won"])
    q = lambda f: wins[min(len(wins) - 1, int(f * len(wins)))]
    t10, t50, t90 = q(0.1), q(0.5), q(0.9)
    gammas = [1 - 10 ** (-k / 20) for k in range(20, 81)]  # 1-γ from 0.1 down to 1e-4
    xs = [1 - g for g in gammas]
    p, x, y = axes(1e-4, 0.1, 0, 1, [1e-4, 1e-3, 1e-2, 1e-1], [0, 0.25, 0.5, 0.75, 1.0],
                   "1 − γ (log scale)", "value of the +1.0 win, seen from row 0",
                   fx=lambda v: {1e-4: "0.0001", 1e-3: "0.001", 1e-2: "0.01", 1e-1: "0.1"}[v],
                   xlog=True)
    band = [(a, (1 - a) ** t10) for a in xs] + [(a, (1 - a) ** t90) for a in reversed(xs)]
    p.append(f'<path class="f1" opacity="0.18" d="{path(band, x, y)} Z"/>')
    p.append(f'<path class="l1" stroke-width="2" d="{path([(a, (1 - a) ** t50) for a in xs], x, y)}"/>')
    for g, lab in ((0.99, "γ = 0.99"), (0.999, "γ = 0.999")):
        a = 1 - g
        p.append(f'<line class="l4" stroke-dasharray="4 3" x1="{x(a):.1f}" x2="{x(a):.1f}" y1="{MT}" y2="{H - MB}"/>')
        p.append(f'<text class="muted" x="{x(a) + 4:.1f}" y="{MT + 12}">{lab}</text>')
        p.append(f'<circle class="f2" r="3.5" cx="{x(a):.1f}" cy="{y(g ** t50):.1f}"/>')
        p.append(f'<text x="{x(a) + 6:.1f}" y="{y(g ** t50) - 6:.1f}">{g ** t50:.3f}</text>')
    p.append(f'<text x="{x(1.3e-4):.1f}" y="{y(0.9):.1f}">γ = 1 keeps it at 1.0</text>')
    return svg(p, "Discounted value of the win at the first decision, as a function of gamma.")


def fig_deaths(d) -> str:
    lost = [r for r in d["runs"] if r["policy"] == "net" and not r["won"] and r["outcome"] != "error"]
    c = Counter(r["floor"] for r in lost)
    top = max(c.values()) if c else 1
    ymax = int(math.ceil(top / 5) * 5)
    p, x, y = axes(0, 49, 0, ymax, [1, 8, 16, 24, 31, 38, 45], range(0, ymax + 1, 5),
                   "floor on which the run ended", "network runs lost (of the panel)")
    bw = (x(1) - x(0)) * 0.8
    for f, k in sorted(c.items()):
        p.append(f'<rect class="f2" x="{x(f) - bw / 2:.1f}" y="{y(k):.1f}" width="{bw:.1f}" height="{y(0) - y(k):.1f}"/>')
    for f, lab, dy in ((16, "act-1 boss", 10), (31, "act-2 boss", 10), (45, "act-3 bosses (floors 45, 46)", 27)):
        p.append(f'<line class="l4" stroke-dasharray="4 3" x1="{x(f):.1f}" x2="{x(f):.1f}" y1="{MT}" y2="{H - MB}"/>')
        p.append(f'<text class="muted" x="{x(f) - 4:.1f}" y="{MT + dy}" text-anchor="end">{lab}</text>')
    return svg(p, "Histogram of the floor on which the network's lost runs ended.")


def fig_parts(d) -> str:
    rows = []
    for pol, name in (("net", "network"), ("random", "random play")):
        rs = [r for r in d["runs"] if r["policy"] == pol and r["outcome"] != "error"]
        n = len(rs)
        rows.append((name, sum(r["floors_part"] for r in rs) / n, sum(r["boss_part"] for r in rs) / n,
                     sum(r["win_part"] for r in rs) / n))
    p, x, y = axes(0, 1.2, 0, 2, [0, 0.2, 0.4, 0.6, 0.8, 1.0, 1.2], [], "mean return per run (reward units)", "",
                   fy=lambda v: "")
    bh = 34
    for i, (name, f, b, w) in enumerate(rows):
        yc = MT + 50 + i * 100
        x0 = 0.0
        for val, cls in ((f, "f1"), (b, "f3"), (w, "f2")):
            p.append(f'<rect class="{cls}" x="{x(x0):.1f}" y="{yc:.1f}" width="{max(0.6, x(x0 + val) - x(x0)):.1f}" height="{bh}"/>')
            x0 += val
        p.append(f'<text x="{ML + 4}" y="{yc - 6}">{esc(name)}: {x0:.4f}</text>')
    lx = ML + 250
    for i, (cls, lab) in enumerate((("f1", "floors"), ("f3", "bosses"), ("f2", "win"))):
        p.append(f'<rect class="{cls}" x="{lx + 80 * i}" y="{H - MB - 28}" width="12" height="12"/>')
        p.append(f'<text x="{lx + 80 * i + 16}" y="{H - MB - 18}">{lab}</text>')
    return svg(p, "Mean return split into floors, bosses and win, for the network and random play.")


def stats(d) -> dict:
    """The numbers the chapter quotes, printed for checking the prose."""
    out = {}
    for pol in ("net", "random"):
        rs = [r for r in d["runs"] if r["policy"] == pol and r["outcome"] != "error"]
        rets = [r["ret"] for r in rs]
        out[pol] = {
            "n": len(rs), "errors": sum(r["outcome"] == "error" for r in d["runs"] if r["policy"] == pol),
            "wins": sum(r["won"] for r in rs), "capped": sum(r["outcome"] == "capped" for r in rs),
            "mean": statistics.mean(rets), "sd": statistics.pstdev(rets),
            "floors": statistics.mean(r["floors_part"] for r in rs),
            "boss": statistics.mean(r["boss_part"] for r in rs),
            "win": statistics.mean(r["win_part"] for r in rs),
            "decisions_median": statistics.median(r["decisions"] for r in rs),
            "reward_rows_mean": statistics.mean(r["reward_rows"] for r in rs),
            "reward_row_share": sum(r["reward_rows"] for r in rs) / sum(r["decisions"] for r in rs),
            "floor_median": statistics.median(r["floor"] for r in rs),
            "floor_max": max(r["floor"] for r in rs),
            "lost_ret_max": max((r["ret"] for r in rs if not r["won"]), default=None),
        }
        won = [r for r in rs if r["won"]]
        if won:
            out[pol]["won_ret"] = Counter(round(r["ret"], 3) for r in won).most_common()
            out[pol]["won_floor"] = Counter(r["floor"] for r in won).most_common()
            out[pol]["won_hp_frac"] = statistics.median(r["hp"] / r["max_hp"] for r in won)
            out[pol]["won_hp_frac_q"] = [round(q, 3) for q in statistics.quantiles((r["hp"] / r["max_hp"] for r in won), n=10, method="inclusive")]
            out[pol]["win_last_row_median"] = statistics.median(r["last_reward_row"] for r in won)
            out[pol]["long_map_relics"] = [r["relics"] for r in won if r["relics"]][:5]
        lost = [r for r in rs if not r["won"]]
        out[pol]["lost_floor_top"] = Counter(r["floor"] for r in lost).most_common(8)
    return out


def rescore(d, gamma: float) -> dict:
    """Score the saved runs under other reward rules, without replaying them.

    win only: the sparse rule (R_WIN on a win, nothing else); floors only;
    and the full rule discounted by `gamma`, for the three saved ledgers."""
    out = {}
    for pol in ("net", "random"):
        rs = [r for r in d["runs"] if r["policy"] == pol and r["outcome"] != "error"]
        n = len(rs)
        for name, f in (("full", lambda r: r["ret"]), ("win_only", lambda r: r["win_part"]),
                        ("floors_only", lambda r: r["floors_part"])):
            v = [f(r) for r in rs]
            m = sum(v) / n
            out[f"{pol}/{name}"] = {"mean": round(m, 5), "sd": round(statistics.pstdev(v), 5),
                                    "nonzero_runs": sum(x != 0 for x in v), "runs": n}
    for k, L in d["ledgers"].items():
        if L is None:
            continue
        g = sum(e["amount"] * gamma ** e["row"] for e in L["ledger"])
        out[f"ledger/{k}"] = {"seed": L["seed"], "undiscounted": round(sum(e["amount"] for e in L["ledger"]), 5),
                              f"discounted_{gamma}": round(g, 5)}
    return out


FIGS = {"ledger": fig_ledger, "discount": fig_discount, "deaths": fig_deaths, "parts": fig_parts}


def render() -> None:
    d = json.loads(DATA.read_text(encoding="utf-8"))
    html = CHAPTER.read_text(encoding="utf-8")
    for name, fn in FIGS.items():
        a, b = f"<!-- fig:{name} -->", f"<!-- /fig:{name} -->"
        i, j = html.find(a), html.find(b)
        if i < 0 or j < 0:
            raise SystemExit(f"chapter.html has no {a} ... {b} region")
        html = html[: i + len(a)] + "\n" + fn(d) + "\n" + html[j:]
    CHAPTER.write_text(html, encoding="utf-8")
    print(json.dumps(stats(d), indent=1))


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("what", choices=("collect", "render", "stats", "rescore"))
    ap.add_argument("--runs", type=int, default=300)
    ap.add_argument("--workers", type=int, default=3)
    ap.add_argument("--gamma", type=float, default=0.999, help="rescore: discount for the saved ledgers")
    a = ap.parse_args()
    if a.what == "collect":
        collect(a.runs, a.workers)
    elif a.what == "render":
        render()
    elif a.what == "rescore":
        print(json.dumps(rescore(json.loads(DATA.read_text(encoding="utf-8")), a.gamma), indent=1))
    else:
        print(json.dumps(stats(json.loads(DATA.read_text(encoding="utf-8"))), indent=1))


if __name__ == "__main__":
    main()
