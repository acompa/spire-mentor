"""Chapter 9's numbers and data figures.

    python3 tour/stop-9-sample-efficiency/figures.py

Needs only Python 3 and NumPy, and runs in seconds. It does two things:

1. Copies the campaign numbers the chapter plots (each with its source file and
   line) into `figures.json`.
2. Runs three toy problems, none of them a language model and none of them
   Spire Mentor, and adds their results to `figures.json`:
   - SGD on a noisy quadratic: the loss floor against learning rate and batch size.
   - A contextual bandit trained with PPO's clipped surrogate at 1, 2, 4 and 8 epochs.
   - The same bandit across many training seeds: spread, early against late, and
     how often a few seeds pick the wrong arm.

Then it replaces each `<!-- figure:NAME -->...<!-- /figure:NAME -->` block in
`chapter.html` with a freshly drawn inline SVG whose colours are the template's tokens.
"""

from __future__ import annotations

import json
import math
import re
from pathlib import Path

import numpy as np

STOP = Path(__file__).resolve().parent
CHAPTER = STOP / "chapter.html"
OUT = STOP / "figures.json"
LOG = "docs/experiments/2026-11-03-ironclad-e2e-v111/CAMPAIGN_LOG.md"

# ---------------------------------------------------------------- campaign data

#: Development win rate (%) against decisions (billions), from the campaign log.
#: EMA0.996 weights were read until 4.0B; EMA0.999 from 1.0B on. DEV-1000 or DEV-2000 as noted.
EMA0996 = [  # (decisions B, win %, seeds, log line)
    (0.100, 16.9, 1000, 134), (0.250, 19.0, 1000, 141), (0.500, 27.8, 1000, 142),
    (0.595, 38.6, 1000, 146), (0.750, 43.6, 1000, 148), (1.00, 43.1, 2000, 155),
    (1.25, 47.7, 1000, 157), (1.50, 51.5, 1000, 158), (1.75, 52.9, 1000, 161),
    (2.00, 51.75, 2000, 167), (2.25, 54.5, 1000, 175), (2.50, 54.0, 1000, 177),
    (2.75, 57.1, 1000, 181), (3.00, 57.45, 2000, 186), (3.25, 59.2, 1000, 192),
    (3.50, 59.0, 1000, 197), (3.75, 63.6, 1000, 201), (4.00, 62.25, 2000, 212),
]
EMA0999 = [
    (1.00, 49.45, 2000, 155), (2.00, 57.08, 2000, 167), (3.00, 61.6, 2000, 187),
    (4.00, 66.30, 2000, 212), (4.25, 67.0, 1000, 218), (4.50, 67.2, 1000, 226),
    (4.75, 70.8, 1000, 231), (5.00, 70.70, 2000, 234), (5.25, 70.6, 1000, 240),
    (5.50, 74.3, 1000, 245), (5.75, 74.1, 1000, 246), (6.00, 74.50, 2000, 252),
    (6.25, 75.9, 1000, 258), (6.50, 77.9, 1000, 260), (6.75, 79.1, 1000, 262),
    (7.00, 78.15, 2000, 269), (7.25, 77.4, 1000, 273), (7.50, 78.5, 1000, 279),
    (7.75, 78.5, 1000, 284), (8.00, 79.3, 2000, 288), (8.25, 76.8, 1000, 290),
    (8.50, 81.3, 1000, 294), (8.75, 80.6, 1000, 297), (9.00, 81.4, 2000, 300),
]
EVENTS = [  # (decisions B, short label, log line)
    (0.500, "teacher KL weight reaches 0", 142),
    (0.551, "E = 2 to E = 1", 144),
    (2.28, "engine W3 to W4", 176),
    (4.0785, "learning rate halved", 214),
    (5.2876, "B doubled", 242),
    (6.0876, "B doubled", 256),
    (7.2685, "B doubled", 276),
    (7.7677, "anneal starts", 287),
]
#: The three doublings of decisions per update, from the smoke rows after each switch.
DOUBLINGS = [  # before, after: (decisions per update, probe_kl median, decisions/s), log line
    {"name": "Amendment 14", "at": 5.2876, "line": 243,
     "before": (131_374, 0.0030, 9_297), "after": (262_437, 0.0043, 9_818),
     "step_pp": 3.50, "step_line": 246},
    {"name": "Amendment 15", "at": 6.0876, "line": 257,
     "before": (262_445, 0.0035, 9_257), "after": (524_586, 0.0043, 10_318),
     "step_pp": 3.90, "step_line": 260},
    {"name": "Amendment 16", "at": 7.2685, "line": 277,
     "before": (524_586, 0.0043, 10_251), "after": (1_048_899, 0.0046, 10_479),
     "step_pp": 1.10, "step_line": 284},
]
#: Amendment 16's smoke gives no "before" probe_kl; the before value is Amendment 15's smoke median.
#: Its before rate, 10,251/s, is the log's "(was 10,251)" on line 277.
#: One iteration of 131,072 decisions before and after the epoch switch.
TIMING = {
    "E2": {"collect": 12.9, "cut": 1.0, "total": 20.3, "update": 12.0, "prepass": 3.0, "line": 143},
    "E1": {"rate": 9_100, "rate_e2_plan": (6_400, 6_900), "update": (4.8, 6.7), "prepass": 3.0,
           "line": 145, "plan_line": 9},
    "iter": 131_072,
}


