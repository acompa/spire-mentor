"""Data behind Chapter 2's figures, and the figures themselves.

    tour/run stop-2-policy/figures.py collect [--seeds N] [--workers K]
        Plays PANEL_A seeds with the published Ironclad net: greedily, then
        sampling at temperatures 0.5, 1 and 2. Writes `figures.json`.
    tour/run stop-2-policy/figures.py svg
        Reads `figures.json` and rewrites every `<!-- fig:NAME -->` block in
        `chapter.html` with a freshly drawn SVG.

Both use `check.play`, the peek/decide/apply loop that `test_check.py` pins to
`e2e_selfplay`.
"""

from __future__ import annotations

import argparse
import json
import math
import re
import statistics
import sys
import time
from collections import defaultdict
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

import numpy as np

import check
import tourlib

STOP = Path(__file__).resolve().parent
DATA = STOP / "figures.json"
CHAPTER = STOP / "chapter.html"
TEMPS = (0.5, 1.0, 2.0)


# ── collect ──


def _job(job: tuple[int, str, float]) -> check.Run:
    seed, mode, temp = job
    return check.play(check._student, seed, mode, temperature=temp)


def wilson(k: int, n: int, z: float = 1.959964) -> tuple[float, float]:
    """95% Wilson score interval for k successes in n trials."""
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return c - h, c + h


def example(student, seed: int) -> dict:
    """The first fight decision with at least three classes on `seed`'s greedy
    run, and the first card reward: logits, labels and token counts."""
    found: dict[str, dict] = {}
    import sts2_rs

    rm = sts2_rs.e2e_new_run(seed)
    drv = sts2_rs.E2eDriver(seed)
    while len(found) < 2:
        peek = drv.peek(rm)
        if peek.kind == "forced":
            f = peek.forced
            drv.apply(rm, f["action"], f["kind"], strict=False)
            continue
        if peek.kind == "terminal":
            break
        d = peek.decision
        key = None
        if "fight" not in found and d["is_combat"] and d["n_classes"] >= 3:
            key = "fight"
        elif "between" not in found and d["decision_type"] == "CARD_REWARD":
            key = "between"
        if key:
            row = student.encode(drv.copy(), drv.copy().peek(rm), rm)
            out = student.decide(drv.copy(), drv.copy().peek(rm), rm)
            state = rm.combat_state().get_state() if d["is_combat"] else None
            found[key] = {
                "decision_type": d["decision_type"], "act": d["act"], "floor": d["floor"],
                "is_combat": d["is_combat"], "n_tok": row["n_tok"],
                "labels": [check.label(d, c, rm) for c in d["classes"]],
                "kinds": [c["kind"] for c in d["classes"]],
                "members": [len(c["actions"]) for c in d["classes"]],
                "logits": [float(x) for x in out["logits"]],
                "greedy": int(out["class"]),
                "state": state,
            }
        out = student.decide(drv, peek, rm)
        ca = peek.class_action(out["class"])
        drv.apply(rm, ca["action"], ca["kind"])
    return found


