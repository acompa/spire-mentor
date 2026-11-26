"""Data and plots for chapter 6.

    tour/run stop-6-off-policy/figures.py play --out RUNS.npz [--runs 48] [--workers 3]
    tour/run stop-6-off-policy/figures.py data --runs-file RUNS.npz
    tour/run stop-6-off-policy/figures.py svg

`play` has network A play sampled runs while network B scores every state,
then has B play the same seeds (check.py's loop), and saves every decision's
log-probabilities. `data` turns that file into `figures_data.json`, the small
file the chapter's numbers and plots come from. `svg` redraws the chapter's
data figures from the JSON, in place, between `<!-- fig:NAME -->` markers in
`chapter.html`.
"""

from __future__ import annotations

import argparse
import json
import math
import time
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

import numpy as np

import check
import tourlib

STOP = Path(__file__).resolve().parent
DATA = STOP / "figures_data.json"
CHAPTER = STOP / "chapter.html"

EPS_GRID = (0.003, 0.01, 0.02, 0.03, 0.05, 0.1, 0.2, 0.3, 0.5, 1.0)
HORIZONS = (1, 2, 5, 10, 20, 50, 100, 200)
CAPS = (1.0, 1.25, 1.5, 2.0, 3.0, 5.0, 10.0, 30.0, 100.0, math.inf)


# --- play --------------------------------------------------------------------


def cmd_play(out: Path, n: int, workers: int) -> None:
    seeds = list(tourlib.PANEL_A[:n])
    paths = {"A": str(tourlib.weight(check.NET_A)), "B": str(tourlib.weight(check.NET_B))}
    t0 = time.time()
    with ProcessPoolExecutor(workers, initializer=check._init, initargs=(paths,)) as pool:
        runs_a = list(pool.map(check._play_a, seeds))
        t1 = time.time()
        runs_b = list(pool.map(check._play_b, seeds))
    t2 = time.time()
    lens = np.array([len(r.chosen) for r in runs_a])
    ncls = np.array([len(x) for r in runs_a for x in r.logp_a])
    width = int(ncls.max())
    la = np.full((lens.sum(), width), -np.inf, np.float32)
    lb = np.full_like(la, -np.inf)
    i = 0
    for r in runs_a:
        for a, b in zip(r.logp_a, r.logp_b):
            la[i, : len(a)], lb[i, : len(b)] = a, b
            i += 1
    np.savez_compressed(
        out,
        seeds=np.array(seeds), lens=lens, n_classes=ncls,
        chosen=np.concatenate([r.chosen for r in runs_a]).astype(np.int16),
        logp_a=la, logp_b=lb,
        won_a=np.array([r.won for r in runs_a]), won_b=np.array([r.won for r in runs_b]),
        seconds=np.array([t1 - t0, t2 - t1]), workers=np.array(workers),
    )
    print(f"A: {t1 - t0:.0f} s, B: {t2 - t1:.0f} s, {lens.sum()} decisions -> {out}")


# --- data --------------------------------------------------------------------


def _target(la: np.ndarray, lb: np.ndarray, eps: float) -> np.ndarray:
    """Row-wise log π_eps over each row's classes (-inf past the class count)."""
    if eps >= 1.0:
        return lb.astype(np.float64)
    x = (1.0 - eps) * la.astype(np.float64) + eps * lb.astype(np.float64)
    x = np.where(np.isfinite(la), x, -np.inf)
    m = x.max(1, keepdims=True)
    return x - (m + np.log(np.exp(x - m).sum(1, keepdims=True)))


def _split(x: np.ndarray, lens: np.ndarray) -> list[np.ndarray]:
    return np.split(x, np.cumsum(lens)[:-1])


def _hist(log10_w: np.ndarray, lo: float, hi: float, n: int) -> dict:
    edges = np.linspace(lo, hi, n + 1)
    c, _ = np.histogram(np.clip(log10_w, lo, hi - 1e-9), edges)
    return {"edges": edges.round(4).tolist(), "share": (c / len(log10_w)).round(6).tolist()}