def campaign() -> dict:
    t1 = TIMING["iter"] / TIMING["E1"]["rate"]
    t2 = TIMING["E2"]["total"]
    ddr = []
    for d in DOUBLINGS:
        (b0, k0, r0), (b1, k1, r1) = d["before"], d["after"]
        ddr.append({**d, "ddr_before": b0 / k0, "ddr_after": b1 / k1, "kl_ratio": k1 / k0,
                    "rate_ratio": r1 / r0})
    return {
        "source": LOG,
        "ema0996": EMA0996, "ema0999": EMA0999, "events": EVENTS, "doublings": ddr,
        "timing": TIMING, "t_iter_E1": t1, "t_iter_E2": t2, "time_ratio": t2 / t1,
        "q_break_even": t2 / t1 - 1,
    }


# ---------------------------------------------------------------- toy 1: noisy quadratic

def toy_floor(rng) -> dict:
    """SGD on L = 1/2 sum h_i x_i^2 with gradient noise of covariance Sigma / B.

    Steady state, exact for this model: E[L] - L* = (alpha / 2B) sum_i Sigma_ii / (2 - alpha h_i),
    about alpha tr(Sigma) / 4B when alpha h_i is small.
    """
    d = 50
    h = np.geomspace(0.05, 1.0, d)
    sig2 = np.ones(d)  # Sigma = I
    reps = 400

    def run(alpha_of_step, b_of_step, steps):
        x = np.full((reps, d), 3.0)
        trace = np.empty(steps)
        for t in range(steps):
            a, b = alpha_of_step(t), b_of_step(t)
            g = h * x + rng.standard_normal((reps, d)) * np.sqrt(sig2 / b)
            x = x - a * g
            trace[t] = float(np.mean(0.5 * np.sum(h * x * x, axis=1)))
        return trace

    # Floor against alpha / B at steady state.
    pts = []
    for alpha in (0.05, 0.1, 0.2, 0.4):
        for b in (1, 2, 4, 8, 16):
            tr = run(lambda t: alpha, lambda t: b, 1500)
            pred = alpha / (2 * b) * float(np.sum(sig2 / (2 - alpha * h)))
            pts.append({"alpha": alpha, "B": b, "measured": float(tr[-500:].mean()), "predicted": pred,
                        "small_step": alpha * float(sig2.sum()) / (4 * b)})

    # A schedule like the campaign's, in steps: constant, halve alpha, double B twice.
    seg = 600
    def a_of(t):
        return 0.4 if t < seg else 0.2
    def b_of(t):
        return 1 if t < 2 * seg else (2 if t < 3 * seg else 4)
    trace = run(a_of, b_of, 4 * seg)
    floors = [float(trace[(k + 1) * seg - 200:(k + 1) * seg].mean()) for k in range(4)]
    # Decisions spent: B per step.
    dec = np.cumsum([b_of(t) for t in range(4 * seg)])
    every = 6
    return {
        "d": d, "h_range": [0.05, 1.0], "tr_sigma": float(sig2.sum()), "reps": reps,
        "grid": pts, "schedule": {"segment_steps": seg, "floors": floors,
                                  "trace": [[int(dec[i]), float(trace[i])] for i in range(0, 4 * seg, every)],
                                  "boundaries": [int(dec[seg - 1]), int(dec[2 * seg - 1]), int(dec[3 * seg - 1])]},
    }


