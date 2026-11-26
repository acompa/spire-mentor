"""The data behind chapter.html's measured figures, and the figures themselves.

    tour/run stop-7-imitation/figures.py measure   # plays and scores; writes figures.json
    tour/run stop-7-imitation/figures.py render    # figures.json -> SVG inside chapter.html

`measure` needs `prepare.py`'s cache (out/full). It also trains two
soft-label students (distillation on the teacher's full distribution) on
the 25- and 50-run teacher datasets, if they are not cached yet.
`render` rewrites only the regions of chapter.html between
`<!-- fig:NAME -->` and `<!-- /fig:NAME -->`, and the text of every
`<span class="v" data-k=...>` (a value from figures.json).
"""

from __future__ import annotations

import json
import re
import sys
import time
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import tourlib  # noqa: E402

import imitation as I  # noqa: E402

DATA = HERE / "figures.json"
CHAPTER = HERE / "chapter.html"
OUT = HERE / "out" / "full"
PLAN = I.FULL
WORKERS = 3
#: Decision-index bins for the "agreement along a run" curves.
BIN = 50
SOFT = (25, 50)


# ── measuring ─────────────────────────────────────────────────────────────


def view(pt: Path, rb: dict, actions: np.ndarray, batch: int = 512) -> dict[str, np.ndarray]:
    """What the checkpoint `pt` says on each row: its greedy class, the
    probability it gives `actions[i]`, its own top probability and entropy."""
    import torch

    from ppo.e2e import net as N

    dev = I.device()
    net = N.load_checkpoint(pt)[0].to(dev).eval()
    n = len(rb["n_tok"])
    out = {k: [] for k in ("argmax", "p_action", "p_top", "entropy")}
    with torch.no_grad():
        for i in range(0, n, batch):
            idx = np.arange(i, min(n, i + batch))
            b = I._to(N.to_batch(N.select_rows(rb, idx), "cpu"), dev)
            logp = torch.log_softmax(net.policy(b).float(), -1)
            p = logp.exp()
            a = torch.as_tensor(actions[idx], device=dev)
            out["argmax"].append(p.argmax(-1).cpu().numpy())
            out["p_action"].append(p.gather(1, a[:, None])[:, 0].cpu().numpy())
            out["p_top"].append(p.max(-1).values.cpu().numpy())
            ent = -(p * torch.where(p > 0, logp, torch.zeros_like(logp))).sum(-1)
            out["entropy"].append(ent.cpu().numpy())
    return {k: np.concatenate(v) for k, v in out.items()}


def binned(t: np.ndarray, hit: np.ndarray, cap: int = 700) -> list[dict]:
    """Agreement (percent) per bin of decision index `t`, with each bin's count."""
    rows = []
    for lo in range(0, cap, BIN):
        m = (t >= lo) & (t < lo + BIN)
        if m.sum():
            rows.append({"lo": lo, "n": int(m.sum()), "agree": 100.0 * float(hit[m].mean())})
    return rows


def ensure_soft(log=print) -> None:
    rb, y = I.load_rows(OUT / "teacher_rows.npz")
    for n in SOFT:
        dest = OUT / f"soft{n:03d}"
        if not dest.with_suffix(".e2ew").exists():
            sub_rb, sub_y = I.runs(rb, y, PLAN.train_seeds(n))
            log(f"train soft{n:03d} on {len(sub_y)} rows with the teacher's full distribution")
            I.train(sub_rb, sub_y, dest, epochs=PLAN.epochs, soft=True, log=log)