def cmd_data(runs_file: Path) -> None:
    z = np.load(runs_file)
    lens, chosen = z["lens"], z["chosen"].astype(np.int64)
    la = z["logp_a"].astype(np.float64)
    lb = z["logp_b"].astype(np.float64)
    won_a, won_b = z["won_a"], z["won_b"]
    pa = np.exp(la)  # 0 past the class count
    rows = np.arange(len(chosen))
    out: dict = {
        "runs": int(len(lens)),
        "decisions": int(lens.sum()),
        "decisions_per_run": float(lens.mean()),
        "classes_mean": float(z["n_classes"].mean()),
        "play_seconds": z["seconds"].round(1).tolist(),
        "play_workers": int(z["workers"]),
        "won_a": int(won_a.sum()),
        "won_b": int(won_b.sum()),
        "top_agreement": float(np.mean(la.argmax(1) == lb.argmax(1))),
        "a_samples_its_argmax": float(np.mean(chosen == la.argmax(1))),
    }

    per_eps = {}
    for eps in EPS_GRID:
        lt = _target(la, lb, eps)
        pt = np.exp(lt)
        lr = lt[rows, chosen] - la[rows, chosen]  # log ρ of the action A took
        runs = _split(lr, lens)
        run_lw = np.array([r.sum() for r in runs])
        # Exact per-state quantities, summed over every action (not sampled).
        with np.errstate(divide="ignore", invalid="ignore"):
            kl = np.where(pa > 0, pa * (la - lt), 0.0).sum(1)  # KL(μ ‖ π)
            m2 = np.where(pa > 0, pt**2 / pa, 0.0).sum(1)  # E_μ[ρ²]
        caps = {}
        for c in CAPS:
            kept = (pt if math.isinf(c) else np.minimum(pt, c * pa)).sum(1)  # E_μ[min(ρ, c)]
            with np.errstate(divide="ignore", invalid="ignore"):
                m2c = np.where(pa > 0, np.minimum(pt / np.where(pa > 0, pa, 1), c) ** 2 * pa, 0).sum(1)
            caps["inf" if math.isinf(c) else f"{c:g}"] = {
                "lost_mass": float(1 - kept.mean()),
                "var": float((m2c - kept**2).mean()),
                "clamped_share": float((lr > math.log(c)).mean()) if math.isfinite(c) else 0.0,
            }
        # V-trace's trace: products of min(1, ρ) over the k steps that follow.
        trunc = np.minimum(lr, 0.0)
        trace = {}
        for k in HORIZONS:
            vals = []
            for r in _split(trunc, lens):
                if len(r) > k:
                    cs = np.concatenate([[0.0], np.cumsum(r)])
                    vals.append(np.exp(cs[k:] - cs[:-k]))
            trace[str(k)] = float(np.concatenate(vals).mean())
        w = np.exp(lr)
        per_eps[f"{eps:g}"] = {
            "eps": eps,
            "kl_mean": float(kl.mean()),
            "var_log_rho": float(lr.var()),
            "mean_log_rho": float(lr.mean()),
            "chi2_mean": float((m2 - 1).mean()),
            "clamped_share": float((w > check.CLAMP).mean()),
            "w_quantiles": dict(zip(("p01", "p10", "p50", "p90", "p99"),
                                    np.quantile(w, [0.01, 0.1, 0.5, 0.9, 0.99]).tolist())),
            "w_max": float(w.max()),
            "w_mean": float(w.mean()),
            "ess_by_horizon": {str(k): check.ess_share(check.window_log_weights(runs, k))
                               for k in HORIZONS},
            "ess_run": check.ess_share(run_lw),
            "run_log10_w": (run_lw / math.log(10)).round(3).tolist(),
            "run_log10_min": float(run_lw.min() / math.log(10)),
            "run_log10_max": float(run_lw.max() / math.log(10)),
            "run_log10_w_range": float((run_lw.max() - run_lw.min()) / math.log(10)),
            "top_run_weight_share": float(np.exp(run_lw.max() - check.logsumexp(run_lw))),
            "caps": caps,
            "trace_mean_by_horizon": trace,
        }
        if eps in (0.1, 0.3, 1.0):
            per_eps[f"{eps:g}"]["hist_log10_w"] = _hist(lr / math.log(10), -3.0, 2.0, 50)
    out["per_eps"] = per_eps
    out["won_a_by_run"] = won_a.astype(int).tolist()

    lr1 = np.array(per_eps["1"]["run_log10_w"]) * math.log(10)
    plain, snis = check.is_estimates(lr1, won_a)
    out["ope"] = {
        "a_rate": float(won_a.mean()),
        "truth_b_rate": float(won_b.mean()),
        "plain": plain,
        "plain_log10": math.log10(plain) if plain > 0 else None,
        "snis": snis,
    }
    DATA.write_text(json.dumps(out, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {DATA}")


# --- svg ---------------------------------------------------------------------

W, H = 560, 300
ML, MR, MT, MB = 62, 18, 18, 48


class Axes:
    """Maps data to a W×H viewBox with left and bottom axes."""

    def __init__(self, x0, x1, y0, y1, logx=False, logy=False, w=W, h=H):
        self.x0, self.x1, self.y0, self.y1, self.logx, self.logy = x0, x1, y0, y1, logx, logy
        self.w, self.h = w, h

    def _t(self, v, lo, hi, log):
        if log:
            v, lo, hi = math.log10(v), math.log10(lo), math.log10(hi)
        return (v - lo) / (hi - lo)

    def x(self, v):
        return ML + self._t(v, self.x0, self.x1, self.logx) * (self.w - ML - MR)

    def y(self, v):
        return self.h - MB - self._t(v, self.y0, self.y1, self.logy) * (self.h - MT - MB)

    def frame(self, xticks, yticks, xlabel, ylabel, xfmt=str, yfmt=str) -> list[str]:
        s = []
        for t in yticks:
            y = self.y(t)
            s.append(f'<line class="grid" x1="{ML}" y1="{y:.1f}" x2="{self.w - MR}" y2="{y:.1f}"/>')
            s.append(f'<text class="muted" x="{ML - 6}" y="{y + 4:.1f}" text-anchor="end">{yfmt(t)}</text>')
        for t in xticks:
            x = self.x(t)
            s.append(f'<line class="ink" x1="{x:.1f}" y1="{self.h - MB}" x2="{x:.1f}" y2="{self.h - MB + 4}"/>')
            s.append(f'<text class="muted" x="{x:.1f}" y="{self.h - MB + 18}" text-anchor="middle">{xfmt(t)}</text>')
        s.append(f'<line class="ink" x1="{ML}" y1="{self.h - MB}" x2="{self.w - MR}" y2="{self.h - MB}"/>')
        s.append(f'<line class="ink" x1="{ML}" y1="{MT}" x2="{ML}" y2="{self.h - MB}"/>')
        s.append(f'<text x="{(ML + self.w - MR) / 2:.0f}" y="{self.h - 8}" text-anchor="middle">{xlabel}</text>')
        s.append(f'<text x="14" y="{(MT + self.h - MB) / 2:.0f}" text-anchor="middle" '
                 f'transform="rotate(-90 14 {(MT + self.h - MB) / 2:.0f})">{ylabel}</text>')
        return s

    def line(self, xs, ys, cls, extra="") -> str:
        pts = " ".join(f"{self.x(a):.1f},{self.y(b):.1f}" for a, b in zip(xs, ys))
        return f'<polyline class="{cls}" stroke-width="2" points="{pts}" {extra}/>'


def _svg(body: list[str], label: str, w=W, h=H) -> str:
    return (f'<svg viewBox="0 0 {w} {h}" role="img" aria-label="{label}">\n'
            + "\n".join(body) + "\n</svg>")


def _pct(v):
    return f"{v:g}%"


def fig_hist(d) -> str:
    """Per-decision weights, small and large gap, as a share of decisions per bin."""
    ax = Axes(-3, 2, 0, 0.5)
    b = ax.frame([-3, -2, -1, 0, 1, 2], [0, 0.1, 0.2, 0.3, 0.4, 0.5],
                 "per-decision weight ρ (log scale)", "share of A's decisions",
                 xfmt=lambda t: {-3: "0.001", -2: "0.01", -1: "0.1", 0: "1", 1: "10", 2: "100"}[t],
                 yfmt=lambda t: f"{t:.1f}")
    x2 = ax.x(math.log10(check.CLAMP))
    b.append(f'<line class="l4" stroke-dasharray="4 3" x1="{x2:.1f}" y1="{MT}" x2="{x2:.1f}" y2="{H - MB}"/>')
    b.append(f'<text class="muted" x="{x2 + 4:.1f}" y="{MT + 10}">clamp at 2</text>')
    for key, cls, name, dy in (("0.1", "l1", "ε = 0.1", 0), ("1", "l2", "B itself", 16)):
        h = d["per_eps"][key]["hist_log10_w"]
        e, s = h["edges"], h["share"]
        pts = []
        for i, v in enumerate(s):
            pts += [(e[i], v), (e[i + 1], v)]
        b.append(ax.line([p[0] for p in pts], [min(p[1], 0.5) for p in pts], cls))
        b.append(f'<line class="{cls}" stroke-width="2" x1="{ML + 14}" y1="{MT + 14 + dy}" x2="{ML + 34}" y2="{MT + 14 + dy}"/>'
                 f'<text x="{ML + 40}" y="{MT + 18 + dy}">{name}</text>')
    return _svg(b, "Histogram of per-decision importance weights for the small and large gap.")


def fig_ess(d) -> str:
    ks = [int(k) for k in d["per_eps"]["1"]["ess_by_horizon"]]
    ax = Axes(1, 200, 0, 100, logx=True)
    b = ax.frame([1, 2, 5, 10, 20, 50, 100, 200], [0, 20, 40, 60, 80, 100],
                 "k, number of consecutive ratios multiplied (log scale)", "ESS, % of weights", yfmt=_pct)
    lines = (("0.03", "l4"), ("0.1", "l1"), ("0.3", "l3"), ("1", "l2"))
    for i, (key, cls) in enumerate(lines):
        e = d["per_eps"][key]
        ys = [100 * e["ess_by_horizon"][str(k)] for k in ks]
        b.append(ax.line(ks, ys, cls))
        b += [f'<circle class="{cls.replace("l", "f")}" cx="{ax.x(k):.1f}" cy="{ax.y(y):.1f}" r="3"/>'
              for k, y in zip(ks, ys)]
        if key != "1":
            kk = np.geomspace(1, 200, 40)
            th = 100 * np.exp(-kk * e["var_log_rho"])
            b.append(ax.line(kk, th, cls, 'stroke-dasharray="5 4" stroke-opacity="0.8"'))
        name = "B itself" if key == "1" else f"ε = {key}"
        b.append(f'<line class="{cls}" stroke-width="2" x1="{ML + 14}" y1="{ax.y(68) + 16 * i:.1f}" x2="{ML + 34}" y2="{ax.y(68) + 16 * i:.1f}"/>'
                 f'<text x="{ML + 40}" y="{ax.y(68) + 4 + 16 * i:.1f}">{name}</text>')
    return _svg(b, "Effective sample size against the number of ratios multiplied, for four gaps.")


def fig_clamp(d) -> str:
    """Exact lost mass and weight variance against the clamp level, large gap."""
    caps = [c for c in CAPS if math.isfinite(c)]
    ax = Axes(1, 100, 0.01, 10, logx=True, logy=True)
    b = ax.frame([1, 2, 5, 10, 30, 100], [0.01, 0.1, 1, 10],
                 "clamp level c (log scale)", "per state, mean over A's states (log scale)",
                 yfmt=lambda t: f"{t:g}")
    e = d["per_eps"]["1"]["caps"]
    lost = [e[f"{c:g}"]["lost_mass"] for c in caps]
    var = [e[f"{c:g}"]["var"] for c in caps]
    b.append(ax.line(caps, lost, "l2"))
    b.append(ax.line(caps, var, "l1"))
    x2 = ax.x(2.0)
    b.append(f'<line class="l4" stroke-dasharray="4 3" x1="{x2:.1f}" y1="{MT}" x2="{x2:.1f}" y2="{H - MB}"/>')
    b.append(f'<text class="muted" x="{x2 + 4:.1f}" y="{MT + 10}">trainer: c = 2</text>')
    b.append(f'<line class="l2" stroke-width="2" x1="{ML + 14}" y1="{MT + 30}" x2="{ML + 34}" y2="{MT + 30}"/>'
             f'<text x="{ML + 40}" y="{MT + 34}">probability mass lost (bias)</text>'
             f'<line class="l1" stroke-width="2" x1="{ML + 14}" y1="{MT + 46}" x2="{ML + 34}" y2="{MT + 46}"/>'
             f'<text x="{ML + 40}" y="{MT + 50}">variance of min(ρ, c)</text>')
    return _svg(b, "Bias and variance of the clamped weight against the clamp level, for target B.")


def fig_trace(d) -> str:
    ks = [int(k) for k in d["per_eps"]["1"]["trace_mean_by_horizon"]]
    ax = Axes(1, 200, 1e-4, 1, logx=True, logy=True)
    b = ax.frame([1, 2, 5, 10, 20, 50, 100, 200], [1e-4, 1e-3, 1e-2, 1e-1, 1],
                 "k, steps after the TD error (log scale)", "mean of ∏ min(1, ρ) (log scale)",
                 yfmt=lambda t: f"{t:g}")
    for i, (key, cls) in enumerate((("0.1", "l1"), ("0.3", "l3"), ("1", "l2"))):
        ys = [max(d["per_eps"][key]["trace_mean_by_horizon"][str(k)], 1e-4) for k in ks]
        b.append(ax.line(ks, ys, cls))
        name = "B itself" if key == "1" else f"ε = {key}"
        b.append(f'<line class="{cls}" stroke-width="2" x1="{ML + 20}" y1="{H - MB - 60 + 16 * i}" x2="{ML + 40}" y2="{H - MB - 60 + 16 * i}"/>'
                 f'<text x="{ML + 46}" y="{H - MB - 56 + 16 * i}">{name}</text>')
    return _svg(b, "Mean product of truncated ratios against the number of steps, for three gaps.")


def fig_runs(d) -> str:
    """Per-run log10 weights for target B, won against lost."""
    lw = d["per_eps"]["1"]["run_log10_w"]
    won = d["won_a_by_run"]
    lo = 10 * math.floor(min(lw) / 10)
    ax = Axes(lo, 0, 0, 1, h=200)
    hh = 200
    b = []
    for t in range(lo, 1, 25 if -lo > 120 else 10):
        x = ax.x(t)
        b.append(f'<line class="ink" x1="{x:.1f}" y1="{hh - MB}" x2="{x:.1f}" y2="{hh - MB + 4}"/>'
                 f'<text class="muted" x="{x:.1f}" y="{hh - MB + 18}" text-anchor="middle">{t}</text>')
    b.append(f'<line class="ink" x1="{ML}" y1="{hh - MB}" x2="{W - MR}" y2="{hh - MB}"/>')
    b.append(f'<text x="{(ML + W - MR) / 2:.0f}" y="{hh - 8}" text-anchor="middle">log10 of the per-run weight ∏ρ for target B</text>')
    for row, (flag, cls, name) in enumerate(((1, "f1", "A won"), (0, "f2", "A lost"))):
        y = 40 + 45 * row
        b.append(f'<text x="{ML - 6}" y="{y + 4}" text-anchor="end">{name}</text>')
        for v, wv in zip(lw, won):
            if wv == flag:
                b.append(f'<circle class="{cls}" cx="{ax.x(v):.1f}" cy="{y}" r="4" fill-opacity="0.7"/>')
    return _svg(b, "Dot plot of per-run log weights for target B, split by whether A won.", h=hh)


def fig_learner(d) -> str:
    """The real learner's weight tails per update (stop 5's run on this Mac)."""
    st = json.loads((STOP / "learner_stats.json").read_text(encoding="utf-8"))
    rows = [r for r in st["rows"] if r["it"] > 1]
    its = [r["it"] for r in rows]
    ax = Axes(min(its), max(its), 0.8, 2.1)
    b = ax.frame(list(range(5, max(its) + 1, 5)), [0.8, 1.0, 1.2, 1.4, 1.6, 1.8, 2.0],
                 "update (iteration of 8,192 decisions)", "importance weight π_prox/μ",
                 yfmt=lambda t: f"{t:.1f}")
    y2 = ax.y(2.0)
    b.append(f'<line class="l2" stroke-dasharray="4 3" x1="{ML}" y1="{y2:.1f}" x2="{W - MR}" y2="{y2:.1f}"/>'
             f'<text class="muted" x="{W - MR - 4}" y="{y2 - 6:.1f}" text-anchor="end">clamp at 2</text>')
    top = [(i, r["is_tails"]["p99"]) for i, r in zip(its, rows)]
    bot = [(i, r["is_tails"]["p01"]) for i, r in zip(its, rows)][::-1]
    pts = " ".join(f"{ax.x(i):.1f},{ax.y(v):.1f}" for i, v in top + bot)
    b.append(f'<polygon class="f1" fill-opacity="0.25" points="{pts}"/>')
    b.append(ax.line(its, [r["is_tails"]["max"] for r in rows], "l3"))
    b.append(f'<rect class="f1" fill-opacity="0.25" x="{W - 200}" y="{MT + 40}" width="20" height="10"/>'
             f'<text x="{W - 174}" y="{MT + 49}">1st to 99th percentile</text>'
             f'<line class="l3" stroke-width="2" x1="{W - 200}" y1="{MT + 64}" x2="{W - 180}" y2="{MT + 64}"/>'
             f'<text x="{W - 174}" y="{MT + 68}">largest weight</text>')
    return _svg(b, "Band of importance weights per update in the real learner, far below the clamp.")


def learner_summary() -> dict:
    st = json.loads((STOP / "learner_stats.json").read_text(encoding="utf-8"))
    u = [r for r in st["rows"] if r["it"] > 1]
    med = lambda xs: float(np.median(xs))
    return {
        "updates": len(u),
        "stale_mean": float(np.mean([r["staleness"]["mean"] for r in u])),
        "stale_max": max(r["staleness"]["max"] for r in u),
        "fresh_max": max(r["staleness"]["fresh_max"] for r in u),
        "p01_median": med([r["is_tails"]["p01"] for r in u]),
        "p99_median": med([r["is_tails"]["p99"] for r in u]),
        "w_max": max(r["is_tails"]["max"] for r in u),
        "clamped_max": max(r["is_tails"]["frac_clamped"] for r in u),
        "clipfrac_median": med([r["clipfrac"] or 0.0 for r in u]),
    }


def _get(d, path: str):
    for part in path.split("/"):
        d = d[part] if isinstance(d, dict) else d[int(part)]
    return d


FORMATS = {
    "pct0": lambda v: f"{100 * v:.0f}%", "pct1": lambda v: f"{100 * v:.1f}%",
    "pct2": lambda v: f"{100 * v:.2f}%", "f2": lambda v: f"{v:.2f}", "f3": lambda v: f"{v:.3f}",
    "f4": lambda v: f"{v:.4f}", "int": lambda v: f"{round(v):,}", "g2": lambda v: f"{v:.2g}", "f0": lambda v: f"{v:,.0f}",
    "log1": lambda v: f"{v:.1f}".replace("-", "−"), "e1": lambda v: f"{v:.1e}".replace("e-", "e−"),
    "sci": lambda v: (lambda m, e: f"{m} × 10<sup>{e.replace('-', '−')}</sup>")(*f"{v:.1e}".replace("e+", "e").split("e")),
}


def fill_values(html: str, d: dict) -> str:
    """Rewrite every `<span data-k="path|fmt">…</span>` from the JSON."""
    import re

    def sub(m):
        path, fmt = m.group(1).split("|")
        return f'<span data-k="{m.group(1)}">{FORMATS[fmt](_get(d, path))}</span>'

    return re.sub(r'<span data-k="([^"]+)">.*?</span>', sub, html)


FIGS = {"learner": fig_learner, "hist": fig_hist, "ess": fig_ess, "clamp": fig_clamp, "trace": fig_trace, "runs": fig_runs}


def cmd_svg() -> None:
    d = json.loads(DATA.read_text(encoding="utf-8"))
    html = CHAPTER.read_text(encoding="utf-8")
    for name, fn in FIGS.items():
        start, end = f"<!-- fig:{name} -->", f"<!-- /fig:{name} -->"
        i, j = html.find(start), html.find(end)
        if i < 0 or j < 0:
            print(f"no markers for {name}; skipped")
            continue
        html = html[: i + len(start)] + "\n" + fn(d) + "\n" + html[j:]
    d["learner"] = learner_summary()
    html = fill_values(html, d)
    # The host may serve the page without a charset: keep everything ASCII.
    html = "".join(c if ord(c) < 128 else f"&#{ord(c)};" for c in html)
    CHAPTER.write_text(html, encoding="utf-8")
    print(f"redrew {', '.join(FIGS)} in {CHAPTER.name}")


def main() -> None:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("play")
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--runs", type=int, default=48)
    p.add_argument("--workers", type=int, default=3)
    q = sub.add_parser("data")
    q.add_argument("--runs-file", type=Path, required=True)
    sub.add_parser("svg")
    a = ap.parse_args()
    if a.cmd == "play":
        cmd_play(a.out, a.runs, a.workers)
    elif a.cmd == "data":
        cmd_data(a.runs_file)
    else:
        cmd_svg()


if __name__ == "__main__":
    main()
