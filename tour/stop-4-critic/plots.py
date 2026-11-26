"""Inline SVG plots for Chapter 4, drawn from `chapter_data.json`.

Each plot is written between `<!-- FIG:name -->` and `<!-- /FIG:name -->` in
`chapter.html`, so the page's figures always match the committed data. Every
colour is a theme token class from the chapter template (`l1`, `f2`, `grid`, ...).

    python3 tour/stop-4-critic/plots.py        # no simulator needed
"""

from __future__ import annotations

import json
import re
from pathlib import Path

STOP = Path(__file__).resolve().parent
DATA = STOP / "chapter_data.json"
PAGE = STOP / "chapter.html"


def fmt(v: float) -> str:
    return f"{v:.1f}".rstrip("0").rstrip(".") if abs(v) >= 10 else f"{v:.6g}"


class Plot:
    """One set of axes inside an SVG of width `w` (user units)."""

    def __init__(self, xlim, ylim, *, w=640, h=260, top=0, left=62, right=18, pad_t=14,
                 pad_b=46, xlabel="", ylabel="") -> None:  # fmt: skip
        self.x0, self.x1 = xlim
        self.y0, self.y1 = ylim
        self.L, self.R = left, w - right
        self.T, self.B = top + pad_t, top + h - pad_b
        self.xlabel, self.ylabel = xlabel, ylabel
        self.parts: list[str] = []

    def x(self, v: float) -> float:
        return round(self.L + (v - self.x0) / (self.x1 - self.x0) * (self.R - self.L), 1)

    def y(self, v: float) -> float:
        return round(self.B - (v - self.y0) / (self.y1 - self.y0) * (self.B - self.T), 1)

    def axes(self, xticks, yticks, xfmt=fmt, yfmt=fmt) -> Plot:
        p = self.parts
        for v in yticks:
            p.append(f'<line class="grid" x1="{self.L}" x2="{self.R}" y1="{self.y(v)}" y2="{self.y(v)}"/>')
            p.append(f'<text class="muted" x="{self.L - 6}" y="{self.y(v) + 4}" text-anchor="end">{yfmt(v)}</text>')
        for v in xticks:
            p.append(f'<line class="ink" x1="{self.x(v)}" x2="{self.x(v)}" y1="{self.B}" y2="{self.B + 4}"/>')
            p.append(f'<text class="muted" x="{self.x(v)}" y="{self.B + 17}" text-anchor="middle">{xfmt(v)}</text>')
        p.append(f'<line class="ink" x1="{self.L}" x2="{self.R}" y1="{self.B}" y2="{self.B}"/>')
        p.append(f'<line class="ink" x1="{self.L}" x2="{self.L}" y1="{self.T}" y2="{self.B}"/>')
        if self.xlabel:
            p.append(f'<text x="{(self.L + self.R) / 2}" y="{self.B + 36}" text-anchor="middle">{self.xlabel}</text>')
        if self.ylabel:
            cy = (self.T + self.B) / 2
            p.append(f'<text x="14" y="{cy}" text-anchor="middle" transform="rotate(-90 14 {cy})">{self.ylabel}</text>')
        return self

    def line(self, xs, ys, cls="l1", width=1.8, dash=None) -> None:
        pts = " ".join(f"{self.x(a)},{self.y(b)}" for a, b in zip(xs, ys))
        d = f' stroke-dasharray="{dash}"' if dash else ""
        self.parts.append(f'<polyline class="{cls}" stroke-width="{width}"{d} points="{pts}"/>')

    def dots(self, xs, ys, cls="f1", r=3.5, square=False) -> None:
        for a, b in zip(xs, ys):
            if square:
                self.parts.append(f'<rect class="{cls}" x="{self.x(a) - r}" y="{self.y(b) - r}" width="{2 * r}" height="{2 * r}"/>')
            else:
                self.parts.append(f'<circle class="{cls}" cx="{self.x(a)}" cy="{self.y(b)}" r="{r}"/>')

    def band(self, xa, xb, cls, opacity) -> None:
        self.parts.append(f'<rect class="{cls}" opacity="{opacity}" x="{self.x(xa)}" y="{self.T}" '
                          f'width="{max(0.6, self.x(xb) - self.x(xa))}" height="{self.B - self.T}"/>')

    def text(self, x, y, s, anchor="start", cls="", data=True) -> None:
        px, py = (self.x(x), self.y(y)) if data else (x, y)
        c = f' class="{cls}"' if cls else ""
        self.parts.append(f'<text{c} x="{px}" y="{py}" text-anchor="{anchor}">{s}</text>')

    def legend(self, items, x, y, data=False) -> None:
        """items: (kind, cls, label); kind is 'line', 'dash', 'dot', 'square' or 'band'."""
        px, py = (self.x(x), self.y(y)) if data else (x, y)
        for i, (kind, cls, label) in enumerate(items):
            yy = py + 17 * i
            if kind in ("line", "dash"):
                d = ' stroke-dasharray="5 3"' if kind == "dash" else ""
                self.parts.append(f'<line class="{cls}" stroke-width="2"{d} x1="{px}" x2="{px + 22}" y1="{yy - 4}" y2="{yy - 4}"/>')
            elif kind == "dot":
                self.parts.append(f'<circle class="{cls}" cx="{px + 11}" cy="{yy - 4}" r="3.5"/>')
            elif kind == "square":
                self.parts.append(f'<rect class="{cls}" x="{px + 7}" y="{yy - 8}" width="8" height="8"/>')
            else:
                self.parts.append(f'<rect class="{cls}" opacity=".3" x="{px}" y="{yy - 10}" width="22" height="11"/>')
            self.parts.append(f'<text x="{px + 28}" y="{yy}">{label}</text>')