def measure_student(name: str, hrb: dict, hy: np.ndarray, teacher_pt: Path) -> dict:
    pt = OUT / f"{name}.pt"
    games = I.play_many(PLAN.eval_seeds(), pt.with_suffix(".e2ew"), WORKERS)
    srb, chosen = I.gather(games)
    # Teacher states (held out): does the student pick the teacher's move?
    stud = I.choices(pt, hrb)
    t_on_held = view(teacher_pt, hrb, stud)
    held_hit = stud == hy
    # Student states: would the teacher pick the student's move?
    t_on_own = view(teacher_pt, srb, chosen)
    own_hit = t_on_own["argmax"] == chosen
    k = len(chosen) / len(games)
    early = hrb["t"] < k
    return {
        "heldout_agreement": 100.0 * float(held_hit.mean()),
        "heldout_early_agreement": 100.0 * float(held_hit[early].mean()),
        "own_agreement": 100.0 * float(own_hit.mean()),
        "heldout_combat_agreement": 100.0 * float(held_hit[hrb["is_combat"] == 1].mean()),
        "heldout_ooc_agreement": 100.0 * float(held_hit[hrb["is_combat"] == 0].mean()),
        "own_combat_agreement": 100.0 * float(own_hit[srb["is_combat"] == 1].mean()),
        "own_ooc_agreement": 100.0 * float(own_hit[srb["is_combat"] == 0].mean()),
        # How bad are the disagreements? The teacher's probability on the
        # student's move, where the two differ.
        "heldout_p_teacher_on_student_when_differ": float(t_on_held["p_action"][~held_hit].mean()),
        "own_p_teacher_on_student_when_differ": float(t_on_own["p_action"][~own_hit].mean()),
        "heldout_clear_mistake_rate": 100.0 * float((t_on_held["p_action"] < 0.05).mean()),
        "own_clear_mistake_rate": 100.0 * float((t_on_own["p_action"] < 0.05).mean()),
        "heldout_by_t": binned(hrb["t"], held_hit),
        "own_by_t": binned(srb["t"], own_hit),
        "win_rate": 100.0 * float(np.mean([g.won for g in games])),
        "mean_floor": float(np.mean([g.floor for g in games])),
        "decisions_per_run": float(k),
        "floors": [g.floor for g in games],
        "decisions": [g.decisions for g in games],
        "won": [bool(g.won) for g in games],
    }


def measure(log=print) -> None:
    import sts2_rs

    t0 = time.time()
    ensure_soft(log)
    teacher_pt, teacher_w = I.teacher_paths()
    hrb, hy = I.load_rows(OUT / "heldout_rows.npz")
    tview = view(teacher_pt, hrb, hy)
    data = {
        "plan": {"sizes": PLAN.sizes, "heldout_runs": PLAN.heldout, "dagger_runs": PLAN.dagger,
                 "eval_runs": PLAN.evaluate, "epochs": PLAN.epochs, "workers": WORKERS},
        "rows": {
            "teacher_train": {str(n): int(len(I.runs(*I.load_rows(OUT / "teacher_rows.npz"),
                                                          PLAN.train_seeds(n))[1]))
                              for n in PLAN.sizes},
            "heldout": int(len(hy)),
            "dagger": int(len(I.load_rows(OUT / "dagger_rows.npz")[1])),
        },
        "teacher_on_heldout": {
            "mean_p_top": float(tview["p_top"].mean()),
            "share_p_top_over_90pct": 100.0 * float((tview["p_top"] > 0.9).mean()),
            "mean_entropy_nats": float(tview["entropy"].mean()),
            "combat_share": 100.0 * float((hrb["is_combat"] == 1).mean()),
        },
        "students": {},
    }
    for name in [*PLAN.names(), *(f"soft{n:03d}" for n in SOFT)]:
        log(f"{name}: 150 PANEL_A runs and agreement ({time.time() - t0:.0f}s)")
        data["students"][name] = measure_student(name, hrb, hy, teacher_pt)
    log(f"teacher: {PLAN.evaluate} PANEL_A runs ({time.time() - t0:.0f}s)")
    res = tourlib.selfplay_many(sts2_rs.E2eStudent(str(teacher_w)), PLAN.eval_seeds(), WORKERS)
    data["teacher"] = {
        "win_rate": 100.0 * float(np.mean([r["won"] for r in res])),
        "mean_floor": float(np.mean([r["floor"] or 0 for r in res])),
        "decisions_per_run": float(np.mean([r["decisions"] for r in res])),
        "floors": [int(r["floor"] or 0) for r in res],
        "decisions": [int(r["decisions"]) for r in res],
        "won": [bool(r["won"]) for r in res],
    }
    data["seconds"] = round(time.time() - t0)
    DATA.write_text(json.dumps(data, indent=1))
    log(f"wrote {DATA} in {data['seconds']}s")