# ---------------------------------------------------------------- toy 2 and 3: PPO on a bandit

C, A = 20, 8          # contexts, actions
B_DEC = 512           # decisions per iteration
M_ROWS = 128          # minibatch rows
CLIP = 0.2
MILESTONES = (0.5, 0.9)


def problem():
    rng = np.random.default_rng(7)
    p = rng.uniform(0.0, 1.0, (C, A))
    j_rand, j_max = p.mean(), p.max(axis=1).mean()
    return p, j_rand, j_max


def softmax(z):
    z = z - z.max(axis=-1, keepdims=True)
    e = np.exp(z)
    return e / e.sum(axis=-1, keepdims=True)


def train(epochs: int, lr: float, seeds: int, iters: int, seed0: int) -> np.ndarray:
    """Normalised score after each iteration, shape (seeds, iters + 1)."""
    p, j_rand, j_max = problem()
    rng = np.random.default_rng(seed0)
    th = np.zeros((seeds, C, A))
    out = np.empty((seeds, iters + 1))
    sidx = np.arange(seeds)[:, None]

    def score(th):
        return ((softmax(th) * p).sum(-1).mean(-1) - j_rand) / (j_max - j_rand)

    out[:, 0] = score(th)
    for it in range(iters):
        pi_old = softmax(th)
        c = rng.integers(0, C, (seeds, B_DEC))
        u = rng.random((seeds, B_DEC, 1))
        a = (u > np.cumsum(pi_old[sidx, c], axis=-1)).sum(-1).clip(max=A - 1)
        r = (rng.random((seeds, B_DEC)) < p[c, a]).astype(float)
        adv = (r - r.mean(1, keepdims=True)) / (r.std(1, keepdims=True) + 1e-8)
        logp_old = np.log(pi_old[sidx, c, a])
        for _ in range(epochs):
            perm = np.argsort(rng.random((seeds, B_DEC)), axis=1)
            for k in range(0, B_DEC, M_ROWS):
                rows = perm[:, k:k + M_ROWS]
                cc, aa, ad = c[sidx, rows], a[sidx, rows], adv[sidx, rows]
                pi = softmax(th)
                rho = np.exp(np.log(pi[sidx, cc, aa]) - logp_old[sidx, rows])
                live = ~(((ad > 0) & (rho > 1 + CLIP)) | ((ad < 0) & (rho < 1 - CLIP)))
                w = rho * ad * live / M_ROWS
                g = np.zeros((seeds, C, A))
                np.add.at(g, (np.broadcast_to(sidx, cc.shape), cc, aa), w)
                wc = np.zeros((seeds, C))
                np.add.at(wc, (np.broadcast_to(sidx, cc.shape), cc), w)
                g -= wc[..., None] * pi
                th += lr * g
        out[:, it + 1] = score(th)
    return out


def first_cross(curve: np.ndarray, level: float) -> np.ndarray:
    """Decisions at which each seed's curve first reaches `level` (inf if never)."""
    hit = curve >= level
    idx = np.where(hit.any(1), hit.argmax(1), -1)
    return np.where(idx >= 0, idx * B_DEC, np.inf).astype(float)


def toy_epochs() -> dict:
    seeds, iters = 40, 100
    out = {}
    for lr in (2.0, 4.0, 8.0):
        res = {}
        for e in (1, 2, 4, 8):
            cur = train(e, lr, seeds, iters, seed0=100 + e)
            d90 = first_cross(cur, MILESTONES[1])
            res[str(e)] = {"mean_curve": cur.mean(0).tolist(), "d_star_median": float(np.median(d90)),
                           "never": int(np.isinf(d90).sum())}
        d1, d2 = res["1"]["d_star_median"], res["2"]["d_star_median"]
        out[f"{lr:g}"] = {"arms": res, "ratio_1_to_2": d1 / d2, "q": d1 / d2 - 1}
    return {"seeds": seeds, "iters": iters, "B": B_DEC, "M": M_ROWS, "clip": CLIP, "C": C, "A": A,
            "milestone": MILESTONES[1], "by_lr": out, "lr_shown": "4"}