def svg(w, h, label, plots) -> str:
    body = "\n".join(s for p in plots for s in p.parts)
    return (f'<svg viewBox="0 0 {w} {h}" role="img" aria-label="{label}">\n{body}\n</svg>')


ROOM_BAND = {"monster": ("f4", ".14"), "elite": ("f2", ".32"), "boss": ("f3", ".32")}


def trace_panel(tr, top, title, nopriv=False) -> Plot:
    n = tr["n"]
    ticks = [i for i in range(0, n + 1, 100 if n > 300 else 50)]
    p = Plot((0, n), (0, 1.25), top=top, h=230, xlabel="decision index in the run",
             ylabel="critic value (reward units)").axes(ticks, [0, 0.25, 0.5, 0.75, 1.0, 1.25])
    for f in tr["fights"]:
        cls, op = ROOM_BAND[f["room"]] if f["room"] in ROOM_BAND else ("f4", ".14")
        p.band(f["start"], f["end"] + 1, cls, op)
    xs = list(range(n))
    if nopriv:
        p.line(xs, tr["value_nopriv"], "l4", 1.4, "5 3")
    p.line(xs, tr["value"], "l1", 1.6)
    p.text(p.L + 6, top + 12, title, data=False, cls="muted")
    return p


def fig_traces(d) -> str:
    a = trace_panel(d["trace_won"], 0, f"seed {d['trace_won']['seed']}: won the run")
    b = trace_panel(d["trace_lost"], 250, f"seed {d['trace_lost']['seed']}: lost the run", nopriv=True)
    # The won run sits near 1.1 throughout, so its lower half holds the shared legend.
    a.legend([("line", "l1", "critic value V (training input, with PRIV)"),
              ("dash", "l4", "same critic, PRIV tokens removed (lower panel)"),
              ("band", "f4", "monster fight"), ("band", "f2", "elite fight"),
              ("band", "f3", "boss fight")], a.L + 14, 95)  # fmt: skip
    return svg(640, 480, "The critic's value at every decision of a won run and of a lost run, "
               "with fights shaded by room type.", [a, b])


def fig_calibration(d) -> str:
    rows = d["calibration"]
    p = Plot((0, 1.2), (0, 1.2), w=520, h=340, xlabel="mean critic value V in the band (reward units)",
             ylabel="G (reward units) or share won").axes([0, 0.2, 0.4, 0.6, 0.8, 1.0, 1.2], [0, 0.2, 0.4, 0.6, 0.8, 1.0, 1.2])
    p.line([0, 1.2], [0, 1.2], "l4", 1.2, "4 4")
    mv = [r["mean_v"] for r in rows]
    p.line(mv, [r["mean_g"] for r in rows], "l1", 1.4)
    p.dots(mv, [r["mean_g"] for r in rows], "f1")
    p.dots(mv, [r["won"] for r in rows], "f2", 3.5, square=True)
    for r in rows:
        p.text(r["mean_v"], r["mean_g"], f"{r['n']:,}", "end", "muted")
        p.parts[-1] = p.parts[-1].replace(f'x="{p.x(r["mean_v"])}"', f'x="{p.x(r["mean_v"]) - 7}"').replace(
            f'y="{p.y(r["mean_g"])}"', f'y="{p.y(r["mean_g"]) - 7}"')
    p.legend([("dot", "f1", "return that followed, G"), ("square", "f2", "share of runs won"),
              ("dash", "l4", "perfect calibration")], p.L + 220, p.B - 60)
    return svg(520, 340, "Calibration: the mean return that followed each band of critic values, "
               "against the band's mean value.", [p])


