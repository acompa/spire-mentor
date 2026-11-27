"""Chapter 10's toy experiment and its plots.

A toy problem, not a language model. Eight prompts, each a softmax over eight
whole completions, some marked correct (reward 1) and the rest wrong
(reward 0). The policy's parameters are the 64 logits. For each group size n
the script draws many batches of n completions per prompt, forms four
gradient estimators of J = mean success rate, and measures their mean and
variance against the exact gradient.

    python3 figures.py run      # simulate, write figures.json (a few seconds)
    python3 figures.py render   # draw the plots into chapter.html
"""

import json
import math
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
DATA = HERE / "figures.json"
CHAPTER = HERE / "chapter.html"

P_TARGET = [0.03, 0.10, 0.25, 0.40, 0.60, 0.75, 0.90, 0.97]  # success rate of each prompt
K = 8  # completions per prompt
N_CORRECT = 3  # completions 0..2 are correct
GROUP_SIZES = [2, 4, 8, 16, 32, 64]
TRIALS = 20000
ESTIMATORS = ["none", "loo", "mean", "grpo"]


def build_policy(rng):
    """Logits whose softmax puts mass p on the correct completions."""
    import numpy as np

    pis = []
    for p in P_TARGET:
        w = rng.uniform(0.5, 1.5, K)
        pi = np.empty(K)
        pi[:N_CORRECT] = p * w[:N_CORRECT] / w[:N_CORRECT].sum()
        pi[N_CORRECT:] = (1 - p) * w[N_CORRECT:] / w[N_CORRECT:].sum()
        pis.append(pi)
    pi = np.array(pis)
    correct = np.zeros(K)
    correct[:N_CORRECT] = 1.0
    return pi, correct


def advantages(R, kind):
    """R has shape (trials, n). Returns the per-sample weight A_i."""
    import numpy as np

    n = R.shape[1]
    if kind == "none":
        return R
    s = R.sum(1, keepdims=True)
    if kind == "loo":
        return R - (s - R) / (n - 1)
    mean = s / n
    if kind == "mean":
        return R - mean
    std = R.std(1, keepdims=True)  # population standard deviation
    return (R - mean) / (std + 1e-6)


def run() -> None:
    import numpy as np

    rng = np.random.default_rng(0)
    pi, correct = build_policy(rng)
    M = len(P_TARGET)
    p = pi @ correct
    true_g = (pi * (correct[None, :] - p[:, None])) / M  # dJ/dlogits, shape (M, K)
    tg = true_g.ravel()
    out = {"p": p.tolist(), "true_grad_norm": float(np.linalg.norm(tg)), "rows": []}
    for n in GROUP_SIZES:
        allsame = 0.0
        g = {k: np.zeros((TRIALS, M, K)) for k in ESTIMATORS}
        for x in range(M):
            a = rng.choice(K, size=(TRIALS, n), p=pi[x])
            R = correct[a]
            allsame += float(np.mean((R.sum(1) == 0) | (R.sum(1) == n)))
            onehot = np.zeros((TRIALS, n, K))
            np.put_along_axis(onehot, a[..., None], 1.0, axis=2)
            score = onehot - pi[x][None, None, :]  # d log pi(a) / d logits
            for k in ESTIMATORS:
                A = advantages(R, k)
                g[k][:, x, :] = (A[..., None] * score).mean(1) / M
        row = {"n": n, "allsame": allsame / M}
        for k in ESTIMATORS:
            G = g[k].reshape(TRIALS, -1)
            mean = G.mean(0)
            var = G.var(0).sum()
            cos = float(mean @ tg / (np.linalg.norm(mean) * np.linalg.norm(tg)))
            row[k] = {
                "scale": float(mean @ tg / (tg @ tg)),  # 1 means unbiased in scale
                "cos": cos,
                "rel_bias": float(np.linalg.norm(mean - tg) / np.linalg.norm(tg)),
                "trvar": float(var),
                "inv_snr": float(var / (mean @ mean)),  # noise / signal, scale-free
            }
        out["rows"].append(row)
        print(n, {k: (round(row[k]["inv_snr"], 3), round(row[k]["cos"], 4), round(row[k]["scale"], 3)) for k in ESTIMATORS}, "allsame", round(row["allsame"], 3))
    # analytic share of single-outcome groups, mean over the eight prompts
    out["allsame_exact"] = [
        float(np.mean([pp**n + (1 - pp) ** n for pp in p])) for n in GROUP_SIZES
    ]
    DATA.write_text(json.dumps(out, indent=1) + "\n")


# ── drawing ─────────────────────────────────────────────────────────────────


def f(x: float) -> str:
    return f"{x:.1f}"