def toy_seeds() -> dict:
    """Two arms that differ only in learning rate: spread, early against late, wrong picks."""
    seeds, iters = 200, 120
    arms = {"A": 2.0, "B": 2.1}
    curves = {k: train(1, lr, seeds, iters, seed0=500 + i) for i, (k, lr) in enumerate(arms.items())}
    early = {k: first_cross(v, MILESTONES[0]) for k, v in curves.items()}
    late = {k: first_cross(v, MILESTONES[1]) for k, v in curves.items()}
    auc = {k: v.mean(1) for k, v in curves.items()}

    def ranks(x):
        """Ranks with ties given their average rank."""
        x = np.asarray(x)
        order = np.argsort(x, kind="stable")
        r = np.empty(len(x))
        r[order] = np.arange(len(x), dtype=float)
        for v in np.unique(x):
            m = x == v
            r[m] = r[m].mean()
        return r

    def spearman(x, y):
        return float(np.corrcoef(ranks(x), ranks(y))[0, 1])

    rng = np.random.default_rng(11)
    true_better = "B" if np.mean(late["B"]) < np.mean(late["A"]) else "A"
    wrong = {}
    for n in (1, 3, 5, 10, 20):
        reps = 20000
        ia = rng.integers(0, seeds, (reps, n))
        ib = rng.integers(0, seeds, (reps, n))
        ma, mb = late["A"][ia].mean(1), late["B"][ib].mean(1)
        # A tie (equal mean decisions) is settled by a coin flip, so it counts half.
        miss = np.where(mb == ma, 0.5, (mb < ma) != (true_better == "B"))
        wrong[str(n)] = float(np.mean(miss))
    # Several identical arms, one test each at level 0.05: chance of at least one false alarm.
    fw = {str(k): 1 - 0.95 ** k for k in (1, 2, 4, 8)}
    show = 12
    return {
        "seeds": seeds, "iters": iters, "lr": arms, "B": B_DEC, "milestones": MILESTONES,
        "late_mean": {k: float(np.mean(v)) for k, v in late.items()},
        "late_sd": {k: float(np.std(v, ddof=1)) for k, v in late.items()},
        "late_range": {k: [float(v.min()), float(v.max())] for k, v in late.items()},
        "early_late_spearman": {k: spearman(early[k], late[k]) for k in arms},
        "auc_late_spearman": {k: spearman(-auc[k], late[k]) for k in arms},
        "true_better": true_better, "wrong_pick": wrong, "familywise": fw,
        "curves_shown": {k: v[:show].tolist() for k, v in curves.items()},
        "early": {k: v.tolist() for k, v in early.items()},
        "late": {k: v.tolist() for k, v in late.items()},
    }


# ---------------------------------------------------------------- drawing

class Plot:
    """A minimal SVG plot. Every colour is a template token class."""

    def __init__(self, w=640, h=320, left=64, right=24, top=22, bottom=52, xlim=(0, 1), ylim=(0, 1),
                 xlog=False, ylog=False, label=""):
        self.w, self.h, self.l, self.r, self.t, self.b = w, h, left, right, top, bottom
        self.xlim, self.ylim, self.xlog, self.ylog = xlim, ylim, xlog, ylog
        self.out = [f'<svg viewBox="0 0 {w} {h}" role="img" aria-label="{label}">']

    def x(self, v):
        lo, hi = self.xlim
        if self.xlog:
            v, lo, hi = math.log10(v), math.log10(lo), math.log10(hi)
        return self.l + (v - lo) / (hi - lo) * (self.w - self.l - self.r)

    def y(self, v):
        lo, hi = self.ylim
        if self.ylog:
            v, lo, hi = math.log10(v), math.log10(lo), math.log10(hi)
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
        ym = (self.t + self.h - self.b) / 2
        o.append(f'<text x="16" y="{ym:.0f}" text-anchor="middle" transform="rotate(-90 16 {ym:.0f})">{ylabel}</text>')

    def line(self, pts, cls="l1", width=2, dash=None, opacity=None):
        d = " ".join(f"{'M' if i == 0 else 'L'}{self.x(a):.1f} {self.y(b):.1f}" for i, (a, b) in enumerate(pts))
        extra = f' stroke-dasharray="{dash}"' if dash else ""
        extra += f' opacity="{opacity}"' if opacity else ""
        self.out.append(f'<path class="{cls}" d="{d}" stroke-width="{width}"{extra}/>')

    def dot(self, xv, yv, fill="f1", r=3):
        self.out.append(f'<circle class="{fill}" cx="{self.x(xv):.1f}" cy="{self.y(yv):.1f}" r="{r}"/>')

    def vline(self, xv, cls="l4", dash="3 3"):
        self.out.append(f'<line class="{cls}" x1="{self.x(xv):.1f}" x2="{self.x(xv):.1f}" y1="{self.t}" '
                        f'y2="{self.h - self.b}" stroke-width="1" stroke-dasharray="{dash}"/>')

    def text(self, xv, yv, s, cls="", anchor="start", dx=0, dy=0, rotate=False, size=None):
        c = f' class="{cls}"' if cls else ""
        xx, yy = self.x(xv) + dx, self.y(yv) + dy
        rot = f' transform="rotate(-90 {xx:.1f} {yy:.1f})"' if rotate else ""
        sz = f' style="font-size:{size}px"' if size else ""
        self.out.append(f'<text{c} x="{xx:.1f}" y="{yy:.1f}" text-anchor="{anchor}"{rot}{sz}>{s}</text>')

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