def summarize(greedy: list[check.Run], sampled: dict[float, list[check.Run]]) -> dict:
    steps = [s for r in greedy for s in r.steps]
    miss = {"fight": sorted(1 - s.top_p for s in steps if s.is_combat),
            "between": sorted(1 - s.top_p for s in steps if not s.is_combat)}
    # Empirical CDF of 1 - p_top on a log grid.
    grid = [10 ** (e / 4) for e in range(-48, 1)]
    cdf = {k: [float(np.searchsorted(v, g, side="right") / len(v)) for g in grid]
           for k, v in miss.items()}

    by_type = defaultdict(list)
    for s in steps:
        by_type[s.dtype].append(s)
    types = []
    for dt, ss in sorted(by_type.items(), key=lambda kv: -len(kv[1])):
        norm = [s.entropy / math.log(s.n_classes) for s in ss if s.n_classes > 1]
        types.append({
            "type": dt, "combat": ss[0].is_combat, "count": len(ss),
            "mean_classes": statistics.mean(s.n_classes for s in ss),
            "median_top": statistics.median(s.top_p for s in ss),
            "share_sure": sum(s.top_p > check.TOP_SURE for s in ss) / len(ss),
            "mean_entropy": statistics.mean(s.entropy for s in ss),
            "mean_norm_entropy": statistics.mean(norm) if norm else 0.0,
        })

    # Along each greedy run: the chance that sampling at T = 1 would have
    # matched greedy for the first k decisions, prod p_top.
    ks = list(range(0, 701, 10))
    surv = []
    for r in greedy:
        lp = np.concatenate([[0.0], np.cumsum(np.log([s.top_p for s in r.steps]))])
        surv.append([float(np.exp(lp[min(k, len(lp) - 1)])) if k < len(lp) else float("nan")
                     for k in ks])
    surv = np.array(surv)
    survival = {"k": ks,
                "median": [float(np.nanmedian(c)) if np.isfinite(c).any() else None
                           for c in surv.T]}
    expected_off = [sum(1 - s.top_p for s in r.steps) for r in greedy]

    n = len(greedy)
    gw = {r.seed: r.won for r in greedy}
    modes = [{"label": "greedy", "temperature": 0.0, "wins": sum(gw.values()), "n": n,
              "ci": wilson(sum(gw.values()), n), "differs": 0.0,
              "decisions_per_run": len(steps) / n}]
    for t, runs in sampled.items():
        ss = [s for r in runs for s in r.steps]
        w = sum(r.won for r in runs)
        modes.append({
            "label": f"T = {t:g}", "temperature": t, "wins": w, "n": len(runs),
            "ci": wilson(w, len(runs)),
            "differs": sum(s.played != s.greedy for s in ss) / len(ss),
            "decisions_per_run": len(ss) / len(runs),
            "paired": check.paired(greedy, runs),
            "differs_fight": _share([s for s in ss if s.is_combat]),
            "differs_between": _share([s for s in ss if not s.is_combat]),
        })
    return {
        "n_decisions": len(steps),
        "median_top_fight": statistics.median(1 - x for x in miss["fight"]),
        "median_top_between": statistics.median(1 - x for x in miss["between"]),
        "share_sure": sum(s.top_p > check.TOP_SURE for s in steps) / len(steps),
        "share_sure_fight": _sure([s for s in steps if s.is_combat]),
        "share_sure_between": _sure([s for s in steps if not s.is_combat]),
        "mean_entropy_fight": statistics.mean(s.entropy for s in steps if s.is_combat),
        "mean_entropy_between": statistics.mean(s.entropy for s in steps if not s.is_combat),
        "n_fight": len(miss["fight"]), "n_between": len(miss["between"]),
        "cdf": {"grid": grid, **cdf},
        "types": types,
        "survival": survival,
        "expected_off_greedy": {"mean": statistics.mean(expected_off),
                                "median": statistics.median(expected_off)},
        "modes": modes,
        "outcomes": {"greedy": {r.seed: r.won for r in greedy},
                     **{f"T{t:g}": {r.seed: r.won for r in runs} for t, runs in sampled.items()}},
    }


def _share(ss) -> float:
    return sum(s.played != s.greedy for s in ss) / len(ss)


def _sure(ss) -> float:
    return sum(s.top_p > check.TOP_SURE for s in ss) / len(ss)


def collect(n_seeds: int, workers: int) -> None:
    import sts2_rs

    path = str(tourlib.weight(check.NET))
    seeds = list(tourlib.PANEL_A[:n_seeds])
    t0 = time.time()
    ex = example(sts2_rs.E2eStudent(path), seeds[0])
    jobs = [(s, "greedy", 1.0) for s in seeds] + [(s, "sample", t) for t in TEMPS for s in seeds]
    with ProcessPoolExecutor(workers, initializer=check._init_worker, initargs=(path,)) as pool:
        runs = list(pool.map(_job, jobs))
    greedy = runs[:n_seeds]
    sampled = {t: runs[n_seeds * (i + 1): n_seeds * (i + 2)] for i, t in enumerate(TEMPS)}
    out = {"net": check.NET, "seeds": [seeds[0], seeds[-1]], "example_seed": seeds[0],
           "example": ex, **summarize(greedy, sampled),
           "seconds": round(time.time() - t0), "workers": workers}
    DATA.write_text(json.dumps(out, indent=1, default=float) + "\n")
    print(f"wrote {DATA} in {out['seconds']} s")


# ── svg ──


def _f(x: float) -> str:
    return f"{x:.1f}".rstrip("0").rstrip(".")


