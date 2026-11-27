"""Data and figures for Chapter 5 (chapter.html).

    python figures.py extract RUN_DIR...   # learners' --keep directories -> runs.json
    python figures.py render               # runs.json -> the SVG figures inside chapter.html

`extract` reads each arm's `stats.jsonl` and `learning_check.json` from the
directories `check.py --keep DIR` leaves (one sub-directory per arm) and keeps
the per-iteration columns the chapter plots. `render` rewrites every block
between `<!-- figure:NAME -->` and `<!-- /figure:NAME -->` in chapter.html.
Plain Python, no third-party packages: it runs with any python3.
"""

from __future__ import annotations

import json
import math
import re
import sys
from pathlib import Path

STOP = Path(__file__).resolve().parent
DATA = STOP / "runs.json"
CHAPTER = STOP / "chapter.html"

ARM_LABEL = {
    "clipped": "clipped (ε = 0.2, base lr, 2 epochs)",
    "unclipped": "unclipped (no clip, 10× lr, 4 epochs)",
    "clipped_fast": "clipped, fast (ε = 0.2, 10× lr, 4 epochs)",
}
ARM_CLASS = {"clipped": "1", "unclipped": "2", "clipped_fast": "3"}


# ── extract ─────────────────────────────────────────────────────────────────


def _num(x):
    return float(x) if isinstance(x, (int, float)) and math.isfinite(x) else None


def extract_arm(d: Path) -> dict:
    rows = []
    for line in (d / "stats.jsonl").read_text().splitlines():
        if not line.strip():
            continue
        r = json.loads(line)
        u = r["update"]
        rows.append(
            {
                "it": r["it"],
                "phase": u.get("phase"),
                "decisions": r["decisions"],
                "time": r["time"],
                "runs": r["runs"],
                "floor": _num(r.get("floor_mean")),
                "kl_probe": _num(u.get("probe_kl")),
                "kl_probe_epochs": [_num(x) for x in u.get("probe_kl_epochs") or []],
                "kl_k3": _num(u.get("kl")),
                "clipfrac": _num(u.get("clipfrac")),
                "dualfrac": _num(u.get("dualfrac")),
                "entropy": _num(u.get("ent_n")),
                "ev_combat": _num(u.get("ev_combat")),
                "grad_norm": _num(u.get("gradnorm_mean")),
                "lr_trunk": _num(u.get("lr_trunk")),
                "dps": _num(r.get("dec_per_s")),
            }
        )
    cfg = json.loads((d / "cfg.json").read_text())
    vp = d / "learning_check.json"
    v = json.loads(vp.read_text()) if vp.exists() else {}
    keep = ("pass", "window", "complete", "first_mean", "last_mean", "gain", "ev_combat")
    return {
        "config": {
            k: cfg[k]
            for k in ("clip", "epochs", "iter_decisions", "minibatch", "workers", "rank_threads", "net", "sched")
        },
        "verdict": {k: v.get(k) for k in keep},
        "rows": rows,
    }


def extract(dirs: list[Path]) -> None:
    data = json.loads(DATA.read_text()) if DATA.exists() else {"arms": {}}
    for root in dirs:
        for d in sorted(p for p in root.iterdir() if (p / "stats.jsonl").exists()):
            data["arms"][d.name] = extract_arm(d)
            print(f"{d.name}: {len(data['arms'][d.name]['rows'])} iterations")
    data["note"] = (
        "Per-iteration statistics of ppo.e2e.learner learning checks run by check.py on an "
        "M2 Max, CPU only, 2x128 net. See chapter.html, section 'Measured'."
    )
    DATA.write_text(json.dumps(data, indent=1) + "\n")


# ── summaries the chapter quotes ────────────────────────────────────────────