def fig_campaign(s):
    c = s["campaign"]
    pl = Plot(h=360, xlim=(0.08, 10), ylim=(10, 90), xlog=True, top=30,
              label="Development win rate of the Spire Mentor network against decisions, log scale")
    pl.axes([0.1, 0.2, 0.5, 1, 2, 5, 9], range(10, 91, 10), "decisions (billions, log scale)",
            "development win rate (%)", xfmt=lambda v: f"{v:g}", yfmt=lambda v: f"{v}")
    for i, (x, lab, _) in enumerate(c["events"]):
        pl.vline(x)
        pl.text(x, 12, lab, "muted", "start", dx=-3, rotate=True, size=10)
    pl.line([(x, y) for x, y, *_ in c["ema0996"]], "l2", 1.8)
    for x, y, *_ in c["ema0996"]:
        pl.dot(x, y, "f2", 2.6)
    pl.line([(x, y) for x, y, *_ in c["ema0999"]], "l1", 1.8)
    for x, y, *_ in c["ema0999"]:
        pl.dot(x, y, "f1", 2.6)
    pl.legend([("l1", "EMA0.999 weights"), ("l2", "EMA0.996 weights")], x0=pl.l + 12, y0=pl.t + 8)
    return pl.svg()


def fig_timing(s):
    t = s["campaign"]["timing"]
    e2 = t["E2"]
    t1 = s["campaign"]["t_iter_E1"]
    w, h, l, r = 640, 200, 70, 40
    scale = (w - l - r) / 22.0
    o = [f'<svg viewBox="0 0 {w} {h}" role="img" aria-label="Seconds per iteration of 131,072 decisions at two epochs and at one epoch">']
    for v in range(0, 23, 2):
        X = l + v * scale
        o.append(f'<line class="grid" x1="{X:.1f}" x2="{X:.1f}" y1="20" y2="140"/>')
        o.append(f'<text class="muted" x="{X:.1f}" y="158" text-anchor="middle">{v}</text>')
    o.append(f'<text x="{l + 11 * scale:.0f}" y="186" text-anchor="middle">seconds per iteration of 131,072 decisions</text>')

    def seg(y, x0, x1, cls, op, label):
        o.append(f'<rect class="{cls}" x="{l + x0 * scale:.1f}" y="{y}" width="{(x1 - x0) * scale:.1f}" height="30" opacity="{op}"/>')
        if label:
            o.append(f'<text x="{l + (x0 + x1) / 2 * scale:.1f}" y="{y + 19}" text-anchor="middle" style="font-size:11px">{label}</text>')

    wait = 4.0
    c, k = e2["collect"], e2["cut"]
    o.append('<text x="8" y="49">E = 2</text>')
    seg(30, 0, c, "f4", 0.35, "collect 12.9")
    seg(30, c, c + k, "f3", 0.6, "")
    seg(30, c + k, c + k + wait, "f2", 0.55, "wait ~4")
    seg(30, c + k + wait, e2["total"], "f4", 0.15, "")
    o.append(f'<text x="{l + e2["total"] * scale + 4:.1f}" y="49" style="font-size:11px">20.3</text>')
    o.append('<text x="8" y="109">E = 1</text>')
    seg(90, 0, t1, "f1", 0.35, "")
    o.append(f'<text x="{l + t1 * scale / 2:.1f}" y="109" text-anchor="middle" style="font-size:11px">131,072 ÷ 9,100 per second</text>')
    o.append(f'<text x="{l + t1 * scale + 4:.1f}" y="109" style="font-size:11px">{t1:.1f}</text>')
    o.append("</svg>")
    return "\n".join(o)