# ── rendering ─────────────────────────────────────────────────────────────
# Every colour is a class or token from the chapter template, so both themes work.


class Plot:
    """A line/point chart in an SVG viewBox, with labelled axes."""

    def __init__(self, w, h, x0, x1, y0, y1, *, left=56, right=16, top=16, bottom=46):
        self.w, self.h = w, h
        self.x0, self.x1, self.y0, self.y1 = x0, x1, y0, y1
        self.l, self.r, self.t, self.b = left, w - right, top, h - bottom
        self.parts: list[str] = []

    def X(self, x):
        return self.l + (x - self.x0) / (self.x1 - self.x0) * (self.r - self.l)

    def Y(self, y):
        return self.b - (y - self.y0) / (self.y1 - self.y0) * (self.b - self.t)

    def axes(self, xticks, yticks, xlabel, ylabel, xfmt=str, yfmt=str):
        p = self.parts
        for y in yticks:
            p.append(f'<line class="grid" x1="{self.l}" x2="{self.r}" y1="{self.Y(y):.1f}" y2="{self.Y(y):.1f}"/>')
            p.append(f'<text class="muted" x="{self.l - 6}" y="{self.Y(y) + 4:.1f}" text-anchor="end">{yfmt(y)}</text>')
        for x in xticks:
            p.append(f'<text class="muted" x="{self.X(x):.1f}" y="{self.b + 16}" text-anchor="middle">{xfmt(x)}</text>')
        p.append(f'<line class="ink" x1="{self.l}" x2="{self.r}" y1="{self.b}" y2="{self.b}"/>')
        p.append(f'<line class="ink" x1="{self.l}" x2="{self.l}" y1="{self.t}" y2="{self.b}"/>')
        p.append(f'<text x="{(self.l + self.r) / 2:.1f}" y="{self.h - 8}" text-anchor="middle">{xlabel}</text>')
        p.append(f'<text x="14" y="{(self.t + self.b) / 2:.1f}" text-anchor="middle" '
                 f'transform="rotate(-90 14 {(self.t + self.b) / 2:.1f})">{ylabel}</text>')

    def line(self, pts, cls, width=2, dash=None):
        d = " ".join(f"{'M' if i == 0 else 'L'}{self.X(x):.1f} {self.Y(y):.1f}" for i, (x, y) in enumerate(pts))
        dd = f' stroke-dasharray="{dash}"' if dash else ""
        self.parts.append(f'<path class="{cls}" d="{d}" stroke-width="{width}"{dd}/>')

    def dots(self, pts, fcls, r=3.5):
        for x, y in pts:
            self.parts.append(f'<circle class="{fcls}" cx="{self.X(x):.1f}" cy="{self.Y(y):.1f}" r="{r}"/>')

    def label(self, x, y, text, cls="", anchor="start", dx=6, dy=4):
        c = f' class="{cls}"' if cls else ""
        self.parts.append(f'<text{c} x="{self.X(x) + dx:.1f}" y="{self.Y(y) + dy:.1f}" text-anchor="{anchor}">{text}</text>')

    def svg(self, aria):
        return (f'<svg viewBox="0 0 {self.w} {self.h}" role="img" aria-label="{aria}">'
                + "".join(self.parts) + "</svg>")


def fig_bound() -> str:
    """Theory, not measurement: the two bounds for a per-decision error of 1%."""
    eps = 0.01
    T = 700
    p = Plot(560, 300, 0, T, 0, 100)
    p.axes([0, 100, 200, 300, 400, 500, 600, 700], [0, 25, 50, 75, 100],
           "horizon T (decisions)", "bound on extra cost (units)")
    ts = list(range(0, T + 1, 10))
    p.line([(t, eps * t) for t in ts], "l1", 2.5)
    # The quadratic leaves the chart at T = 100; draw it only that far.
    p.line([(t, eps * t * t) for t in range(0, 101, 5)], "l2", 2.5)
    p.label(100, 100, f"off the chart: {eps * T * T:,.0f} at T = {T}", "muted", "start", 8, 30)
    p.label(640, eps * 640, "εT (DAgger)", "", "end", -4, -8)
    p.label(100, 100, "εT² (behaviour cloning)", "", "start", 4, 14)
    return p.svg("Two bounds on extra cost against the horizon T for per-step error 0.01: "
                 "the linear bound stays below 7 at T = 700, the quadratic one passes 100 at T = 100.")