def median(xs):
    xs = sorted(x for x in xs if x is not None)
    if not xs:
        return math.nan
    n = len(xs)
    return xs[n // 2] if n % 2 else 0.5 * (xs[n // 2 - 1] + xs[n // 2])


def summary(arm: dict) -> dict:
    rows = arm["rows"]
    pol = [r for r in rows if r["phase"] != "V-warm"]
    ent = [r["entropy"] for r in pol if r["entropy"] is not None]
    a, b = rows[1], rows[-1]  # from the end of the first iteration: steady throughput
    return {
        "iterations": len(rows),
        "decisions": rows[-1]["decisions"],
        "floor_first_window": arm["verdict"]["first_mean"],
        "floor_last_window": arm["verdict"]["last_mean"],
        "kl_probe_median": median([r["kl_probe"] for r in pol]),
        "kl_probe_max": max(r["kl_probe"] or 0 for r in pol),
        "kl_k3_median": median([r["kl_k3"] for r in pol]),
        "clipfrac_median": median([r["clipfrac"] for r in pol]),
        "clipfrac_max": max(r["clipfrac"] or 0 for r in pol),
        "entropy_first": ent[0],
        "entropy_last3": sum(ent[-3:]) / 3,
        "entropy_min": min(ent),
        "ev_last5": sum(r["ev_combat"] for r in rows[-5:]) / 5,
        "grad_norm_median": median([r["grad_norm"] for r in pol]),
        "steady_dps": (b["decisions"] - a["decisions"]) / (b["time"] - a["time"]),
    }


# ── a small SVG plotting kit ────────────────────────────────────────────────


def f(x: float) -> str:
    return f"{x:.1f}"


class Axes:
    """A plot box in an SVG of width W, mapping data to pixels."""

    def __init__(self, x0, y0, w, h, xlim, ylim, ylog=False):
        self.x0, self.y0, self.w, self.h = x0, y0, w, h
        self.xlim, self.ylim, self.ylog = xlim, ylim, ylog

    def X(self, x):
        a, b = self.xlim
        return self.x0 + (x - a) / (b - a) * self.w

    def Y(self, y):
        a, b = self.ylim
        if self.ylog:
            y, a, b = math.log10(max(y, 1e-300)), math.log10(a), math.log10(b)
        return self.y0 + self.h - (y - a) / (b - a) * self.h

    def frame(self, xticks, yticks, xlabel, ylabel, xfmt=str, yfmt=str):
        out = []
        for t in yticks:
            y = self.Y(t)
            out.append(f'<line class="grid" x1="{f(self.x0)}" y1="{f(y)}" x2="{f(self.x0 + self.w)}" y2="{f(y)}"/>')
            out.append(f'<text class="muted" x="{f(self.x0 - 6)}" y="{f(y + 4)}" text-anchor="end">{yfmt(t)}</text>')
        for t in xticks:
            x = self.X(t)
            out.append(f'<line class="ink" x1="{f(x)}" y1="{f(self.y0 + self.h)}" x2="{f(x)}" y2="{f(self.y0 + self.h + 4)}"/>')
            out.append(f'<text class="muted" x="{f(x)}" y="{f(self.y0 + self.h + 17)}" text-anchor="middle">{xfmt(t)}</text>')
        out.append(f'<line class="ink" x1="{f(self.x0)}" y1="{f(self.y0 + self.h)}" x2="{f(self.x0 + self.w)}" y2="{f(self.y0 + self.h)}"/>')
        out.append(f'<line class="ink" x1="{f(self.x0)}" y1="{f(self.y0)}" x2="{f(self.x0)}" y2="{f(self.y0 + self.h)}"/>')
        out.append(f'<text x="{f(self.x0 + self.w / 2)}" y="{f(self.y0 + self.h + 34)}" text-anchor="middle">{xlabel}</text>')
        cy = self.y0 + self.h / 2
        out.append(f'<text x="{f(self.x0 - 46)}" y="{f(cy)}" text-anchor="middle" transform="rotate(-90 {f(self.x0 - 46)} {f(cy)})">{ylabel}</text>')
        return out

    def line(self, pts, cls, width=2, dash=None, dots=False):
        pts = [(x, y) for x, y in pts if y is not None]
        d = " ".join(f"{'M' if i == 0 else 'L'}{f(self.X(x))} {f(self.Y(y))}" for i, (x, y) in enumerate(pts))
        da = f' stroke-dasharray="{dash}"' if dash else ""
        out = [f'<path class="l{cls}" d="{d}" stroke-width="{width}"{da}/>']
        if dots:
            out += [f'<circle class="f{cls}" cx="{f(self.X(x))}" cy="{f(self.Y(y))}" r="2.6"/>' for x, y in pts]
        return out


def legend(x, y, items):
    out = []
    for i, (cls, text, dash) in enumerate(items):
        yy = y + 18 * i
        da = f' stroke-dasharray="{dash}"' if dash else ""
        out.append(f'<line class="l{cls}" x1="{x}" y1="{yy}" x2="{x + 22}" y2="{yy}" stroke-width="2.5"{da}/>')
        out.append(f'<text x="{x + 28}" y="{yy + 4}">{text}</text>')
    return out


def svg(w, h, label, body):
    return (
        f'<svg viewBox="0 0 {w} {h}" role="img" aria-label="{label}">'
        + "".join(body)
        + "</svg>"
    )


# ── the figures ─────────────────────────────────────────────────────────────


def fig_clip(panels: str, dual: bool) -> str:
    """One term of the PPO objective against the ratio. `panels` is "pos"
    (A > 0 only) or "both" (A > 0 and A < 0). `dual` adds the dual clip
    (c = 3) that the project applies when A < 0. The chapter draws three
    versions in sequence, each adding one part to the last."""
    eps, c = 0.2, 3.0
    body = []
    specs = [(1.0, "Â > 0 (here Â = +1)")]
    if panels == "both":
        specs.append((-1.0, "Â < 0 (here Â = −1)"))
    for i, (A, title) in enumerate(specs):
        x0 = 60 + 320 * i
        ax = Axes(x0, 34, 240, 190, (0, 4), (-3.6, 1.4) if A < 0 else (-0.2, 4.2))
        yt = [-3, -2, -1, 0, 1] if A < 0 else [0, 1, 2, 3, 4]
        body += ax.frame([0, 1, 2, 3, 4], yt, "ratio ρ = π_θ / π_old", "objective term", yfmt=lambda t: f"{t:g}")
        lo, hi = ax.X(1 - eps), ax.X(1 + eps)
        # Where the gradient with respect to rho is zero: the flat parts.
        if A > 0:
            body.append(f'<rect x="{f(hi)}" y="{ax.y0}" width="{f(ax.X(4) - hi)}" height="{ax.h}" class="f4" opacity="0.13"/>')
        else:
            body.append(f'<rect x="{f(ax.X(0))}" y="{ax.y0}" width="{f(lo - ax.X(0))}" height="{ax.h}" class="f4" opacity="0.13"/>')
            if dual:
                body.append(f'<rect x="{f(ax.X(c))}" y="{ax.y0}" width="{f(ax.X(4) - ax.X(c))}" height="{ax.h}" class="f4" opacity="0.13"/>')
        rs = [k / 200 * 4 for k in range(201)]
        body += ax.line([(r, r * A) for r in rs], 4, width=1.5, dash="5 4")

        def term(r):
            s = min(r * A, max(1 - eps, min(1 + eps, r)) * A)
            if A < 0 and dual:
                s = max(s, c * A)
            return max(s, ax.ylim[0])

        body += ax.line([(r, term(r)) for r in rs], 1, width=2.6)
        body.append(f'<circle class="f2" cx="{f(ax.X(1))}" cy="{f(ax.Y(A))}" r="4"/>')
        body.append(f'<text x="{f(x0 + 120)}" y="20" text-anchor="middle">{title}</text>')
        if A > 0:
            body.append(f'<text class="muted" x="{f(hi + 6)}" y="{f(ax.y0 + 16)}">flat: no gradient</text>')
            body.append(f'<text class="muted" x="{f(ax.X(2.2))}" y="{f(ax.Y(3.2))}">ρÂ (unclipped)</text>')
        else:
            body.append(f'<text class="muted" x="{f(ax.X(0.05))}" y="{f(ax.y0 + 16)}">flat</text>')
            if dual:
                body.append(f'<text class="muted" x="{f(ax.X(c) + 4)}" y="{f(ax.y0 + 16)}">dual clip</text>')
            body.append(f'<text class="muted" x="{f(ax.X(1.5))}" y="{f(ax.Y(-2.4))}">ρÂ</text>')
    return svg(320 * len(specs), 270, "The clipped PPO objective as a function of the probability ratio", body)


def fig_flow(stage: int) -> str:
    """One iteration of the project's learner, drawn in five stages. Each
    stage redraws the previous one and adds one part, in a fixed layout."""
    box = [
        (20, 20, "Rollout workers", ["Rust simulator", "the learner samples a ~ μ"]),
        (240, 20, "Batch of decisions", ["131,072 per iteration"]),
        (460, 20, "Pre-pass (no grad)", ["weights at update start"]),
        (460, 140, "Returns, advantages", ["GAE (Ch. 4), normalised"]),
        (240, 140, "2 epochs × minibatches", ["clipped loss + value + entropy", "grad clip 1.0, AdamW"]),
        (20, 140, "Probe KL", ["exact, 8,192 rows, per epoch"]),
    ]
    out = ['<defs><marker id="arr5-' + str(stage) + '" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0L10 5L0 10z" fill="currentColor"/></marker></defs>']
    out.append('<g color="var(--fg)">')
    arrow = f'marker-end="url(#arr5-{stage})"'
    for i, (x, y, title, lines) in enumerate(box[: stage + 1]):
        out.append(f'<rect class="box" x="{x}" y="{y}" width="160" height="62" rx="3"/>')
        ty = y + (20 if len(lines) > 1 else 24)
        out.append(f'<text x="{x + 80}" y="{ty}" text-anchor="middle">{title}</text>')
        for j, line in enumerate(lines):
            out.append(f'<text class="muted small" x="{x + 80}" y="{ty + 17 + 15 * j}" text-anchor="middle">{line}</text>')
    edges = [
        ('<line class="ink" x1="180" y1="51" x2="240" y2="51" {a}/>', '<text class="muted small" x="210" y="100" text-anchor="middle">tokens, a_t, log μ, r_t</text>'),
        ('<line class="ink" x1="400" y1="51" x2="460" y2="51" {a}/>', ""),
        ('<line class="ink" x1="540" y1="82" x2="540" y2="140" {a}/>', '<text class="muted small" x="532" y="115" text-anchor="end">V_φ(s_t), log π_old(a_t|s_t)</text>'),
        ('<line class="ink" x1="460" y1="171" x2="400" y2="171" {a}/>', '<text class="muted small" x="430" y="222" text-anchor="middle">Â_t, V^targ</text>'),
        ('<line class="ink" x1="240" y1="171" x2="180" y2="171" {a}/>', '<text class="muted small" x="210" y="222" text-anchor="middle">new θ</text>'),
    ]
    for k in range(stage):
        line, label = edges[k]
        out.append(line.format(a=arrow))
        if label:
            out.append(label)
    if stage >= 4:
        # The new weights go back to the acting network.
        out.append('<line class="ink" x1="320" y1="202" x2="320" y2="290"/>')
        out.append(f'<path class="ink" d="M320 290 L630 290 L630 6 L100 6 L100 20" fill="none" stroke-dasharray="4 4" {arrow}/>')
        out.append('<text class="muted small" x="470" y="310" text-anchor="middle">new weights replace the acting network between inference batches</text>')
    if stage >= 5:
        out.append('<path class="ink" d="M100 202 L100 270 L320 270" fill="none"/>')
        out.append('<text class="muted small" x="110" y="258">KL &gt; 0.06 in U1, U2: stop</text>')
    out.append("</g>")
    return f'<svg class="flow" viewBox="0 0 640 320" role="img" aria-label="Data flow of one PPO iteration in Spire Mentor, stage {stage} of 5">' + "".join(out) + "</svg>"


def fig_baseline() -> str:
    """Variance of the one-sample score-function gradient against the baseline b,
    for a softmax over three actions (exact, not simulated)."""
    p = [0.5, 0.3, 0.2]
    R = [0.30, 0.25, 0.20]

    def grad_sq(a):  # |e_a - p|^2: the squared norm of d log pi(a) / d logits
        return sum(((1.0 if i == a else 0.0) - p[i]) ** 2 for i in range(3))

    def mean_g():
        return [sum(p[a] * R[a] * ((1.0 if i == a else 0.0) - p[i]) for a in range(3)) for i in range(3)]

    mg = mean_g()
    m2 = sum(x * x for x in mg)

    def var(b):
        return sum(p[a] * (R[a] - b) ** 2 * grad_sq(a) for a in range(3)) - m2

    V = sum(pi * r for pi, r in zip(p, R))
    bstar = sum(p[a] * R[a] * grad_sq(a) for a in range(3)) / sum(p[a] * grad_sq(a) for a in range(3))
    bs = [-0.1 + i * 0.5 / 200 for i in range(201)]
    vmax = max(var(b) for b in bs)
    ax = Axes(80, 20, 520, 200, (-0.1, 0.4), (0, vmax * 1.05))
    body = ax.frame(
        [-0.1, 0, 0.1, 0.2, 0.3, 0.4],
        [0, 0.02, 0.04, 0.06],
        "baseline b (reward units)",
        "variance of ĝ",
        xfmt=lambda t: f"{t:g}",
        yfmt=lambda t: f"{t:g}",
    )
    body += ax.line([(b, var(b)) for b in bs], 1, width=2.6)
    for b, name, dy in [(0.0, "b = 0 (REINFORCE)", -8), (V, "b = V = Σπ·G", -8), (bstar, "b*", 16)]:
        body.append(f'<circle class="f2" cx="{f(ax.X(b))}" cy="{f(ax.Y(var(b)))}" r="4"/>')
        anchor = "start" if b < 0.1 else "end"
        dx = 8 if anchor == "start" else -8
        body.append(f'<text x="{f(ax.X(b) + dx)}" y="{f(ax.Y(var(b)) + dy)}" text-anchor="{anchor}">{name}</text>')
    return svg(640, 270, "Gradient variance against the baseline for a three-action softmax", body)


def _series(arm, key, x="decisions", scale=1.0):
    return [(r[x] / scale, r[key]) for r in arm["rows"] if r["phase"] != "V-warm" or key == "floor"]


def fig_floor(data) -> str:
    arms = data["arms"]
    xmax = max(a["rows"][-1]["decisions"] for a in arms.values()) / 1000
    ax = Axes(80, 20, 520, 220, (0, math.ceil(xmax / 50) * 50), (0, 10))
    body = ax.frame(list(range(0, int(ax.xlim[1]) + 1, 50)), [0, 2, 4, 6, 8, 10],
                    "decisions collected (thousands)", "mean floor of finished runs")
    items = []
    for name in ("clipped", "unclipped", "clipped_fast"):
        if name in arms:
            cls = ARM_CLASS[name]
            body += ax.line(_series(arms[name], "floor", scale=1000), cls, dots=True)
            items.append((cls, ARM_LABEL[name], None))
    body += legend(96, 34, items)
    return svg(640, 290, "Mean floor reached per iteration in each arm", body)


def fig_kl(data) -> str:
    arms = data["arms"]
    xmax = max(a["rows"][-1]["decisions"] for a in arms.values()) / 1000
    ax = Axes(80, 20, 520, 220, (0, math.ceil(xmax / 50) * 50), (1e-4, 100), ylog=True)
    body = ax.frame(list(range(0, int(ax.xlim[1]) + 1, 50)), [1e-4, 1e-3, 1e-2, 1e-1, 1, 10, 100],
                    "decisions collected (thousands)", "KL(π_old ‖ π_new), nats",
                    yfmt=lambda t: f"{t:g}")
    for y, txt in [(0.02, "0.02: the campaign's upper KL target"), (0.06, "0.06: the campaign's hard stop")]:
        yy = ax.Y(y)
        body.append(f'<line class="l4" x1="{ax.x0}" y1="{f(yy)}" x2="{ax.x0 + ax.w}" y2="{f(yy)}" stroke-dasharray="3 4"/>')
        body.append(f'<text class="muted" x="{ax.x0 + ax.w - 4}" y="{f(yy - 4)}" text-anchor="end">{txt}</text>')
    items = []
    for name in ("clipped", "unclipped", "clipped_fast"):
        if name in arms:
            cls = ARM_CLASS[name]
            pts = [(x, max(y, 1.1e-4)) for x, y in _series(arms[name], "kl_probe", scale=1000) if y is not None]
            body += ax.line(pts, cls, dots=True)
            items.append((cls, ARM_LABEL[name], None))
    body += legend(96, 30, items)
    return svg(640, 290, "KL divergence between the policy before and after each update", body)


def fig_entropy_ev(data) -> str:
    arms = data["arms"]
    xmax = max(a["rows"][-1]["decisions"] for a in arms.values()) / 1000
    xm = math.ceil(xmax / 50) * 50
    body = []
    a1 = Axes(70, 30, 230, 190, (0, xm), (0, 1.05))
    a2 = Axes(390, 30, 230, 190, (0, xm), (-1.5, 1.05))
    body += a1.frame(list(range(0, xm + 1, 100)), [0, 0.25, 0.5, 0.75, 1], "decisions (thousands)",
                     "entropy / log |𝒜(s)|", yfmt=lambda t: f"{t:g}")
    body += a2.frame(list(range(0, xm + 1, 100)), [-1.5, -1, -0.5, 0, 0.5, 1], "decisions (thousands)",
                     "explained variance", yfmt=lambda t: f"{t:g}")
    body.append('<text x="185" y="16" text-anchor="middle">Policy entropy</text>')
    body.append('<text x="505" y="16" text-anchor="middle">Critic: explained variance (combat)</text>')
    for name in ("clipped", "unclipped", "clipped_fast"):
        if name in arms:
            cls = ARM_CLASS[name]
            body += a1.line(_series(arms[name], "entropy", scale=1000), cls)
            ev = [(x, max(-1.5, y)) for x, y in _series(arms[name], "ev_combat", scale=1000) if y is not None]
            body += a2.line(ev, cls)
    return svg(640, 270, "Policy entropy and critic explained variance per iteration", body)


def fig_clipfrac(data) -> str:
    arms = data["arms"]
    xmax = max(a["rows"][-1]["decisions"] for a in arms.values()) / 1000
    ax = Axes(80, 20, 520, 200, (0, math.ceil(xmax / 50) * 50), (0, 0.2))
    body = ax.frame(list(range(0, int(ax.xlim[1]) + 1, 50)), [0, 0.05, 0.1, 0.15, 0.2],
                    "decisions collected (thousands)", "clip fraction", yfmt=lambda t: f"{t:g}")
    items = []
    for name in ("clipped", "clipped_fast"):
        if name in arms:
            cls = ARM_CLASS[name]
            body += ax.line(_series(arms[name], "clipfrac", scale=1000), cls, dots=True)
            items.append((cls, ARM_LABEL[name], None))
    body += legend(330, 30, items)
    return svg(640, 270, "Share of policy rows whose ratio left the clip band, per iteration", body)


def render() -> None:
    data = json.loads(DATA.read_text())
    figs = {
        "clip-pos": fig_clip("pos", dual=False),
        "clip-neg": fig_clip("both", dual=False),
        "clip-dual": fig_clip("both", dual=True),
        **{f"flow-{k}": fig_flow(k) for k in range(1, 6)},
        "baseline": fig_baseline(),
        "floor": fig_floor(data),
        "kl": fig_kl(data),
        "entropy-ev": fig_entropy_ev(data),
        "clipfrac": fig_clipfrac(data),
    }
    html = CHAPTER.read_text()
    for name, s in figs.items():
        pat = re.compile(rf"(<!-- figure:{re.escape(name)} -->).*?(<!-- /figure:{re.escape(name)} -->)", re.S)
        if not pat.search(html):
            raise SystemExit(f"chapter.html has no block for figure {name!r}")
        html = pat.sub(lambda m: m.group(1) + s + m.group(2), html)
    CHAPTER.write_text(html)
    for name, arm in data["arms"].items():
        print(name, json.dumps({k: round(v, 5) if isinstance(v, float) else v for k, v in summary(arm).items()}))


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "extract":
        extract([Path(p) for p in sys.argv[2:]])
    elif cmd == "render":
        render()
    else:
        raise SystemExit(__doc__)