def fig_ddr(s):
    ds = s["campaign"]["doublings"]
    pl = Plot(h=300, xlim=(0.4, 3.6), ylim=(0, 250), bottom=58,
              label="Decisions per update divided by probe KL, before and after each doubling")
    pl.axes([], [0, 50, 100, 150, 200, 250], "", "decisions per unit of probe KL (millions)", yfmt=lambda v: f"{v}")
    for i, d in enumerate(ds):
        x = i + 1
        for dx, v, cls in ((-0.17, d["ddr_before"] / 1e6, "f4"), (0.17, d["ddr_after"] / 1e6, "f1")):
            x0, x1 = pl.x(x + dx - 0.14), pl.x(x + dx + 0.14)
            pl.out.append(f'<rect class="{cls}" x="{x0:.1f}" y="{pl.y(v):.1f}" width="{x1 - x0:.1f}" '
                          f'height="{pl.y(0) - pl.y(v):.1f}" opacity="0.8"/>')
            pl.text(x + dx, v, f"{v:.0f}", "", "middle", dy=-5, size=11)
        pl.text(x, 0, d["name"], "", "middle", dy=18)
        pl.text(x, 0, f"level step {d['step_pp']:+.1f} pp", "muted", "middle", dy=34, size=11)
    pl.legend([("f4", "before the doubling"), ("f1", "after")], x0=pl.l + 14, y0=pl.t + 10)
    return pl.svg()


def fig_floor(s):
    f = s["toy_floor"]
    pl = Plot(h=300, xlim=(0.002, 0.5), ylim=(0.02, 5), xlog=True, ylog=True,
              label="Toy: loss floor of SGD on a noisy quadratic against learning rate over batch size")
    pl.axes([0.003, 0.01, 0.03, 0.1, 0.3], [0.03, 0.1, 0.3, 1, 3], "learning rate ÷ batch size (α / B)",
            "loss above the optimum", xfmt=lambda v: f"{v:g}", yfmt=lambda v: f"{v:g}")
    xs = np.geomspace(0.002, 0.5, 50)
    pl.line([(x, x * f["tr_sigma"] / 4) for x in xs], "l4", 1.5, "5 4")
    for p in f["grid"]:
        pl.dot(p["alpha"] / p["B"], p["measured"], "f1", 3.2)
    pl.legend([("f1", "measured: 4 learning rates × 5 batch sizes"), ("l4", "α tr(Σ) / 4B, the small-step limit")], y0=pl.t + 10)
    return pl.svg()


def fig_schedule(s):
    sch = s["toy_floor"]["schedule"]
    tr = sch["trace"]
    pl = Plot(h=290, xlim=(0, tr[-1][0]), ylim=(0.3, 100), ylog=True,
              label="Toy: loss against decisions as the learning rate halves and the batch doubles twice")
    pl.axes([0, 1000, 2000, 3000, 4000], [0.3, 1, 3, 10, 30, 100], "decisions (gradient samples)",
            "loss above the optimum (log scale)", xfmt=lambda v: f"{v:,}", yfmt=lambda v: f"{v:g}")
    pl.line([(a, b) for a, b in tr], "l1", 1.6)
    labs = ["α halved", "B doubled", "B doubled"]
    for x, lab in zip(sch["boundaries"], labs):
        pl.vline(x)
        pl.text(x, 60, lab, "muted", "start", dx=4, size=11)
    return pl.svg()