def fig_agreement_along_run(d) -> str:
    s = d["students"]["bc100"]
    p = Plot(560, 300, 0, 700, 50, 100)
    p.axes([0, 100, 200, 300, 400, 500, 600, 700], [50, 60, 70, 80, 90, 100],
           "decision index in the run", "agreement with teacher (%)")
    for key, cls, f in (("heldout_by_t", "l1", "f1"), ("own_by_t", "l2", "f2")):
        pts = [(b["lo"] + BIN / 2, b["agree"]) for b in s[key] if b["n"] >= 100]
        p.line(pts, cls)
        p.dots(pts, f)
    # Labels sit beside the early bins, where the curves are far apart.
    h = next(b for b in s["heldout_by_t"] if b["lo"] == 100)
    o = next(b for b in s["own_by_t"] if b["lo"] == 250)
    p.label(h["lo"] + BIN / 2, h["agree"], "teacher's states (held out)", "", "start", 4, -12)
    p.label(o["lo"] + BIN / 2, o["agree"], "student's own states", "", "middle", 0, 22)
    return p.svg("Agreement of the 100-run student with the teacher by decision index, on teacher "
                 "states and on the student's own states.")


def fig_survival(d) -> str:
    """Share of runs still alive at each floor (a run that ends at floor f
    counts as alive up to f)."""
    p = Plot(560, 300, 0, 50, 0, 100)
    p.axes([0, 10, 20, 30, 40, 50], [0, 25, 50, 75, 100], "floor", "runs still going (%)")
    series = [("teacher", d["teacher"], "l4", 2.5, None),
              ("bc025", d["students"]["bc025"], "l3", 2, "4 3"),
              ("bc100", d["students"]["bc100"], "l1", 2.5, None),
              ("DAgger", d["students"]["dagger"], "l2", 2.5, None)]
    # Each label sits just off its own curve at a floor where the curves are apart.
    where = {"teacher": (30, "start", 0, -8), "bc025": (8, "end", -8, 0),
             "bc100": (22, "start", 0, 14), "DAgger": (22, "start", 0, -8)}
    for name, s, cls, w, dash in series:
        f = np.array(s["floors"])
        pts = [(x, 100.0 * float((f >= x).mean())) for x in range(0, 51)]
        p.line(pts, cls, w, dash)
        x, anchor, dx, dy = where[name]
        p.label(x, 100.0 * float((f >= x).mean()), name, "", anchor, dx, dy)
    return p.svg("Share of 150 PANEL_A runs still going at each floor, for the teacher, the 25- and "
                 "100-run students and the DAgger student.")


def fig_data_curve(d) -> str:
    """Held-out agreement against teacher runs in the dataset (log2 axis),
    hard against soft labels."""
    S = d["students"]
    L = np.log2
    p = Plot(560, 300, L(20), L(120), 60, 80)
    p.axes([L(25), L(50), L(100)], [60, 65, 70, 75, 80], "teacher runs in the training set (log scale)",
           "held-out agreement (%)", xfmt=lambda v: f"{2 ** v:.0f}")
    hard = [(L(n), S[f"bc{n:03d}"]["heldout_agreement"]) for n in (25, 50, 100)]
    soft = [(L(n), S[f"soft{n:03d}"]["heldout_agreement"]) for n in SOFT]
    p.line(hard, "l1")
    p.dots(hard, "f1")
    p.line(soft, "l2", 2, "5 3")
    p.dots(soft, "f2", 3)
    p.label(*hard[-1], "hard labels", "", "end", -8, -10)
    p.label(*soft[-1], "soft labels", "", "end", -10, -12)
    return p.svg("Held-out agreement against the number of teacher runs on a log scale, for students "
                 "trained on the teacher's chosen move and on its full distribution: the two nearly coincide.")