def svg_mechanism(d: dict) -> str:
    """Tokens -> trunk -> one head call per option -> logits -> softmax -> pick,
    with the real numbers of the example fight decision."""
    ex = d["example"]["fight"]
    z = np.array(ex["logits"])
    p = check.softmax(z)
    order = list(np.argsort(-z))
    n = len(z)
    rowh = 34
    top = 150
    h = top + n * rowh + 70
    W = 760
    o = [f'<svg viewBox="0 0 {W} {h}" role="img" aria-label="How the policy network turns one '
         f'observation and its {n} legal options into one probability per option.">',
         '<defs><marker id="ar2" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" '
         'markerHeight="7" orient="auto-start-reverse"><path d="M0 0L10 5L0 10z" '
         'fill="currentColor"/></marker></defs><g color="var(--fg)">']
    # tokens
    o.append('<text x="10" y="18" class="muted">observation o_t as tokens</text>')
    toks = [("RUNCTX", "f1"), ("draw", "f4"), ("discard", "f4"), ("deck", "f4"), ("relics", "f4"),
            ("boss", "f4"), ("enemy", "f2"), ("hand card", "f2"), ("…", "f4")]
    x = 10
    for name, cls in toks:
        w = 8 + 6.6 * len(name)
        o.append(f'<rect x="{x}" y="28" width="{w:.0f}" height="22" rx="3" class="{cls}" '
                 f'fill-opacity="0.18" stroke="var(--fg)" stroke-width="0.6"/>'
                 f'<text x="{x + w / 2:.0f}" y="43" text-anchor="middle" font-size="11">{name}</text>')
        x += w + 4
    o.append(f'<text x="{x + 4:.0f}" y="43" class="muted" font-size="11">'
             f'{ex["n_tok"]} tokens in this decision</text>')
    # trunk
    o.append('<line class="ink" x1="250" y1="52" x2="250" y2="72" marker-end="url(#ar2)"/>')
    o.append('<rect class="box" x="10" y="74" width="740" height="40" rx="3"/>'
             '<text x="380" y="92" text-anchor="middle">trunk: 4 attention blocks, no positions, '
             'one 256-number vector out per token</text>'
             '<text x="380" y="107" text-anchor="middle" class="muted" font-size="11">'
             'every token can attend to every other token</text>')
    o.append('<text x="10" y="140" class="muted" font-size="11">option token h(a) + state token '
             'h(RUNCTX) + kind</text>'
             '<text x="300" y="140" class="muted" font-size="11">combat head → logit z(s,a)</text>'
             '<text x="500" y="140" class="muted" font-size="11">softmax over 𝒜(s) → π(a|s)</text>')
    o.append('<line class="ink" x1="120" y1="114" x2="120" y2="128" marker-end="url(#ar2)"/>')
    zmin, zmax = float(z.min()), float(z.max())
    for r, i in enumerate(order):
        y = top + r * rowh
        lab = ex["labels"][i].replace("->", "→")
        if len(lab) > 34:
            lab = lab[:33] + "…"
        o.append(f'<rect x="10" y="{y}" width="250" height="{rowh - 8}" rx="3" class="f2" '
                 f'fill-opacity="0.12" stroke="var(--fg)" stroke-width="0.6"/>'
                 f'<text x="18" y="{y + 17}" font-size="11">{lab}</text>')
        o.append(f'<line class="ink" x1="262" y1="{y + 13}" x2="296" y2="{y + 13}" marker-end="url(#ar2)"/>')
        # logit bar centred at zero
        zx0 = 300 + 80 * (0 - zmin) / (zmax - zmin)
        zx = 300 + 80 * (z[i] - zmin) / (zmax - zmin)
        o.append(f'<line x1="{zx0:.1f}" y1="{y}" x2="{zx0:.1f}" y2="{y + rowh - 8}" class="grid"/>'
                 f'<rect x="{min(zx0, zx):.1f}" y="{y + 6}" width="{abs(zx - zx0):.1f}" height="14" class="f3"/>'
                 f'<text x="400" y="{y + 17}" font-size="11">{z[i]:+.2f}</text>')
        o.append(f'<line class="ink" x1="448" y1="{y + 13}" x2="496" y2="{y + 13}" marker-end="url(#ar2)"/>')
        pw = 160 * p[i]
        o.append(f'<rect x="500" y="{y + 6}" width="160" height="14" class="grid"/>'
                 f'<rect x="500" y="{y + 6}" width="{max(pw, 0.5):.1f}" height="14" class="f1"/>'
                 f'<text x="668" y="{y + 17}" font-size="11">{_pfmt(p[i])}</text>')
        if i == ex["greedy"]:
            o.append(f'<text x="706" y="{y + 17}" font-size="11" font-weight="600">← max</text>')
    yb = top + n * rowh + 6
    o.append(f'<text x="10" y="{yb + 14}" class="muted" font-size="11">'
             f'Logits are unbounded scores; only their differences matter. The softmax turns them into '
             f'probabilities that sum to 1.</text>'
             f'<text x="10" y="{yb + 32}" class="muted" font-size="11">'
             f'Training samples a class from the bars on the right; the shipped agent takes the longest '
             f'bar.</text>')
    o.append('</g></svg>')
    return "\n".join(o)