def fig_weights(d) -> str:
    p = Plot((0, 80), (0, 1.0), xlabel="steps ahead, l (decisions)", ylabel="weight on δ at t + l (unitless)").axes(
        [0, 10, 20, 30, 40, 50, 60, 70, 80], [0, 0.25, 0.5, 0.75, 1.0])
    ls = list(range(81))
    for lam, cls, dash in ((0.5, "l4", None), (0.9, "l3", None), (0.95, "l1", None), (0.97, "l2", "5 3")):
        p.line(ls, [lam**k for k in ls], cls, 1.8, dash)
    p.dots([20], [0.95**20], "f1", 4)
    p.text(21.5, 0.95**20 + 0.03, f"0.95²⁰ = {0.95**20:.3f}", "start")
    p.legend([("line", "l4", "λ = 0.5"), ("line", "l3", "λ = 0.9"), ("line", "l1", "λ = 0.95 (fights)"),
              ("dash", "l2", "λ = 0.97 (run rows)")], p.R - 170, p.T + 14)
    return svg(640, 260, "The weight lambda to the power l that GAE gives a TD residual l steps later, "
               "for four values of lambda.", [p])


def fig_lambda(d) -> str:
    rows = d["lam_sweep"]
    top = max(max(r["std_combat"], r["std_run"]) for r in rows)
    ymax = 0.05 * (int(top / 0.05) + 1)
    yt = [round(0.05 * i, 2) for i in range(int(round(ymax / 0.05)) + 1)]
    p = Plot((0, 1), (0, ymax), xlabel="λ (unitless)", ylabel="std of Â (reward units)").axes(
        [0, 0.25, 0.5, 0.75, 0.9, 1.0], yt)
    lam = [r["lam"] for r in rows]
    p.line(lam, [r["std_combat"] for r in rows], "l1")
    p.dots(lam, [r["std_combat"] for r in rows], "f1")
    p.line(lam, [r["std_run"] for r in rows], "l2")
    p.dots(lam, [r["std_run"] for r in rows], "f2", square=True)
    p.legend([("dot", "f1", "combat decisions"), ("square", "f2", "run decisions (map, rewards, shop)")],
             p.L + 14, p.T + 14)
    return svg(640, 260, "Standard deviation of the advantage estimate across decisions as lambda "
               "moves from 0 to 1.", [p])


def fig_fork(d) -> str:
    rows = {(r["room"], r["hp"]): r for r in d["fork"]}
    groups = ["all", "<50%", "50-80%", ">=80%"]
    rooms = [("monster", "f4"), ("elite", "f2"), ("boss", "f3")]
    vals = [r["dv"] for r in d["fork"]]
    lo = min(-0.02, min(vals)) - 0.01
    hi = max(0.02, max(vals)) + 0.01
    step = 0.02
    yt = [round(step * k, 2) for k in range(int(lo / step) - 1, int(hi / step) + 2) if lo <= step * k <= hi]
    p = Plot((0, 4), (lo, hi), h=280, xlabel="HP at the start of the fight (share of max HP)",
             ylabel="mean ΔV_run (reward units)").axes([], yt, yfmt=lambda v: f"{v:+.2f}")
    p.parts.append(f'<line class="ink" x1="{p.L}" x2="{p.R}" y1="{p.y(0)}" y2="{p.y(0)}"/>')
    bw = 0.22
    for gi, g in enumerate(groups):
        p.text(gi + 0.5, lo, "", "middle")
        p.parts[-1] = f'<text class="muted" x="{p.x(gi + 0.5)}" y="{p.B + 17}" text-anchor="middle">{"all fights" if g == "all" else "HP " + g.replace(">=", "≥ ").replace("<", "&lt; ")}</text>'
        for ri, (room, cls) in enumerate(rooms):
            r = rows.get((room, g))
            if not r:
                continue
            x0 = gi + 0.5 + (ri - 1.5) * bw
            ya, yb = sorted((p.y(0), p.y(r["dv"])))
            p.parts.append(f'<rect class="{cls}" x="{p.x(x0)}" y="{ya}" width="{p.x(x0 + bw) - p.x(x0) - 2}" height="{max(0.8, yb - ya)}"/>')
            ny = yb + 12 if r["dv"] < 0 else ya - 4
            p.parts.append(f'<text class="muted" x="{p.x(x0 + bw / 2)}" y="{ny}" text-anchor="middle" font-size="10">{r["n"]}</text>')
    p.legend([("square", "f4", "monster"), ("square", "f2", "elite"), ("square", "f3", "boss")],
             p.R - 110, p.T + 14)
    return svg(640, 280, "Mean change in the run value across a fight, by room type and by HP at "
               "the start of the fight.", [p])


FIGS = {"traces": fig_traces, "calibration": fig_calibration, "weights": fig_weights,
        "lambda": fig_lambda, "fork": fig_fork}


def main() -> None:
    d = json.loads(DATA.read_text())
    html = PAGE.read_text()
    for name, fn in FIGS.items():
        pat = re.compile(rf"(<!-- FIG:{name} -->).*?(<!-- /FIG:{name} -->)", re.S)
        if not pat.search(html):
            raise SystemExit(f"chapter.html has no FIG:{name} markers")
        html = pat.sub(lambda m, s=fn(d): f"{m.group(1)}\n{s}\n{m.group(2)}", html)
    PAGE.write_text(html)
    print(f"wrote {len(FIGS)} figures into {PAGE.name}")


if __name__ == "__main__":
    main()