def paired(a: list, b: list) -> dict:
    """Mean and standard error of the per-seed difference a - b."""
    x = np.asarray(a, float) - np.asarray(b, float)
    return {"mean": float(x.mean()), "se": float(x.std(ddof=1) / np.sqrt(len(x)))}


def derived(d: dict) -> dict:
    """Quantities the chapter quotes that are computed from the measured ones."""
    S, t = d["students"], d["teacher"]
    out = {
        "floor_gain": {
            "dagger_vs_bc100": paired(S["dagger"]["floors"], S["bc100"]["floors"]),
            "bc100_vs_bc050": paired(S["bc100"]["floors"], S["bc050"]["floors"]),
            "soft025_vs_bc025": paired(S["soft025"]["floors"], S["bc025"]["floors"]),
            "soft050_vs_bc050": paired(S["soft050"]["floors"], S["bc050"]["floors"]),
        },
        "wins": {k: float(sum(v["won"])) for k, v in {**S, "teacher": t}.items()},
        "past_floor_17": {k: 100.0 * float(np.mean(np.asarray(v["floors"]) >= 17))
                          for k, v in {**S, "teacher": t}.items()},
    }
    # If mean floor grows with log(data), as bc025 -> bc050 -> bc100 suggests,
    # the DAgger round's extra rows are worth this many floors as teacher rows.
    r = d["rows"]
    extra = np.log2((r["teacher_train"]["100"] + r["dagger"]) / r["teacher_train"]["100"])
    out["dagger_extra_doublings"] = float(extra)
    out["floors_if_teacher_rows"] = float(extra * out["floor_gain"]["bc100_vs_bc050"]["mean"])
    out["dagger_extra_pct"] = 100.0 * r["dagger"] / r["teacher_train"]["100"]
    # Equation (survival) solved for the fatal-mistake rate that turns the
    # teacher's win rate into the 100-run student's, over a teacher-length run.
    eps_f = -np.log(S["bc100"]["win_rate"] / t["win_rate"]) / t["decisions_per_run"]
    out["eps_f_pct"] = 100.0 * float(eps_f)
    out["decisions_per_fatal"] = float(1.0 / eps_f)
    out["clear_per_fatal"] = S["bc100"]["own_clear_mistake_rate"] / 100.0 / float(eps_f)
    return out


def lookup(d: dict, key: str) -> float:
    """A dotted path into figures.json; `a - b` gives a difference of two paths."""
    if " - " in key:
        a, b = key.split(" - ")
        return lookup(d, a) - lookup(d, b)
    v = d
    for part in key.split("."):
        v = v[part]
    return float(v)


def fill_values(html: str, d: dict) -> str:
    """Replace the text of every `<span class="v" data-k=KEY data-f=FMT>` with the measured value."""
    pat = re.compile(r'(<span class="v" data-k="([^"]+)" data-f="([^"]+)">)[^<]*(</span>)')
    return pat.sub(lambda m: m.group(1) + format(lookup(d, m.group(2)), m.group(3)) + m.group(4), html)


def render() -> None:
    d = json.loads(DATA.read_text())
    figs = {"bound": fig_bound(), "along": fig_agreement_along_run(d),
            "survival": fig_survival(d), "datacurve": fig_data_curve(d)}
    html = CHAPTER.read_text()
    for name, svg in figs.items():
        pat = re.compile(rf"(<!-- fig:{name} -->).*?(<!-- /fig:{name} -->)", re.S)
        if not pat.search(html):
            raise SystemExit(f"chapter.html has no fig:{name} region")
        html = pat.sub(lambda m: m.group(1) + svg + m.group(2), html)
    html = fill_values(html, {**d, "derived": derived(d)})
    CHAPTER.write_text(html)
    print(f"rendered {', '.join(figs)} and the measured values into {CHAPTER.name}")


if __name__ == "__main__":
    {"measure": measure, "render": render}[sys.argv[1] if len(sys.argv) > 1 else "measure"]()