def fig_epochs(s):
    t = s["toy_epochs"]
    iters = 60
    arms = t["by_lr"][t["lr_shown"]]["arms"]
    pl = Plot(h=300, xlim=(0, iters * t["B"]), ylim=(0, 1),
              label="Toy: bandit score against decisions at 1, 2, 4 and 8 epochs, learning rate 4")
    pl.axes([0, 10000, 20000, 30000], [0, 0.25, 0.5, 0.75, 1.0], "decisions",
            "score (0 random, 1 best)", xfmt=lambda v: f"{v:,}", yfmt=lambda v: f"{v:g}")
    pl.line([(0, t["milestone"]), (iters * t["B"], t["milestone"])], "l4", 1, "4 4")
    for e, cls in (("1", "l1"), ("2", "l2"), ("4", "l3"), ("8", "l4")):
        cur = arms[e]["mean_curve"][:iters + 1]
        pl.line([(i * t["B"], v) for i, v in enumerate(cur)], cls, 2)
    pl.legend([("l1", "E = 1"), ("l2", "E = 2"), ("l3", "E = 4"), ("l4", "E = 8")], x0=pl.w - 140, y0=pl.h - pl.b - 76)
    return pl.svg()


def fig_seeds(s):
    t = s["toy_seeds"]
    iters = t["iters"]
    pl = Plot(h=300, xlim=(0, iters * t["B"]), ylim=(0, 1),
              label="Toy: twelve training seeds of each of two arms")
    pl.axes([0, 20000, 40000, 60000], [0, 0.25, 0.5, 0.75, 1.0], "decisions", "score (0 random, 1 best)",
            xfmt=lambda v: f"{v:,}", yfmt=lambda v: f"{v:g}")
    for k, cls in (("A", "l2"), ("B", "l1")):
        for cur in t["curves_shown"][k]:
            pl.line([(i * t["B"], v) for i, v in enumerate(cur)], cls, 1.1, opacity="0.75")
    pl.line([(0, 0.9), (iters * t["B"], 0.9)], "l4", 1, "4 4")
    pl.legend([("l2", f"arm A, learning rate {t['lr']['A']}"), ("l1", f"arm B, learning rate {t['lr']['B']}")],
              x0=pl.w - 250, y0=pl.h - pl.b - 40)
    return pl.svg()


FIGURES = {"campaign": fig_campaign, "timing": fig_timing, "ddr": fig_ddr, "floor": fig_floor,
           "schedule": fig_schedule, "epochs": fig_epochs, "seeds": fig_seeds}


def inject(html: str, name: str, svg: str) -> str:
    pat = re.compile(rf"(<!-- figure:{name} -->).*?(<!-- /figure:{name} -->)", re.S)
    if not pat.search(html):
        raise SystemExit(f"chapter.html has no <!-- figure:{name} --> block")
    return pat.sub(lambda m: m.group(1) + "\n" + svg + "\n" + m.group(2), html)


def main() -> None:
    s = {"campaign": campaign()}
    s["toy_floor"] = toy_floor(np.random.default_rng(3))
    s["toy_epochs"] = toy_epochs()
    s["toy_seeds"] = toy_seeds()
    OUT.write_text(json.dumps(s, indent=1) + "\n", encoding="utf-8")
    brief = {
        "time_ratio": s["campaign"]["time_ratio"],
        "ddr": [(d["name"], round(d["ddr_before"] / 1e6, 1), round(d["ddr_after"] / 1e6, 1),
                 round(d["kl_ratio"], 2), round(d["rate_ratio"], 3)) for d in s["campaign"]["doublings"]],
        "floor_schedule": s["toy_floor"]["schedule"]["floors"],
        "floor_grid": [(p["alpha"], p["B"], round(p["measured"], 4), round(p["predicted"], 4))
                       for p in s["toy_floor"]["grid"]],
        "epochs": {lr: ({e: a["d_star_median"] for e, a in v["arms"].items()}, round(v["ratio_1_to_2"], 3))
                   for lr, v in s["toy_epochs"]["by_lr"].items()},
        "seeds": {k: s["toy_seeds"][k] for k in ("late_mean", "late_sd", "late_range", "early_late_spearman",
                                                  "auc_late_spearman", "true_better", "wrong_pick", "familywise")},
    }
    print(json.dumps(brief, indent=1))
    if CHAPTER.exists():
        html = CHAPTER.read_text(encoding="utf-8")
        for name, draw in FIGURES.items():
            html = inject(html, name, draw(s))
        CHAPTER.write_text(html, encoding="utf-8")
        print(f"updated {len(FIGURES)} figures in {CHAPTER.name}")


if __name__ == "__main__":
    main()
