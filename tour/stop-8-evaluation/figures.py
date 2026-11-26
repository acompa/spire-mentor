"""Chapter 8's numbers and data figures, from `data/runs.json` (written by `check.py --save`).

    tour/run stop-8-evaluation/figures.py

Writes `data/summary.json` (every measured number the chapter quotes) and
replaces each `<!-- figure:NAME -->...<!-- /figure:NAME -->` block in
`chapter.html` with a freshly drawn inline SVG. Colours come from the chapter
template's tokens, so the plots follow the light and dark themes.
"""

from __future__ import annotations

import json
import math
import re
from pathlib import Path

import numpy as np
from ppo.e2e.stats import PairedTable, mcnemar_exact, newcombe_paired, paired_bootstrap, wilson

import check

STOP = Path(__file__).resolve().parent
DATA = STOP / "data"
CHAPTER = STOP / "chapter.html"
SIM_SEED = 20261125
#: Idle-machine throughput for ten threads. The only committed source is the tour README
#: before 05ce399b ("Running a check"), which says "about 800"; no measurement is recorded.
IDLE_RATE_10 = 800.0
#: Expected range (max - min) of six independent standard normals.
EXPECTED_RANGE_6 = 2.534


def expected_max_normal(k: int) -> float:
    """E[max of k independent standard normals], by numerical integration."""
    if k == 1:
        return 0.0
    x = np.linspace(-8, 8, 20001)
    phi = np.exp(-x * x / 2) / math.sqrt(2 * math.pi)
    cdf = 0.5 * (1 + np.vectorize(math.erf)(x / math.sqrt(2)))
    return float(np.sum(x * k * cdf ** (k - 1) * phi) * (x[1] - x[0]))


def selection_bias(k: int, n: int, d: float) -> float:
    """Expected optimism of the best of k equally good candidates read on the same n seeds."""
    return expected_max_normal(k) * math.sqrt(d / (2 * n))


def simulate_selection(k: int, n: int, p: float, d: float, reps: int, rng) -> float:
    """Monte Carlo of `selection_bias`: k candidates of true rate p with pairwise discordance d.

    On each seed, with probability d / (2p(1-p)) every candidate draws its own
    Bernoulli(p) outcome; otherwise all share one. Two candidates then disagree
    with probability d.
    """
    indep = d / (2 * p * (1 - p))
    shared = rng.random((reps, n, 1)) < p
    own = rng.random((reps, n, k)) < p
    mode = rng.random((reps, n, 1)) < indep
    wins = np.where(mode, own, shared)
    rates = wins.mean(axis=1)  # reps x k
    return float((rates.max(axis=1) - p).mean())


