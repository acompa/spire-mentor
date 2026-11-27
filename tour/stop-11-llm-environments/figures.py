"""Toy experiment and figures for Chapter 11 (chapter.html).

    python3 figures.py run      # simulate the toy problem -> toy.json
    python3 figures.py render   # toy.json -> the SVG plots inside chapter.html

The toy is a policy-gradient problem, not a language model. There are 64 tasks.
Task x is solved with probability p_x, spread evenly over [0.05, 0.95]. On each
task the policy picks one of three actions: "solve" (true success with
probability p_x), "hack" (never a true success) or "give up" (never a true
success). A verifier scores each attempt with 0 or 1 and makes two kinds of
error: a false pass on an attempt that failed, and a false failure on an attempt
that succeeded. The policy is a softmax over the three actions, one set of
logits per task, trained by REINFORCE with a leave-one-out baseline over 8
attempts per task per update (Chapter 10 derives that baseline).

Experiment A: every failed attempt passes with the same rate e_pass, whatever
the action. Experiment B: only the hack passes falsely, with rate e_hack.

`render` rewrites every block between `<!-- figure:NAME -->` and
`<!-- /figure:NAME -->` in chapter.html. Needs NumPy for `run`; `render` is
plain Python.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

STOP = Path(__file__).resolve().parent
DATA = STOP / "toy.json"
CHAPTER = STOP / "chapter.html"

N_TASKS = 64
GROUP = 8  # attempts per task per update
LR = 0.5  # Experiment B; Experiment A uses LR_A
LR_A = 0.1
SEEDS = 10
INIT_LOGITS = (1.0, -1.5, 0.0)  # solve, hack, give up: probabilities 0.69, 0.06, 0.25
SOLVE, HACK, GIVEUP = 0, 1, 2


def task_difficulty():
    import numpy as np

    return np.linspace(0.05, 0.95, N_TASKS)


def train(e_pass: float, e_fail: float, e_hack: float, steps: int, seed: int, lr: float = LR, log_every: int = 10):
    """Train the tabular softmax policy; return the logged curves and final policy.

    e_pass: false-pass rate on a failed solve attempt and on giving up.
    e_hack: false-pass rate on the hack (Experiment A sets it equal to e_pass).
    e_fail: false-failure rate on a true success.
    """
    import numpy as np

    rng = np.random.default_rng(seed)
    p = task_difficulty()
    z = np.tile(np.array(INIT_LOGITS), (N_TASKS, 1))
    fp = np.array([e_pass, e_hack, e_pass])  # false-pass rate by action
    log = {"step": [], "true": [], "verifier": [], "hack": []}

    def record(t, pi):
        true = float(np.mean(pi[:, SOLVE] * p))
        ver = float(
            np.mean(
                pi[:, SOLVE] * (p * (1 - e_fail) + (1 - p) * e_pass)
                + pi[:, HACK] * e_hack
                + pi[:, GIVEUP] * e_pass
            )
        )
        log["step"].append(t)
        log["true"].append(true)
        log["verifier"].append(ver)
        log["hack"].append(float(np.mean(pi[:, HACK])))

    for t in range(steps + 1):
        pi = np.exp(z - z.max(axis=1, keepdims=True))
        pi /= pi.sum(axis=1, keepdims=True)
        if t % log_every == 0:
            record(t, pi)
        if t == steps:
            break
        # Sample GROUP attempts per task.
        u = rng.random((N_TASKS, GROUP))
        cdf = np.cumsum(pi, axis=1)
        a = (u[..., None] > cdf[:, None, :]).sum(axis=2)  # (tasks, group)
        a = np.minimum(a, 2)
        y = (a == SOLVE) & (rng.random((N_TASKS, GROUP)) < p[:, None])
        pass_if_true = rng.random((N_TASKS, GROUP)) >= e_fail
        pass_if_false = rng.random((N_TASKS, GROUP)) < fp[a]
        v = np.where(y, pass_if_true, pass_if_false).astype(float)
        # Leave-one-out baseline over the group.
        b = (v.sum(axis=1, keepdims=True) - v) / (GROUP - 1)
        adv = v - b
        onehot = np.eye(3)[a]  # (tasks, group, 3)
        grad = (adv[..., None] * (onehot - pi[:, None, :])).mean(axis=1)
        z += lr * grad
    return log, pi


def run():
    import numpy as np

    out = {
        "config": {
            "tasks": N_TASKS,
            "group": GROUP,
            "lr_a": LR_A,
            "lr_b": LR,
            "seeds": SEEDS,
            "init_probs": [round(float(x), 4) for x in np.exp(INIT_LOGITS) / np.exp(INIT_LOGITS).sum()],
            "p_range": [0.05, 0.95],
        }
    }

    # Experiment A: the same false-pass rate on every failed attempt.
    steps_a = 1500
    target = 0.45  # 90% of the best achievable true success, mean(p_x) = 0.5
    exp_a = {}
    for e in [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7]:
        curves, hit = [], []
        for s in range(SEEDS):
            log, _ = train(e_pass=e, e_fail=0.0, e_hack=e, steps=steps_a, seed=100 + s, lr=LR_A, log_every=5)
            curves.append(log["true"])
            t = next((st for st, v in zip(log["step"], log["true"]) if v >= target), None)
            hit.append(t)
        mean_curve = np.mean(np.array(curves), axis=0)
        reached = [h for h in hit if h is not None]
        exp_a[f"{e:.1f}"] = {
            "e_pass": e,
            "kappa": 1 - e,
            "step": log["step"],
            "true_mean": [round(float(x), 5) for x in mean_curve],
            "steps_to_target": hit,
            "steps_to_target_median": float(np.median(reached)) if len(reached) == SEEDS else None,
            "final_true": round(float(mean_curve[-1]), 4),
        }
    out["exp_a"] = {"steps": steps_a, "target": target, "e_fail": 0.0, "runs": exp_a}

    # Experiment B: only the hack passes falsely, at rate e_hack.
    steps_b = 4000
    exp_b = []
    per_task = {}
    p = task_difficulty()
    for e in [round(0.05 * i, 2) for i in range(21)]:
        finals, pis = [], []
        for s in range(SEEDS):
            log, pi = train(e_pass=0.0, e_fail=0.0, e_hack=e, steps=steps_b, seed=200 + s, log_every=100)
            finals.append((log["true"][-1], log["verifier"][-1], log["hack"][-1]))
            pis.append(pi)
        f = np.mean(np.array(finals), axis=0)
        pi_mean = np.mean(np.array(pis), axis=0)
        exp_b.append(
            {
                "e_hack": e,
                "true": round(float(f[0]), 4),
                "verifier": round(float(f[1]), 4),
                "hack": round(float(f[2]), 4),
                "predicted_hack_share": round(float(np.mean(p < e)), 4),
                "predicted_true": round(float(np.mean(np.where(p > e, p, 0.0))), 4),
                "predicted_verifier": round(float(np.mean(np.maximum(p, e))), 4),
            }
        )
        if e in (0.3, 0.6):
            per_task[f"{e:.2f}"] = [round(float(x), 4) for x in pi_mean[:, HACK]]
    out["exp_b"] = {
        "steps": steps_b,
        "e_fail": 0.0,
        "e_pass": 0.0,
        "sweep": exp_b,
        "per_task_hack": per_task,
        "p": [round(float(x), 4) for x in p],
    }

    # Experiment B, one learning curve: the proxy rises while the truth falls.
    curves = []
    for s in range(SEEDS):
        log, _ = train(e_pass=0.0, e_fail=0.0, e_hack=0.6, steps=steps_b, seed=300 + s, log_every=20)
        curves.append([log["true"], log["verifier"], log["hack"]])
    c = np.mean(np.array(curves), axis=0)
    out["exp_b_curve"] = {
        "e_hack": 0.6,
        "step": log["step"],
        "true": [round(float(x), 5) for x in c[0]],
        "verifier": [round(float(x), 5) for x in c[1]],
        "hack": [round(float(x), 5) for x in c[2]],
    }
    DATA.write_text(json.dumps(out, indent=1) + "\n")
    print(f"wrote {DATA}")
    for k, r in exp_a.items():
        print(f"A e_pass={k} final_true={r['final_true']} steps_to_0.45 median={r['steps_to_target_median']}")
    for r in exp_b:
        print(
            f"B e_hack={r['e_hack']:.2f} hack={r['hack']:.3f} (pred {r['predicted_hack_share']:.3f}) "
            f"true={r['true']:.3f} (pred {r['predicted_true']:.3f}) ver={r['verifier']:.3f}"
        )


# ── render ──────────────────────────────────────────────────────────────────


def sub(base, s):
    return f'{base}<tspan baseline-shift="sub" font-size="9">{s}</tspan>'


class Axes:
    def __init__(self, x0, x1, y0, y1, left=80.0, right=600.0, top=20.0, bottom=240.0):
        self.x0, self.x1, self.y0, self.y1 = x0, x1, y0, y1
        self.left, self.right, self.top, self.bottom = left, right, top, bottom

    def x(self, v):
        return self.left + (v - self.x0) / (self.x1 - self.x0) * (self.right - self.left)

    def y(self, v):
        return self.bottom - (v - self.y0) / (self.y1 - self.y0) * (self.bottom - self.top)

    def frame(self, xticks, yticks, xlabel, ylabel, xfmt=str, yfmt=str):
        s = []
        for t in yticks:
            yy = self.y(t)
            s.append(f'<line class="grid" x1="{self.left:.1f}" y1="{yy:.1f}" x2="{self.right:.1f}" y2="{yy:.1f}"/>')
            s.append(f'<text class="muted" x="{self.left - 6:.1f}" y="{yy + 4:.1f}" text-anchor="end">{yfmt(t)}</text>')
        for t in xticks:
            xx = self.x(t)
            s.append(f'<line class="ink" x1="{xx:.1f}" y1="{self.bottom:.1f}" x2="{xx:.1f}" y2="{self.bottom + 4:.1f}"/>')
            s.append(f'<text class="muted" x="{xx:.1f}" y="{self.bottom + 17:.1f}" text-anchor="middle">{xfmt(t)}</text>')
        s.append(f'<line class="ink" x1="{self.left:.1f}" y1="{self.bottom:.1f}" x2="{self.right:.1f}" y2="{self.bottom:.1f}"/>')
        s.append(f'<line class="ink" x1="{self.left:.1f}" y1="{self.top:.1f}" x2="{self.left:.1f}" y2="{self.bottom:.1f}"/>')
        mx = (self.left + self.right) / 2
        my = (self.top + self.bottom) / 2
        s.append(f'<text x="{mx:.1f}" y="{self.bottom + 34:.1f}" text-anchor="middle">{xlabel}</text>')
        s.append(f'<text x="{self.left - 46:.1f}" y="{my:.1f}" text-anchor="middle" transform="rotate(-90 {self.left - 46:.1f} {my:.1f})">{ylabel}</text>')
        return s

    def path(self, xs, ys, cls, width=2.0, dash=None):
        pts = " L".join(f"{self.x(a):.1f} {self.y(b):.1f}" for a, b in zip(xs, ys))
        d = f' stroke-dasharray="{dash}"' if dash else ""
        return f'<path class="{cls}" d="M{pts}" stroke-width="{width}"{d}/>'

    def dots(self, xs, ys, cls, r=2.8):
        return "".join(f'<circle class="{cls}" cx="{self.x(a):.1f}" cy="{self.y(b):.1f}" r="{r}"/>' for a, b in zip(xs, ys))


def legend(items, x, y, dy=18):
    s = []
    for i, (cls, label, dash) in enumerate(items):
        yy = y + i * dy
        d = f' stroke-dasharray="{dash}"' if dash else ""
        s.append(f'<line class="{cls}" x1="{x}" y1="{yy}" x2="{x + 22}" y2="{yy}" stroke-width="2.5"{d}/>')
        s.append(f'<text x="{x + 28}" y="{yy + 4}">{label}</text>')
    return s


def svg(body, label, h=290):
    return f'<svg viewBox="0 0 640 {h}" role="img" aria-label="{label}">' + "".join(body) + "</svg>"


def fig_a_curves(d):
    a = d["exp_a"]
    xmax = 600
    ax = Axes(0, xmax, 0.3, 0.52)
    body = ax.frame(
        [0, 100, 200, 300, 400, 500, 600],
        [0.3, 0.35, 0.4, 0.45, 0.5],
        "policy updates",
        "true success rate",
        yfmt=lambda v: f"{v:.2f}",
    )
    body.append(f'<line class="l4" x1="{ax.left}" y1="{ax.y(0.5):.1f}" x2="{ax.right}" y2="{ax.y(0.5):.1f}" stroke-dasharray="3 4"/>')
    body.append(f'<text class="muted" x="{ax.right - 4}" y="{ax.y(0.5) - 5:.1f}" text-anchor="end">0.50: always attempt a solution</text>')
    items = []
    for key, cls in [("0.0", "l1"), ("0.3", "l2"), ("0.6", "l3")]:
        r = a["runs"][key]
        pts = [(x, y) for x, y in zip(r["step"], r["true_mean"]) if x <= xmax]
        body.append(ax.path([x for x, _ in pts], [y for _, y in pts], cls))
        items.append((cls, sub("e", "pass") + f" = {key}, κ = {1 - float(key):.1f}", None))
    body += legend(items, 380, 170)
    return svg(body, "True success rate against policy updates for three false-pass rates")


def fig_a_steps(d):
    a = d["exp_a"]
    runs = [r for r in a["runs"].values() if r["steps_to_target_median"] is not None]
    base = a["runs"]["0.0"]["steps_to_target_median"]
    ax = Axes(1.0, 3.5, 0, 3.5)
    body = ax.frame([1.0, 1.5, 2.0, 2.5, 3.0, 3.5], [0, 1, 2, 3], "1 / κ", "updates to reach 0.45, relative to κ = 1", xfmt=lambda v: f"{v:.1f}")
    xs = [1 / r["kappa"] for r in runs]
    ys = [r["steps_to_target_median"] / base for r in runs]
    body.append(ax.path([1.0, 3.5], [1.0, 3.5], "l4", width=1.5, dash="4 4"))
    body.append(f'<text class="muted" x="{ax.x(3.0):.1f}" y="{ax.y(3.3):.1f}" text-anchor="end">slope 1: time grows as 1/κ</text>')
    body.append(ax.path(xs, ys, "l1"))
    body.append(ax.dots(xs, ys, "f1"))
    for r, x, y in zip(runs, xs, ys):
        body.append(f'<text class="muted" x="{ax.x(x) + 6:.1f}" y="{ax.y(y) + 14:.1f}">{r["e_pass"]:.1f}</text>')
    return svg(body, "Updates needed to reach a true success rate of 0.45 against one over kappa")


def fig_b_sweep(d):
    b = d["exp_b"]["sweep"]
    ax = Axes(0, 1, 0, 1)
    body = ax.frame(
        [0, 0.2, 0.4, 0.6, 0.8, 1.0],
        [0, 0.25, 0.5, 0.75, 1.0],
        sub("e", "hack") + ", the false-pass rate of the hack",
        "share of policy, or rate",
        xfmt=lambda v: f"{v:.1f}",
        yfmt=lambda v: f"{v:.2f}",
    )
    xs = [r["e_hack"] for r in b]
    body.append(ax.path(xs, [r["predicted_hack_share"] for r in b], "l4", width=1.5, dash="4 4"))
    body.append(ax.path(xs, [r["hack"] for r in b], "l2"))
    body.append(ax.dots(xs, [r["hack"] for r in b], "f2"))
    body.append(ax.path(xs, [r["true"] for r in b], "l1"))
    body.append(ax.dots(xs, [r["true"] for r in b], "f1"))
    body.append(ax.path(xs, [r["verifier"] for r in b], "l3"))
    body.append(ax.dots(xs, [r["verifier"] for r in b], "f3"))
    body += legend(
        [
            ("l3", "verifier pass rate", None),
            ("l1", "true success rate", None),
            ("l2", "probability of the hack", None),
            ("l4", "predicted hack share, P(" + sub("p", "x") + " &lt; " + sub("e", "hack") + ")", "4 4"),
        ],
        96,
        34,
    )
    return svg(body, "Where the policy ends up as the hack's false-pass rate rises")


def fig_b_curve(d):
    c = d["exp_b_curve"]
    ax = Axes(0, d["exp_b"]["steps"], 0, 0.8)
    body = ax.frame([0, 1000, 2000, 3000, 4000], [0, 0.2, 0.4, 0.6, 0.8], "policy updates", "rate", yfmt=lambda v: f"{v:.1f}")
    body.append(ax.path(c["step"], c["verifier"], "l3"))
    body.append(ax.path(c["step"], c["true"], "l1"))
    body.append(ax.path(c["step"], c["hack"], "l2"))
    body += legend(
        [("l3", "verifier pass rate (what training sees)", None), ("l1", "true success rate", None), ("l2", "probability of the hack", None)],
        300,
        176,
    )
    return svg(body, "Verifier pass rate and true success rate during training with a hack false-pass rate of 0.6")


# ── the environment, built up one part per figure ──────────────────────────


def _box(x, y, w, h, lines, new):
    stroke = ' style="stroke: var(--s1); stroke-width: 2"' if new else ""
    out = [f'<rect class="box" x="{x}" y="{y}" width="{w}" height="{h}" rx="3"{stroke}/>']
    n = len(lines)
    for i, t in enumerate(lines):
        yy = y + h / 2 + (i - (n - 1) / 2) * 16 + 4
        cls_t = "" if i == 0 else ' class="muted"'
        out.append(f'<text{cls_t} x="{x + w / 2}" y="{yy:.1f}" text-anchor="middle">{t}</text>')
    return out


def _arrow(x1, y1, x2, y2, new, label=None, lx=None, ly=None, anchor="middle", dash=None, k=0):
    cls = "l1" if new else "ink"
    mk = f"arrN{k}" if new else f"arrO{k}"
    d = f' stroke-dasharray="{dash}"' if dash else ""
    out = [f'<line class="{cls}" x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke-width="{2 if new else 1.2}" marker-end="url(#{mk})"{d}/>']
    if label:
        out.append(f'<text class="muted" x="{lx}" y="{ly}" text-anchor="{anchor}">{label}</text>')
    return out


def env_stage(k):
    """Stage k of the build-up: 1 task and sandbox, 2 tool parser, 3 transition,
    4 render, 5 budget, 6 verifier. The part added at stage k is drawn in the accent colour."""
    b = []
    b.append(
        f'<defs><marker id="arrO{k}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">'
        '<path d="M0 0L10 5L0 10z" style="fill: var(--fg)"/></marker>'
        f'<marker id="arrN{k}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">'
        '<path d="M0 0L10 5L0 10z" style="fill: var(--s1)"/></marker></defs>'
    )
    n = lambda j: k == j
    # 1. task set, prompt, sandbox
    b += _box(20, 20, 150, 46, ["Task set 𝒟", "draw x"], n(1))
    b += _box(20, 150, 150, 60, ["Policy " + sub("π", "θ"), "reads transcript " + sub("h", "t")], n(1))
    sand = ["Sandbox", sub("s", "0") + "(x): files, tests"] if k < 3 else ["Sandbox", sub("s", "t") + " → " + sub("s", "t+1")]
    b += _box(450, 140, 170, 80, sand, n(1) or n(3))
    b += _arrow(95, 66, 95, 148, n(1), "prompt x", 102, 112, "start", k=k)
    b += _arrow(170, 40, 535, 40, n(1), k=k)
    b += _arrow(535, 40, 535, 138, n(1), "build " + sub("s", "0") + "(x)", 542, 92, "start", k=k)
    if k >= 2:
        b += _box(255, 132, 120, 40, ["Tool parser"], n(2))
        b += _arrow(170, 165, 253, 152, n(2), "text " + sub("a", "t"), 210, 146, k=k)
        b += _arrow(375, 152, 448, 165, n(2), "tool call", 410, 146, k=k)
    if k >= 3:
        b += _arrow(535, 220, 535, 262, n(3), "output", 542, 246, "start", k=k)
        b.append(f'<text class="muted" x="560" y="278" text-anchor="middle">stdout, errors</text>' if k == 3 else "")
    if k >= 4:
        b += _box(255, 250, 120, 40, ["Render"], n(4))
        b += _arrow(533, 270, 377, 270, n(4), k=k)
        b += _arrow(255, 270, 95, 270, n(4), k=k)
        b += _arrow(95, 270, 95, 212, n(4), "append to " + sub("h", "t+1"), 102, 245, "start", k=k)
    if k >= 5:
        b += _box(240, 60, 150, 46, ["Budget", "turns, tokens, time"], n(5))
        b += _arrow(315, 106, 315, 130, n(5), k=k)
        b.append(f'<text class="muted" x="400" y="80">stop at a cap</text>')
    if k >= 6:
        b += _box(450, 300, 170, 50, ["Verifier", "v = grade(x, " + sub("s", "T") + ")"], n(6))
        b += _arrow(605, 222, 605, 298, n(6), "final " + sub("s", "T"), 599, 290, "end", k=k)
        b += _arrow(450, 325, 172, 325, n(6), "reward " + sub("r", "T−1") + " = v", 311, 318, k=k)
        b.append('<text class="muted" x="20" y="329">to the trainer</text>')
    h = 300 if k < 6 else 360
    return f'<svg viewBox="0 0 640 {h}" role="img" aria-label="Environment for a language-model agent, stage {k} of 6">' + "".join(b) + "</svg>"


def fig_affine(_d):
    ax = Axes(0, 1, 0, 1, left=90, right=430, top=20, bottom=240)
    body = ax.frame([0, 1], [0, 0.25, 0.5, 0.75, 1.0], "true outcome y (0 = fail, 1 = success)", "E[v | y], probability of a pass", xfmt=lambda v: str(int(v)), yfmt=lambda v: f"{v:.2f}")
    ep, ef = 0.2, 0.1
    body.append(ax.path([0, 1], [ep, 1 - ef], "l1", width=2.5))
    body.append(ax.dots([0, 1], [ep, 1 - ef], "f1", r=4))
    body.append(ax.path([0, 1], [0, 1], "l4", width=1.2, dash="4 4"))
    body.append(f'<text class="muted" x="{ax.x(0) + 8:.1f}" y="{ax.y(ep) + 18:.1f}">{sub("e", "pass")} = 0.2</text>')
    body.append(f'<text class="muted" x="{ax.x(1) + 10:.1f}" y="{ax.y(1 - ef) + 4:.1f}">1 − {sub("e", "fail")} = 0.9</text>')
    body.append(f'<text x="{ax.x(0.5) + 6:.1f}" y="{ax.y(0.55) - 6:.1f}">slope κ = 0.7</text>')
    body.append(f'<text class="muted" x="{ax.x(0.62):.1f}" y="{ax.y(0.5):.1f}">perfect verifier</text>')
    # a hack that passes more often than a real attempt
    body.append(f'<circle class="f2" cx="{ax.x(0):.1f}" cy="{ax.y(0.6):.1f}" r="4"/>')
    body.append(f'<text x="{ax.x(0) + 8:.1f}" y="{ax.y(0.6) + 4:.1f}">a hack: y = 0, passes 60%</text>')
    body.append('<text x="450" y="160">Class-conditional errors put</text>')
    body.append('<text x="450" y="176">every trajectory on the teal line.</text>')
    body.append('<text x="450" y="204">A hack sits off the line,</text>')
    body.append('<text x="450" y="220">above it, at y = 0.</text>')
    return svg(body, "Expected verifier pass against true outcome: an affine map with slope kappa")


FIGURES = {"a-curves": fig_a_curves, "a-steps": fig_a_steps, "b-sweep": fig_b_sweep, "b-curve": fig_b_curve, "affine": fig_affine}
FIGURES.update({f"env-{k}": (lambda d, k=k: env_stage(k)) for k in range(1, 7)})


def render():
    d = json.loads(DATA.read_text())
    html = CHAPTER.read_text()
    for name, fn in FIGURES.items():
        pat = re.compile(rf"(<!-- figure:{name} -->).*?(<!-- /figure:{name} -->)", re.S)
        if not pat.search(html):
            print(f"no marker for {name}", file=sys.stderr)
            continue
        html = pat.sub(lambda m: m.group(1) + fn(d) + m.group(2), html)
    CHAPTER.write_text(html)
    print(f"rendered {', '.join(FIGURES)} into {CHAPTER.name}")


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "run":
        run()
    elif cmd == "render":
        render()
    else:
        print(__doc__)
        sys.exit(2)