def _short(label: str) -> str:
    """A legend-sized name: 'play DEFEND_IRONCLAD' -> 'Defend', 'POTION slot=0 …' -> 'potion 0'."""
    if m := re.match(r"POTION slot=(\d+)", label):
        return f"potion {m[1]}"
    if m := re.match(r"play ([A-Z_]+?)_IRONCLAD", label):
        return m[1].replace("_", " ").title()
    return label


def _pfmt(x: float) -> str:
    if x >= 0.001:
        return f"{x:.3f}"
    e = int(math.floor(math.log10(x)))
    return f"{x / 10 ** e:.1f}e{e}"


def _axes(o, x0, y0, w, h, xt, yt, xlab, ylab, xfmt=str, yfmt=str, xpos=None, ypos=None):
    for v in yt:
        y = ypos(v)
        o.append(f'<line x1="{x0}" y1="{y:.1f}" x2="{x0 + w}" y2="{y:.1f}" class="grid"/>'
                 f'<text x="{x0 - 6}" y="{y + 4:.1f}" text-anchor="end" class="muted" font-size="11">{yfmt(v)}</text>')
    for v in xt:
        x = xpos(v)
        o.append(f'<line x1="{x:.1f}" y1="{y0 + h}" x2="{x:.1f}" y2="{y0 + h + 4}" class="ink"/>'
                 f'<text x="{x:.1f}" y="{y0 + h + 17}" text-anchor="middle" class="muted" font-size="11">{xfmt(v)}</text>')
    o.append(f'<line x1="{x0}" y1="{y0 + h}" x2="{x0 + w}" y2="{y0 + h}" class="ink"/>'
             f'<line x1="{x0}" y1="{y0}" x2="{x0}" y2="{y0 + h}" class="ink"/>'
             f'<text x="{x0 + w / 2}" y="{y0 + h + 36}" text-anchor="middle">{xlab}</text>'
             f'<text transform="translate({x0 - 46},{y0 + h / 2}) rotate(-90)" text-anchor="middle">{ylab}</text>')


def svg_temperature(d: dict) -> str:
    """The example decision's probabilities and entropy as the temperature varies."""
    ex = d["example"]["fight"]
    z = np.array(ex["logits"])
    temps = np.exp(np.linspace(math.log(0.1), math.log(20), 120))
    P = np.array([check.softmax(z / t) for t in temps])
    H = np.array([check.entropy(q) for q in P]) / math.log(len(z))
    W, Hh, x0, y0, w, h = 700, 330, 70, 20, 480, 250
    lx = lambda t: x0 + w * (math.log(t) - math.log(0.1)) / (math.log(20) - math.log(0.1))
    ly = lambda v: y0 + h * (1 - v)
    o = [f'<svg viewBox="0 0 {W} {Hh}" role="img" aria-label="Probabilities of the example '
         f'decision\'s classes against temperature.">']
    _axes(o, x0, y0, w, h, [0.1, 0.5, 1, 2, 5, 20], [0, .25, .5, .75, 1],
          "temperature T (log scale)", "probability", xfmt=lambda v: f"{v:g}",
          yfmt=lambda v: f"{v:g}", xpos=lx, ypos=ly)
    order = list(np.argsort(-z))
    cls = ["l1", "l2", "l3", "l4", "l4"]
    dash = ["", "", "", "", ' stroke-dasharray="2 3"']
    for r, i in enumerate(order):
        pts = " ".join(f"{lx(t):.1f},{ly(q):.1f}" for t, q in zip(temps, P[:, i]))
        o.append(f'<polyline points="{pts}" class="{cls[min(r, 4)]}" stroke-width="2"{dash[min(r, 4)]}/>')
        lab = _short(ex["labels"][i])
        o.append(f'<text x="{x0 + w + 8}" y="{30 + r * 18}" font-size="11"><tspan class="{cls[min(r, 4)].replace("l", "f")}">■</tspan> {lab}</text>')
    pts = " ".join(f"{lx(t):.1f},{ly(v):.1f}" for t, v in zip(temps, H))
    o.append(f'<polyline points="{pts}" class="ink" stroke-width="1.5" stroke-dasharray="6 4"/>')
    o.append(f'<text x="{x0 + w + 8}" y="{30 + len(z) * 18 + 6}" font-size="11">- - entropy / log {len(z)}</text>')
    o.append(f'<line x1="{lx(1):.1f}" y1="{y0}" x2="{lx(1):.1f}" y2="{y0 + h}" class="ink" stroke-dasharray="1 3"/>'
             f'<text x="{lx(1) + 4:.1f}" y="{y0 + 12}" class="muted" font-size="11">T = 1: the trained policy</text>')
    o.append("</svg>")
    return "\n".join(o)