def resplit_optimism(w1: np.ndarray, w2: np.ndarray, reps: int, rng) -> dict:
    """Real-data selection: split the seeds in half, pick the network that wins more on
    half one, and compare its rate there with its rate on half two."""
    n = len(w1)
    gaps, picked1 = [], 0
    for _ in range(reps):
        perm = rng.permutation(n)
        sel, held = perm[: n // 2], perm[n // 2 :]
        r1, r2 = w1[sel].mean(), w2[sel].mean()
        best = w1 if r1 >= r2 else w2
        picked1 += r1 >= r2
        gaps.append(best[sel].mean() - best[held].mean())
    return {"mean_gap": float(np.mean(gaps)), "share_picked_net1": picked1 / reps, "reps": reps}


def analyse(raw: dict) -> dict:
    a1, a2 = raw["arms"]
    runs1, runs2 = raw["runs"][a1], raw["runs"][a2]
    assert [r["seed"] for r in runs1] == [r["seed"] for r in runs2]
    w1 = np.array([r["won"] for r in runs1])
    w2 = np.array([r["won"] for r in runs2])
    panels = [r["panel"] for r in runs1]
    n = len(w1)
    t = PairedTable.from_pairs(list(zip(w1.tolist(), w2.tolist(), strict=True)))
    k1, k2 = int(w1.sum()), int(w2.sum())

    def rate(ws):
        k, m = int(np.sum(ws)), len(ws)
        lo, hi = wilson(k, m)
        return {"k": k, "n": m, "p": k / m, "lo": lo, "hi": hi}

    per_panel = {
        name: {p: rate(ws[np.array(panels) == p]) for p in sorted(set(panels))}
        for name, ws in ((a1, w1), (a2, w2))
    }
    slices = {}
    for name, ws in ((a1, w1), (a2, w2)):
        out = []
        for p in sorted(set(panels)):
            sub = ws[np.array(panels) == p]
            for i in range(0, len(sub) - check.SLICE_SIZE + 1, check.SLICE_SIZE):
                out.append({"panel": p, "start": i, **rate(sub[i : i + check.SLICE_SIZE])})
        slices[name] = out

    decisions = sum(r["decisions"] for r in runs1 + runs2)
    run_seconds = sum(r["seconds"] for r in runs1 + runs2)
    wall = sum(raw["wall_seconds"].values())
    agg_rate = decisions / wall
    per_thread = decisions / run_seconds
    per_run = decisions / (2 * n)
    boot = paired_bootstrap(list(zip(w1.tolist(), w2.tolist(), strict=True)))
    rng = np.random.default_rng(SIM_SEED)
    p_mean = (t.p1 + t.p2) / 2
    sel_sim = {
        str(nn): {str(k): simulate_selection(k, nn, p_mean, t.discordance, 4000 if nn < 4000 else 800, rng)
                  for k in (2, 3, 5, 9)}
        for nn in (300, 1000, 4000)
    }
    def slice_stats(sl):
        rates = [r["p"] for r in sl]
        p = float(np.mean(rates))
        sd = math.sqrt(p * (1 - p) / check.SLICE_SIZE)
        return {"spread": max(rates) - min(rates), "sd": sd, "expected_range": EXPECTED_RANGE_6 * sd,
                "min": min(rates), "max": max(rates), "count": len(rates)}

    def panel_gap(pp_):
        a, b = pp_["A"], pp_["B"]
        return {"diff": a["p"] - b["p"], "p": check.unpaired_z_p(a["k"], a["n"], b["k"], b["n"])}

    return {
        "net1": rate(w1), "net2": rate(w2),
        "panel1": per_panel[a1], "panel2": per_panel[a2],
        "gap1": panel_gap(per_panel[a1]), "gap2": panel_gap(per_panel[a2]),
        "slice1": slice_stats(slices[a1]), "slice2": slice_stats(slices[a2]),
        "half_width1": (rate(w1)["hi"] - rate(w1)["lo"]) / 2,
        "discordant": t.b + t.c,
        "variance_ratio": (check.paired_se(t) / check.unpaired_se(t)) ** 2,
        "arms": [a1, a2],
        "n_paired": n,
        "workers": raw["workers"],
        "loadavg_before": raw["loadavg_before"],
        "loadavg_after": raw["loadavg_after"],
        "pooled": {a1: rate(w1), a2: rate(w2)},
        "per_panel": per_panel,
        "slices": slices,
        "table": {"a": t.a, "b": t.b, "c": t.c, "d": t.d},
        "diff": t.diff,
        "discordance": t.discordance,
        "independence_discordance": t.p1 * (1 - t.p2) + t.p2 * (1 - t.p1),
        "discordance_over_independence": t.discordance / (t.p1 * (1 - t.p2) + t.p2 * (1 - t.p1)),
        "mcnemar_p": mcnemar_exact(t.b, t.c),
        "newcombe_paired": newcombe_paired(t.a, t.b, t.c, t.d),
        "newcombe_unpaired": check.newcombe_unpaired(k1, n, k2, n),
        "unpaired_z_p": check.unpaired_z_p(k1, n, k2, n),
        "paired_se": check.paired_se(t),
        "unpaired_se": check.unpaired_se(t),
        "unpaired_equivalent_extra": (check.unpaired_se(t) / check.paired_se(t)) ** 2 - 1,
        "bootstrap": [boot["lo"], boot["hi"]],
        "phi": (t.a * t.d - t.b * t.c) / math.sqrt((t.a + t.b) * (t.c + t.d) * (t.a + t.c) * (t.b + t.d)),
        "decisions": decisions,
        "decisions_per_run": per_run,
        "wall_seconds": wall,
        "rate_measured": agg_rate,
        "rate_per_thread": per_thread,
        "cost": {
            "decisions": {str(m): {"hours_measured": m / agg_rate / 3600, "hours_idle10": m / IDLE_RATE_10 / 3600}
                          for m in check.COST_DECISIONS},
            "paired_seeds": {
                str(m): {
                    "hours_measured": check.hours_for(m, per_run, agg_rate),
                    "hours_idle10": check.hours_for(m, per_run, IDLE_RATE_10),
                    "mde80": {f"d{round(100 * d):02d}": check.mde80(m, d) for d in check.COST_DISCORDANCE},
                    "mde80_unpaired": check.mde80_unpaired(m, p_mean),
                    "mde80_d": check.mde80(m, t.discordance),
                }
                for m in check.COST_SEEDS
            },
        },
        "campaign_train_rate": 10_500,
        "selection": {
            "e_max": {str(k): expected_max_normal(k) for k in (2, 3, 5, 9)},
            "formula": {str(nn): {str(k): selection_bias(k, nn, t.discordance) for k in (2, 3, 5, 9)}
                        for nn in (300, 1000, 4000)},
            "simulated": sel_sim,
            "resplit": resplit_optimism(w1, w2, 4000, rng),
        },
        "running": [int(x) for x in np.cumsum(w1)],
    }


class Plot:
    """A minimal SVG plot. x and y map data to a fixed frame; every colour is a template token."""

    def __init__(self, w=640, h=330, left=62, right=24, top=22, bottom=52, xlim=(0, 1), ylim=(0, 1),
                 xlog=False, label=""):
        self.w, self.h, self.l, self.r, self.t, self.b = w, h, left, right, top, bottom
        self.xlim, self.ylim, self.xlog = xlim, ylim, xlog
        self.out = [f'<svg viewBox="0 0 {w} {h}" role="img" aria-label="{label}">']

    def x(self, v):
        lo, hi = self.xlim
        if self.xlog:
            v, lo, hi = math.log10(v), math.log10(lo), math.log10(hi)
        return self.l + (v - lo) / (hi - lo) * (self.w - self.l - self.r)

    def y(self, v):
        lo, hi = self.ylim
        return self.h - self.b - (v - lo) / (hi - lo) * (self.h - self.t - self.b)

    def axes(self, xticks, yticks, xlabel, ylabel, xfmt=str, yfmt=str):
        o = self.out
        for v in yticks:
            o.append(f'<line class="grid" x1="{self.l}" x2="{self.w - self.r}" y1="{self.y(v):.1f}" y2="{self.y(v):.1f}"/>')
            o.append(f'<text class="muted" x="{self.l - 8}" y="{self.y(v) + 4:.1f}" text-anchor="end">{yfmt(v)}</text>')
        for v in xticks:
            o.append(f'<text class="muted" x="{self.x(v):.1f}" y="{self.h - self.b + 18}" text-anchor="middle">{xfmt(v)}</text>')
        o.append(f'<line class="ink" x1="{self.l}" x2="{self.w - self.r}" y1="{self.h - self.b}" y2="{self.h - self.b}"/>')
        o.append(f'<line class="ink" x1="{self.l}" x2="{self.l}" y1="{self.t}" y2="{self.h - self.b}"/>')
        o.append(f'<text x="{(self.l + self.w - self.r) / 2:.0f}" y="{self.h - 10}" text-anchor="middle">{xlabel}</text>')
        o.append(f'<text x="14" y="{(self.t + self.h - self.b) / 2:.0f}" text-anchor="middle" '
                 f'transform="rotate(-90 14 {(self.t + self.h - self.b) / 2:.0f})">{ylabel}</text>')

    def line(self, pts, cls="l1", width=2, dash=None):
        d = " ".join(f"{'M' if i == 0 else 'L'}{self.x(a):.1f} {self.y(b):.1f}" for i, (a, b) in enumerate(pts))
        extra = f' stroke-dasharray="{dash}"' if dash else ""
        self.out.append(f'<path class="{cls}" d="{d}" stroke-width="{width}"{extra}/>')

    def band(self, lo_pts, hi_pts, fill="f1", opacity=0.18):
        pts = [(self.x(a), self.y(b)) for a, b in lo_pts] + [(self.x(a), self.y(b)) for a, b in reversed(hi_pts)]
        d = "M" + " L".join(f"{a:.1f} {b:.1f}" for a, b in pts) + " Z"
        self.out.append(f'<path class="{fill}" d="{d}" opacity="{opacity}" stroke="none"/>')

    def dot(self, xv, yv, fill="f1", r=4):
        self.out.append(f'<circle class="{fill}" cx="{self.x(xv):.1f}" cy="{self.y(yv):.1f}" r="{r}"/>')

    def whisker(self, xv, lo, hi, cls="l1"):
        self.out.append(f'<line class="{cls}" x1="{self.x(xv):.1f}" x2="{self.x(xv):.1f}" '
                        f'y1="{self.y(lo):.1f}" y2="{self.y(hi):.1f}" stroke-width="1.6"/>')

    def text(self, xv, yv, s, cls="", anchor="start", dx=0, dy=0):
        c = f' class="{cls}"' if cls else ""
        self.out.append(f'<text{c} x="{self.x(xv) + dx:.1f}" y="{self.y(yv) + dy:.1f}" text-anchor="{anchor}">{s}</text>')

    def legend(self, items, x0=None, y0=None):
        x0 = self.l + 14 if x0 is None else x0
        y0 = self.t + 12 if y0 is None else y0
        for i, (cls, label) in enumerate(items):
            yy = y0 + i * 18
            if cls.startswith("f"):
                self.out.append(f'<circle class="{cls}" cx="{x0 + 9}" cy="{yy - 4}" r="4"/>')
            else:
                self.out.append(f'<line class="{cls}" x1="{x0}" x2="{x0 + 18}" y1="{yy - 4}" y2="{yy - 4}" stroke-width="2"/>')
            self.out.append(f'<text x="{x0 + 26}" y="{yy}">{label}</text>')

    def svg(self):
        return "\n".join(self.out + ["</svg>"])


def pp(v):
    return f"{100 * v:.0f}%"


def fig_running(s: dict) -> str:
    cum = s["running"]
    n = len(cum)
    pl = Plot(xlim=(0, n), ylim=(0.5, 1.0), label="Running win rate of the Ironclad network with its 95% Wilson band")
    pl.axes(range(0, n + 1, 100), [0.5, 0.6, 0.7, 0.8, 0.9, 1.0], "seeds played (n)", "win rate", yfmt=pp)
    pts = [(i + 1, c / (i + 1)) for i, c in enumerate(cum)]
    lo = [(i + 1, wilson(c, i + 1)[0]) for i, c in enumerate(cum)]
    hi = [(i + 1, wilson(c, i + 1)[1]) for i, c in enumerate(cum)]
    pl.band(lo[4:], hi[4:])
    pl.line(pts[4:], "l1", 1.8)
    final = s["pooled"][s["arms"][0]]
    pl.line([(0, final["p"]), (n, final["p"])], "l4", 1, "4 4")
    pl.text(n, final["p"], f"final estimate {100 * final['p']:.1f}%", "muted", "end", dy=-8)
    pl.legend([("l1", "running estimate k/n"), ("f1", "95% Wilson interval (shaded)")], y0=pl.h - pl.b - 34)
    return pl.svg()


def fig_se(s: dict) -> str:
    n, p1, p2 = s["n_paired"], s["pooled"][s["arms"][0]]["p"], s["pooled"][s["arms"][1]]["p"]
    diff = s["diff"]
    pl = Plot(xlim=(0, 0.5), ylim=(0, 3.2), label="Standard error of the difference against discordance")
    pl.axes([0, 0.1, 0.2, 0.3, 0.4, 0.5], [0, 0.5, 1, 1.5, 2, 2.5, 3], "discordance d (share of seeds where the networks disagree)",
            "SE of the difference (pp)", xfmt=lambda v: f"{v:.1f}", yfmt=lambda v: f"{v:g}")
    ds = np.linspace(diff ** 2 + 1e-4, 0.5, 200)
    pl.line([(d, 100 * math.sqrt((d - diff ** 2) / n)) for d in ds], "l1", 2)
    un = 100 * math.sqrt((p1 * (1 - p1) + p2 * (1 - p2)) / n)
    pl.line([(0, un), (0.5, un)], "l2", 2, "6 4")
    ind = s["independence_discordance"]
    pl.line([(ind, 0), (ind, 3.2)], "l4", 1, "2 4")
    pl.text(ind, 0.15, "independent outcomes", "muted", "start", dx=5)
    pl.dot(s["discordance"], 100 * s["paired_se"], "f1", 5)
    pl.text(s["discordance"], 100 * s["paired_se"], f"measured: d = {s['discordance']:.3f}", "", "end", dx=-8, dy=-8)
    pl.legend([("l1", f"paired, n = {n} seeds"), ("l2", f"unpaired, n = {n} per network")])
    return pl.svg()


def fig_slices(s: dict) -> str:
    a1, a2 = s["arms"]
    sl1, sl2 = s["slices"][a1], s["slices"][a2]
    m = len(sl1)
    pl = Plot(xlim=(0.4, m + 0.6), ylim=(0.6, 1.0), label="Win rate on disjoint 100-seed slices, both networks")
    pl.axes(range(1, m + 1), [0.6, 0.7, 0.8, 0.9, 1.0], "100-seed slice (A1-A3 from PANEL_A, B1-B3 from PANEL_B)",
            "win rate", xfmt=lambda i: f"{sl1[i - 1]['panel']}{sl1[i - 1]['start'] // 100 + 1}", yfmt=pp)
    for off, sl, cls, f in ((-0.12, sl1, "l1", "f1"), (0.12, sl2, "l2", "f2")):
        for i, r in enumerate(sl, start=1):
            pl.whisker(i + off, r["lo"], r["hi"], cls)
            pl.dot(i + off, r["p"], f)
    for name, cls in ((a1, "l1"), (a2, "l2")):
        p = s["pooled"][name]["p"]
        pl.line([(0.4, p), (m + 0.6, p)], cls, 1, "4 4")
    pl.legend([("f1", "Ironclad network (net 1)"), ("f2", "all-character network (net 2)")], y0=pl.h - pl.b - 30)
    return pl.svg()


def fig_selection(s: dict) -> str:
    d = s["discordance"]
    pl = Plot(xlim=(1, 10), ylim=(0, 3.0), label="Selection optimism against the number of candidates")
    pl.axes(range(1, 11), [0, 0.5, 1, 1.5, 2, 2.5, 3], "candidates compared on the same seeds (K)",
            "optimism of the best (pp)", yfmt=lambda v: f"{v:g}")
    for nn, cls, f in (("300", "l2", "f2"), ("1000", "l1", "f1"), ("4000", "l3", "f3")):
        pl.line([(k, 100 * selection_bias(k, int(nn), d)) for k in range(1, 11)], cls, 2)
        for k, v in s["selection"]["simulated"][nn].items():
            pl.dot(int(k), 100 * v, f, 3.5)
    pl.legend([("l2", "300 seeds"), ("l1", "1,000 seeds"), ("l3", "4,000 seeds"),
               ("f4", "dots: simulation")], y0=pl.t + 12)
    return pl.svg()


def fig_cost(s: dict) -> str:
    per_run = s["decisions_per_run"]
    pl = Plot(h=360, xlim=(100, 10000), ylim=(0, 16), xlog=True, top=64,
              label="Minimum detectable effect against paired seeds, with hours of play")
    ticks = [100, 300, 1000, 4000, 10000]
    pl.axes(ticks, [0, 4, 8, 12, 16], "paired seeds (log scale)", "MDE80 (pp)",
            xfmt=lambda v: f"{v:,}", yfmt=lambda v: f"{v:g}")
    pl.out.append(f'<text class="muted" x="{(pl.l + pl.w - pl.r) / 2:.0f}" y="16" text-anchor="middle">'
                  f'hours of play, both networks, idle Mac at {IDLE_RATE_10:.0f} decisions/s</text>')
    for v in ticks:
        h = check.hours_for(v, per_run, IDLE_RATE_10)
        pl.out.append(f'<text x="{pl.x(v):.1f}" y="38" text-anchor="middle">{h:.2g} h</text>')
        pl.out.append(f'<line class="grid" x1="{pl.x(v):.1f}" x2="{pl.x(v):.1f}" y1="44" y2="{pl.h - pl.b}"/>')
    ns = np.logspace(2, 4, 100)
    for d, cls in ((0.1, "l3"), (0.2, "l1"), (0.3, "l2")):
        pl.line([(m, 100 * check.mde80(int(m), d)) for m in ns], cls, 2)
    dm = s["discordance"]
    pl.line([(m, 100 * check.mde80(int(m), dm)) for m in ns], "l4", 1.5, "5 4")
    pl.legend([("l3", "δ = 0.10"), ("l1", "δ = 0.20"), ("l2", "δ = 0.30"), ("l4", f"δ = {dm:.3f} (measured)")],
              x0=pl.w - pl.r - 170, y0=pl.t + 14)
    return pl.svg()


FIGURES = {"running": fig_running, "se": fig_se, "slices": fig_slices, "selection": fig_selection, "cost": fig_cost}


def inject(html: str, name: str, svg: str) -> str:
    pat = re.compile(rf"(<!-- figure:{name} -->).*?(<!-- /figure:{name} -->)", re.S)
    if not pat.search(html):
        raise SystemExit(f"chapter.html has no <!-- figure:{name} --> block")
    return pat.sub(lambda m: m.group(1) + "\n" + svg + "\n" + m.group(2), html)


FORMATS = {
    "pct0": lambda v: f"{100 * v:.0f}%",
    "pct1": lambda v: f"{100 * v:.1f}%",
    "pp1": lambda v: f"{100 * v:.1f}",
    "spp1": lambda v: f"{100 * v:+.1f}",
    "pp2": lambda v: f"{100 * v:.2f}",
    "int": lambda v: f"{int(round(v)):,}",
    "f2": lambda v: f"{v:.2f}",
    "f3": lambda v: f"{v:.3f}",
    "h1": lambda v: f"{v:.1f}",
    "h0": lambda v: f"{v:,.0f}",
    "g2": lambda v: f"{v:.2g}",
}


def lookup(s: dict, path: str):
    v = s
    for part in path.split("."):
        v = v[int(part)] if isinstance(v, (list, tuple)) else v[part]
    return v


def fill_numbers(html: str, s: dict) -> str:
    """Set the text of every <span data-m="path" data-f="fmt">...</span> from the summary."""
    pat = re.compile(r'(<span data-m="([^"]+)" data-f="([^"]+)">)[^<]*(</span>)')
    return pat.sub(lambda m: m.group(1) + FORMATS[m.group(3)](lookup(s, m.group(2))) + m.group(4), html)


def main() -> None:
    raw = json.loads((DATA / "runs.json").read_text(encoding="utf-8"))
    s = analyse(raw)
    (DATA / "summary.json").write_text(json.dumps(s, indent=1) + "\n", encoding="utf-8")
    print(json.dumps({k: v for k, v in s.items() if k not in ("running", "slices")}, indent=1))
    if CHAPTER.exists():
        html = CHAPTER.read_text(encoding="utf-8")
        for name, draw in FIGURES.items():
            html = inject(html, name, draw(s))
        html = fill_numbers(html, s)
        CHAPTER.write_text(html, encoding="utf-8")
        print(f"updated {len(FIGURES)} figures in {CHAPTER.name}")


if __name__ == "__main__":
    main()