class Axes:
    def __init__(self, x0, y0, w, h, xlim, ylim, xlog=False, ylog=False):
        self.x0, self.y0, self.w, self.h = x0, y0, w, h
        self.xlim, self.ylim, self.xlog, self.ylog = xlim, ylim, xlog, ylog

    def X(self, x):
        a, b = self.xlim
        if self.xlog:
            x, a, b = math.log2(x), math.log2(a), math.log2(b)
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
        d = " ".join(f"{'M' if i == 0 else 'L'}{f(self.X(x))} {f(self.Y(y))}" for i, (x, y) in enumerate(pts))
        da = f' stroke-dasharray="{dash}"' if dash else ""
        out = [f'<path class="l{cls}" d="{d}" stroke-width="{width}"{da}/>']
        if dots:
            out += [f'<circle class="f{cls}" cx="{f(self.X(x))}" cy="{f(self.Y(y))}" r="3"/>' for x, y in pts]
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
    return f'<svg viewBox="0 0 {w} {h}" role="img" aria-label="{label}">' + "".join(body) + "</svg>"


LABEL = {"none": "no baseline", "loo": "RLOO", "mean": "group mean", "grpo": "GRPO"}
CLS = {"none": 4, "loo": 1, "mean": 3, "grpo": 2}
DASH = {"none": None, "loo": None, "mean": "5 4", "grpo": None}


def fig_weight() -> str:
    """Per-prompt weight 1/sigma that std normalisation applies, against p."""
    ax = Axes(70, 16, 520, 210, (0, 1), (0, 6))
    body = ax.frame([0, 0.2, 0.4, 0.6, 0.8, 1.0], [0, 1, 2, 3, 4, 5, 6],
                    "success rate of the prompt, p", "weight on the prompt", xfmt=lambda t: f"{t:g}")
    pts = [(q / 400, 1 / math.sqrt((q / 400) * (1 - q / 400))) for q in range(11, 390)]
    body += ax.line(pts, 2, width=2.5)
    body += ax.line([(0, 1), (1, 1)], 1, width=2, dash="5 4")
    body.append(f'<circle class="f2" cx="{f(ax.X(0.5))}" cy="{f(ax.Y(2))}" r="4"/>')
    body.append(f'<text class="muted" x="{f(ax.X(0.5))}" y="{f(ax.Y(2) - 9)}" text-anchor="middle">2 at p = 0.5</text>')
    body.append(f'<circle class="f2" cx="{f(ax.X(0.05))}" cy="{f(ax.Y(1 / math.sqrt(0.0475)))}" r="4"/>')
    body.append(f'<text class="muted" x="{f(ax.X(0.07))}" y="{f(ax.Y(1 / math.sqrt(0.0475)) + 4)}">4.6 at p = 0.05</text>')
    body += legend(380, 40, [(2, "divide by σ (GRPO)", None), (1, "no division (RLOO)", "5 4")])
    return svg(640, 270, "Weight that dividing by the group standard deviation gives a prompt, against its success rate", body)


def fig_snr(data) -> str:
    rows = data["rows"]
    ax = Axes(80, 16, 500, 220, (2, 64), (0.1, 20), xlog=True, ylog=True)
    body = ax.frame(GROUP_SIZES, [0.1, 1, 10], "group size n (completions per prompt)",
                    "variance / squared mean (unitless)", yfmt=lambda t: f"{t:g}")
    for k in ESTIMATORS:
        body += ax.line([(r["n"], r[k]["inv_snr"]) for r in rows], CLS[k], dash=DASH[k], dots=True)
    body += legend(420, 30, [(CLS[k], LABEL[k], DASH[k]) for k in ESTIMATORS])
    return svg(640, 280, "Variance over squared mean of four gradient estimators against group size, toy problem", body)


def fig_cos(data) -> str:
    rows = data["rows"]
    ax = Axes(80, 16, 500, 200, (2, 64), (0.97, 1.0), xlog=True)
    body = ax.frame(GROUP_SIZES, [0.97, 0.98, 0.99, 1.0], "group size n (completions per prompt)",
                    "cosine with ∇J (unitless)", yfmt=lambda t: f"{t:.2f}")
    for k in ["loo", "grpo"]:
        body += ax.line([(r["n"], r[k]["cos"]) for r in rows], CLS[k], dots=True)
    body += legend(100, 150, [(CLS["loo"], "RLOO (the other two overlap it)", None), (CLS["grpo"], "GRPO", None)])
    return svg(640, 260, "Cosine between the mean of each estimator and the exact gradient, toy problem", body)


def render() -> None:
    data = json.loads(DATA.read_text())
    figs = {"weight": fig_weight(), "snr": fig_snr(data), "cos": fig_cos(data)}
    html = CHAPTER.read_text()
    for name, s in figs.items():
        pat = re.compile(rf"(<!-- figure:{re.escape(name)} -->).*?(<!-- /figure:{re.escape(name)} -->)", re.S)
        if not pat.search(html):
            raise SystemExit(f"chapter.html has no block for figure {name!r}")
        html = pat.sub(lambda m: m.group(1) + s + m.group(2), html)
    CHAPTER.write_text(html)


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "run":
        run()
    elif cmd == "render":
        render()
    else:
        raise SystemExit(__doc__)