def svg_cdf(d: dict) -> str:
    """Empirical CDF of 1 - p_top, fight and between-fight decisions."""
    grid = d["cdf"]["grid"]
    W, Hh, x0, y0, w, h = 700, 320, 70, 20, 560, 240
    lx = lambda v: x0 + w * (math.log10(v) + 12) / 12
    ly = lambda v: y0 + h * (1 - v)
    o = [f'<svg viewBox="0 0 {W} {Hh}" role="img" aria-label="Cumulative share of decisions '
         f'against the probability left off the top class.">']
    _axes(o, x0, y0, w, h, [1e-12, 1e-9, 1e-6, 1e-3, 1e-1, 1], [0, .25, .5, .75, 1],
          "probability not on the top class, 1 − max π(a|s) (log scale)",
          "share of decisions at or below", xfmt=lambda v: {1e-12: "10⁻¹²", 1e-9: "10⁻⁹", 1e-6: "10⁻⁶",
                                                             1e-3: "10⁻³", 1e-1: "0.1", 1: "1"}[v],
          yfmt=lambda v: f"{v:g}", xpos=lx, ypos=ly)
    for key, cls, name in (("fight", "l1", "in a fight"), ("between", "l2", "between fights")):
        pts = " ".join(f"{lx(g):.1f},{ly(c):.1f}" for g, c in zip(grid, d["cdf"][key]))
        o.append(f'<polyline points="{pts}" class="{cls}" stroke-width="2"/>')
    o.append(f'<line x1="{lx(0.1):.1f}" y1="{y0}" x2="{lx(0.1):.1f}" y2="{y0 + h}" class="ink" stroke-dasharray="2 3"/>'
             f'<text x="{lx(0.1) - 4:.1f}" y="{y0 + h - 8}" text-anchor="end" class="muted" font-size="11">top class at 0.9</text>')
    o.append(f'<text x="{x0 + 10}" y="{y0 + 14}" font-size="11"><tspan class="f1">■</tspan> in a fight '
             f'({d["n_fight"]:,} decisions)</text>'
             f'<text x="{x0 + 10}" y="{y0 + 30}" font-size="11"><tspan class="f2">■</tspan> between fights '
             f'({d["n_between"]:,})</text>')
    o.append("</svg>")
    return "\n".join(o)


def svg_types(d: dict) -> str:
    """Normalised entropy by decision type, with counts."""
    types = [t for t in d["types"] if t["count"] >= 20]
    rowh = 22
    W, x0, w = 700, 150, 380
    Hh = 30 + rowh * len(types) + 46
    mx = 0.5
    lx = lambda v: x0 + w * v / mx
    o = [f'<svg viewBox="0 0 {W} {Hh}" role="img" aria-label="Mean normalised entropy for each '
         f'decision type.">']
    for v in (0, .1, .2, .3, .4, .5):
        o.append(f'<line x1="{lx(v):.1f}" y1="20" x2="{lx(v):.1f}" y2="{20 + rowh * len(types)}" class="grid"/>'
                 f'<text x="{lx(v):.1f}" y="{34 + rowh * len(types)}" text-anchor="middle" class="muted" font-size="11">{v:g}</text>')
    o.append(f'<text x="{x0 + w / 2}" y="{54 + rowh * len(types)}" text-anchor="middle">'
             f'mean entropy ÷ log |𝒜(s)| (0 = certain, 1 = uniform)</text>'
             f'<text x="{x0 + w + 12}" y="14" class="muted" font-size="11">decisions · mean options</text>')
    for r, t in enumerate(types):
        y = 20 + r * rowh
        cls = "f1" if t["combat"] else "f2"
        o.append(f'<text x="{x0 - 8}" y="{y + 15}" text-anchor="end" font-size="11">{t["type"]}</text>'
                 f'<rect x="{x0}" y="{y + 4}" width="{lx(t["mean_norm_entropy"]) - x0:.1f}" height="14" class="{cls}"/>'
                 f'<text x="{x0 + w + 12}" y="{y + 15}" class="muted" font-size="11">'
                 f'{t["count"]:,} · {t["mean_classes"]:.1f}</text>')
    o.append("</svg>")
    return "\n".join(o)


def svg_survival(d: dict) -> str:
    """Median over greedy runs of prod_{i<k} p_top: the chance a T = 1 sampler
    copies greedy for the first k decisions."""
    ks = d["survival"]["k"]
    med = d["survival"]["median"]
    W, Hh, x0, y0, w, h = 700, 310, 70, 20, 560, 230
    lx = lambda k: x0 + w * k / 700
    ly = lambda v: y0 + h * (-math.log10(max(v, 1e-60))) / 60
    o = [f'<svg viewBox="0 0 {W} {Hh}" role="img" aria-label="Chance that sampled play has matched '
         f'greedy play for every one of the first k decisions.">']
    _axes(o, x0, y0, w, h, [0, 100, 200, 300, 400, 500, 600, 700], [1, 1e-15, 1e-30, 1e-45, 1e-60],
          "decisions into the run, k", "chance of no departure yet (log scale)",
          xfmt=str, yfmt=lambda v: {1: "1", 1e-15: "10⁻¹⁵", 1e-30: "10⁻³⁰", 1e-45: "10⁻⁴⁵", 1e-60: "10⁻⁶⁰"}[v],
          xpos=lx, ypos=ly)
    pts = " ".join(f"{lx(k):.1f},{ly(v):.1f}" for k, v in zip(ks, med) if v is not None and v > 1e-60)
    o.append(f'<polyline points="{pts}" class="l1" stroke-width="2"/>')
    o.append("</svg>")
    return "\n".join(o)


def svg_winrate(d: dict) -> str:
    """Win rate with 95% Wilson intervals for greedy and three temperatures."""
    modes = d["modes"]
    W, Hh, x0, y0, w, h = 700, 300, 70, 20, 560, 220
    lo = 0.5
    ly = lambda v: y0 + h * (1 - (v - lo) / (1 - lo))
    lx = lambda i: x0 + w * (i + 0.5) / len(modes)
    o = [f'<svg viewBox="0 0 {W} {Hh}" role="img" aria-label="Win rate of greedy and sampled play '
         f'with 95% intervals.">']
    _axes(o, x0, y0, w, h, [], [.5, .6, .7, .8, .9, 1], "how the action is chosen",
          "win rate", yfmt=lambda v: f"{v:.0%}", xpos=lx, ypos=ly)
    for i, m in enumerate(modes):
        x = lx(i)
        p = m["wins"] / m["n"]
        lo_, hi_ = m["ci"]
        o.append(f'<line x1="{x:.1f}" y1="{ly(lo_):.1f}" x2="{x:.1f}" y2="{ly(hi_):.1f}" class="l4" stroke-width="2"/>'
                 f'<circle cx="{x:.1f}" cy="{ly(p):.1f}" r="5" class="{"f2" if i == 0 else "f1"}"/>'
                 f'<text x="{x + 9:.1f}" y="{ly(p) + 4:.1f}" font-size="11">{p:.1%}</text>'
                 f'<text x="{x:.1f}" y="{y0 + h + 17}" text-anchor="middle" font-size="11">{m["label"]}</text>')
    o.append("</svg>")
    return "\n".join(o)


FIGS = {"mechanism": svg_mechanism, "temperature": svg_temperature, "cdf": svg_cdf,
        "types": svg_types, "survival": svg_survival, "winrate": svg_winrate}


def render() -> None:
    d = json.loads(DATA.read_text())
    html = CHAPTER.read_text()
    for name, fn in FIGS.items():
        pat = re.compile(rf"(<!-- fig:{name} -->).*?(<!-- /fig:{name} -->)", re.S)
        if not pat.search(html):
            sys.exit(f"chapter.html has no <!-- fig:{name} --> block")
        html = pat.sub(lambda m: m.group(1) + "\n" + fn(d) + "\n" + m.group(2), html)
    CHAPTER.write_text(html)
    print(f"rewrote {len(FIGS)} figures in {CHAPTER}")


def main() -> None:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("collect")
    c.add_argument("--seeds", type=int, default=120)
    c.add_argument("--workers", type=int, default=3)
    sub.add_parser("svg")
    a = ap.parse_args()
    if a.cmd == "collect":
        collect(a.seeds, a.workers)
    else:
        render()


if __name__ == "__main__":
    main()
