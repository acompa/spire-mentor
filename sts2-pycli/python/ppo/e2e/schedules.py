"""Clocks and schedules of the e2e learner (plan §6 to §8), as pure functions.

Every schedule here is a pure function of explicit clocks. A resume that
restores the clocks from ``state.pt`` therefore reproduces every schedule
exactly. The clocks are:

- ``t_wall``: wall time in Unix seconds. It drives the learning-rate anneal and
  the freeze of the KL-adaptive factor (both start Sun Nov 8 2026, 06:00 CET).
  Read it once per iteration, at the update start, and log it.
- ``it``: completed updates since T0. It drives the lr warmup and the
  value-only iterations (V-warm).
- ``decisions``: stored policy rows collected since T0 (bootstrap copies are not
  counted). It drives teacher acting (β_act) and the end of U0.
- the *KS clock*, ``decisions - ks_offset``: it drives λ_KS and the entropy
  schedule. The offset lets the learning check hold λ_KS at 1 (§7). While
  ``ks_hold`` is set, λ_KS is 1. When a check passes, `Clocks.pass_learning_check`
  sets the offset so that the 1 → 0 ramp starts at that decision count. The
  check re-runs at K2 and at each restart. Once one has passed, a failed
  re-check leaves the clocks and the ramp alone: `Clocks.hold_ks` returns
  False, and the learner turns that into an OPERATOR alarm.

Phases (§7): V-warm (the first ``value_only_iters`` updates train the value
only), U0 (decisions < 80M: the teachers act with probability β_act), U1
(λ_KS > 0) and U2 (λ_KS = 0). KL control runs only in U1 and U2: the probe's
hard stop and the lr factor m.

The lr of a parameter group is ``base · m · anneal(t_wall) · warmup(it)``.
`check_lr_row` recomputes it from a logged stats row. The ops watchdog alarms
when the logged lr is off its closed form by more than 1e-6 (§8 Runbook).

Per-iteration protocol (`ppo.e2e.algo.AlgoState` wraps it):

1. ``sched = schedule_for(clocks, t_wall, kl.m, collapse, cfg, teacher_types)``
   after ``clocks.decisions`` counts the new batch;
2. the update uses ``sched`` for every lr, β and λ, and calls
   ``kl.stop(probe_kl, sched, cfg)`` after each epoch;
3. ``kl.update(probe_kl, sched, cfg)``, ``collapse.update(entropy, sched, cfg)``
   and ``clocks.it += 1``.
"""

from __future__ import annotations

import math
from collections.abc import Iterable, Mapping
from dataclasses import asdict, dataclass, field
from datetime import datetime, timedelta, timezone
from typing import Any

import numpy as np

__all__ = [
    "ANNEAL_START",
    "CET",
    "FLOOR_SHARE",
    "LR_LOG_KEYS",
    "Clocks",
    "EntropyCollapse",
    "IterSchedule",
    "KLControl",
    "LrWatch",
    "ScheduleCfg",
    "anneal",
    "beta_act",
    "check_lr_row",
    "ent_gate_decisions",
    "entropy_beta",
    "floor_coef_at",
    "floor_mask",
    "kl_control_on",
    "lam_ks",
    "lr",
    "lr_frac",
    "lr_hold",
    "phase",
    "policy_on",
    "schedule_for",
    "warmup",
]

#: Central European Time in November (UTC+1; daylight saving ended Oct 25).
#: A fixed offset, not a zoneinfo zone: Windows may lack the tz database.
CET = timezone(timedelta(hours=1), "CET")
#: Sun Nov 8 2026, 06:00 CET: the lr anneal starts and the KL factor freezes.
ANNEAL_START = datetime(2026, 11, 8, 6, 0, tzinfo=CET).timestamp()
#: `ScheduleCfg.distill_source`'s values.
DISTILL_SOURCES = ("update", "acting")


@dataclass(frozen=True)
class ScheduleCfg:
    """Every schedule constant of plan §6 to §8. The defaults are the plan's."""

    # Wall clock: anneal(t) = 1 - anneal_drop * clamp((t - anneal_start) / anneal_hours).
    anneal_start: float = ANNEAL_START
    anneal_hours: float = 24.0
    anneal_drop: float = 0.7
    # The KL-adaptive factor m stops changing from this wall time on.
    kl_freeze_at: float = ANNEAL_START
    # Base learning rates of the two AdamW groups (§6 Optimizer).
    lr_trunk: float = 1e-4
    lr_value: float = 3e-4
    # Base lr of a third group, "input" (`net.INPUT_ROOTS`: the ID table and its gains,
    # the small-column and section tables, the numeric stem), split off the trunk side.
    # None: no such group, the input layers train in "trunk" at lr_trunk (as before).
    # GH #1197's adapt phase sets lr_trunk 0 and lr_input > 0: only the input layers
    # and the value module train (LP-FT style). m, anneal, warmup and the hold apply alike.
    lr_input: float | None = None
    # lr is 0 while decisions < lr_hold_decisions (surgery restarts: Adam's m/v settle on
    # the new data before any weight moves; the optimizer still steps). 0 = off. Lane sched
    # of the all-char restart (docs only, not defaults): {"lr_trunk": 5e-5, "lr_value": 1.5e-4,
    # "lr_hold_decisions": 5000000, "u0_end": 1, "ks_end": 2, "ent_combat": [0.002, 0.002],
    # "ent_ooc": [0.006, 0.006], "anneal_start": 1798779600, "kl_freeze_at": 1798779600,
    # "budget_decisions": 1500000000}.
    lr_hold_decisions: int = 0
    # Linear lr warmup over the first iterations: warmup(it) = min(1, (it + 1) / n).
    lr_warmup_iters: int = 10
    # The first iterations train the value only (V-warm, §7).
    value_only_iters: int = 3
    # Decisions since T0: U0 ends and β_act reaches 0 here; λ_KS starts its ramp.
    u0_end: int = 80_000_000
    # λ_KS reaches 0 here (on the KS clock).
    ks_end: int = 500_000_000
    # Entropy β of a type is 0 while its λ_KS > ent_gate (374M on the KS clock).
    ent_gate: float = 0.3
    # β decays geometrically from the gate (374M) to ent_end (KS clock), per family.
    ent_end: int = 4_000_000_000
    ent_combat: tuple[float, float] = (0.01, 0.002)
    ent_ooc: tuple[float, float] = (0.0125, 0.006)
    # KL control (U1, U2): hard stop above kl_stop after an epoch; after each
    # iteration m *= kl_mult below kl_low, m /= kl_mult above kl_high, in [m_min, m_max].
    kl_low: float = 0.005
    kl_high: float = 0.02
    kl_stop: float = 0.06
    kl_mult: float = 1.5
    kl_m_min: float = 0.3
    kl_m_max: float = 2.0
    # Entropy collapse: raw entropy below collapse_nats for collapse_iters iterations
    # lifts the type's gate and multiplies its β by collapse_boost, at most collapse_max times.
    collapse_nats: float = 0.1
    collapse_iters: int = 20
    collapse_boost: float = 1.5
    collapse_max: int = 2
    # GH #1541: a probability floor per legal option on the run choices that the
    # global β lets collapse (a card offered 1,700 times and never taken gets no
    # gradient). On a policy row of a `floor_types` type, every class whose π is
    # below p = min(floor_p, floor_share / n_classes) adds
    # floor_coef · ramp · (log p − log π) to the loss. floor_coef 0 (the default) is off.
    #
    # Switched on at full strength, the floor flattened every OOC type on the
    # allchar lane within 3 iterations, the unfloored ones too: the cheapest way
    # to lift every tiny π is to shrink the shared OOC logits (a temperature), and
    # a 50% share let SHOP go near-uniform (train win 0.42 -> 0.05). Hence a small
    # share, no SHOP by default, and a ramp: the coefficient rises linearly from 0
    # at decision `floor_start` to floor_coef at floor_start + floor_ramp (on the
    # learner's decision clock; floor_ramp 0 = a step at floor_start).
    floor_p: float = 0.02
    floor_coef: float = 0.0
    floor_types: tuple[str, ...] = ("CARD_REWARD", "CARD_BUNDLE")
    floor_share: float = 0.1
    floor_start: int = 0
    floor_ramp: int = 20_000_000
    # The run stops here. #1081's 1e9 annealed lr to 0 and stopped at 1B.
    budget_decisions: int = 10_000_000_000
    # Learner-side distillation (GH #1197: the all-character lane forgets Ironclad).
    # A frozen teacher net (`distill_ckpt`, a student export `--init` accepts, its file
    # sha256 `distill_sha256`) runs on the update's own minibatch inputs; on the trained
    # rows of `distill_chars` the loss adds distill_lambda * KL(pi_T || pi_S) through the
    # kickstarting term (`algo.ppo_losses`). Off while distill_ckpt is None or the
    # lambda is 0. It leaves lam_ks, the entropy gate and the KL control alone, and the
    # update refuses it while the stored-row KS teacher is on. Only the path and the
    # sha are saved (in state.pt's algo.cfg), never the teacher's weights. A state
    # from before these fields existed loads with the defaults (off).
    distill_ckpt: str | None = None
    distill_sha256: str | None = None
    distill_lambda: float = 0.0
    distill_chars: tuple[str, ...] = ("Ironclad",)
    # Where the teacher's probabilities come from. "update": the update rank runs the
    # teacher on the stored rows (the rows the student reads). "acting": the workers
    # run a chars-off shadow env beside each distill-character segment, the learner
    # scores its rows with the teacher when it serves them and stores the probs in
    # the rows' `tprob`; the update reads those and runs no teacher. "acting" is how a
    # teacher trained without the "chars" fill event sees its own inputs on a lane
    # whose rows carry it (`update.check_distill_teacher_inputs`).
    distill_source: str = "update"
    # "acting": the learner scores buffered shadow rows once this many are waiting
    # (and always before a collection ends).
    distill_flush_rows: int = 2048
    # "acting": the RareMap the teacher's own training rows carried on their combat
    # S_ctx content slots (a lane with a #1081 combat teacher, `--teacher-pair R3`,
    # stores its rows through that teacher's map): a #1081 combat checkpoint holding
    # it (`ck["rare"]`) and the map's `teachers.rare_sha256`. The shadow tokenizes
    # through it, so the teacher reads exactly its training rows. None: universe rows.
    distill_rare: str | None = None
    distill_rare_sha256: str | None = None

    def __post_init__(self) -> None:
        if self.distill_source not in DISTILL_SOURCES:
            raise ValueError(
                f"distill_source {self.distill_source!r}: one of {list(DISTILL_SOURCES)}"
            )
        if int(self.distill_flush_rows) < 1:
            raise ValueError(f"distill_flush_rows {self.distill_flush_rows!r} must be >= 1")
        if self.distill_ckpt is not None:
            if not self.distill_sha256:
                raise ValueError("distill_ckpt needs distill_sha256 (the file's sha256)")
            if not self.distill_chars:
                raise ValueError("distill_ckpt needs at least one of distill_chars")
        if (self.distill_rare is None) != (self.distill_rare_sha256 is None):
            raise ValueError("distill_rare and distill_rare_sha256 go together")
        if self.distill_rare is not None and self.distill_source != "acting":
            raise ValueError(
                'distill_rare needs distill_source "acting" (only the shadow rows take it)'
            )
        lam = float(self.distill_lambda)
        if not (math.isfinite(lam) and lam >= 0.0):
            raise ValueError(f"distill_lambda {self.distill_lambda!r} must be finite and >= 0")

    @property
    def distill_on(self) -> bool:
        """Whether the update runs the distillation teacher (a checkpoint and lambda > 0)."""
        return self.distill_ckpt is not None and float(self.distill_lambda) > 0.0

    @property
    def distill_acting(self) -> bool:
        """Whether the teacher scores the workers' chars-off shadow rows at serving time."""
        return self.distill_on and self.distill_source == "acting"

    def to_dict(self) -> dict[str, Any]:
        return {k: list(v) if isinstance(v, tuple) else v for k, v in asdict(self).items()}

    def base_lrs(self) -> dict[str, float]:
        """Base lr per optimizer group, in group order: "input" (only with lr_input set),
        "trunk", "value". Everything that knows the groups iterates this."""
        out = {} if self.lr_input is None else {"input": float(self.lr_input)}
        return out | {"trunk": float(self.lr_trunk), "value": float(self.lr_value)}

    @classmethod
    def from_dict(cls, d: Mapping[str, Any]) -> ScheduleCfg:
        return cls(**{k: tuple(v) if isinstance(v, list) else v for k, v in d.items()})


DEFAULT = ScheduleCfg()

#: The default of `ScheduleCfg.floor_share`: the floors of a row's n classes claim at
#: most this share of its mass (each is floored at most at floor_share / n).
FLOOR_SHARE = 0.1


def _clamp01(x: float) -> float:
    return min(1.0, max(0.0, x))


# ── pure schedules ───────────────────────────────────────────────────────


def anneal(t_wall: float, cfg: ScheduleCfg = DEFAULT) -> float:
    """lr anneal factor: 1 before the start, then linear to 1 - anneal_drop over anneal_hours."""
    span = cfg.anneal_hours * 3600.0
    return 1.0 - cfg.anneal_drop * _clamp01((t_wall - cfg.anneal_start) / span)


def warmup(it: int, cfg: ScheduleCfg = DEFAULT) -> float:
    """Linear lr warmup of the update with index ``it`` (0-based): 1/n, 2/n, ..., 1."""
    if cfg.lr_warmup_iters <= 0:
        return 1.0
    return min(1.0, (it + 1) / cfg.lr_warmup_iters)


def lr_hold(decisions: int, cfg: ScheduleCfg = DEFAULT) -> bool:
    """True while the lr hold phase is on: decisions < lr_hold_decisions."""
    return int(decisions) < cfg.lr_hold_decisions


def lr_frac(
    t_wall: float, it: int, m: float, cfg: ScheduleCfg = DEFAULT, decisions: int = 0
) -> float:
    """The factor every base lr is multiplied by: m · anneal(t_wall) · warmup(it).

    0.0 while ``decisions`` (the learner's global decision clock) is below
    ``cfg.lr_hold_decisions``.
    """
    if lr_hold(decisions, cfg):
        return 0.0
    return m * anneal(t_wall, cfg) * warmup(it, cfg)


def lr(
    base: float, t_wall: float, it: int, m: float, cfg: ScheduleCfg = DEFAULT, decisions: int = 0
) -> float:
    """The closed-form lr of a group with base lr ``base``."""
    return base * lr_frac(t_wall, it, m, cfg, decisions)


def policy_on(it: int, cfg: ScheduleCfg = DEFAULT) -> bool:
    """False during V-warm: the update with index ``it`` trains the value only."""
    return it >= cfg.value_only_iters


def beta_act(decisions: int, cfg: ScheduleCfg = DEFAULT) -> float:
    """Teacher acting probability: 1 → 0 linearly over decisions [0, u0_end]."""
    return 1.0 - _clamp01(decisions / cfg.u0_end)


def lam_ks(
    decisions: int, cfg: ScheduleCfg = DEFAULT, *, hold: bool = False, offset: int = 0
) -> float:
    """Kickstarting weight: 1 until u0_end, then linear to 0 at ks_end, on the KS clock.

    ``hold`` keeps it at 1 (a failed learning check). ``offset`` shifts the ramp
    to start later (`Clocks.pass_learning_check`).
    """
    if hold:
        return 1.0
    d = decisions - offset
    return 1.0 - _clamp01((d - cfg.u0_end) / (cfg.ks_end - cfg.u0_end))


def ent_gate_decisions(cfg: ScheduleCfg = DEFAULT) -> float:
    """KS-clock decisions at which λ_KS falls to ent_gate (374M): the entropy gate."""
    return cfg.u0_end + (1.0 - cfg.ent_gate) * (cfg.ks_end - cfg.u0_end)


def entropy_beta(
    decisions: int, combat: bool, cfg: ScheduleCfg = DEFAULT, *, offset: int = 0
) -> float:
    """The family's entropy coefficient before gates and boosts.

    Geometric from the family's start value at the gate (374M) to its end value
    at ent_end (4B), on the KS clock; constant outside that span.
    """
    b0, b1 = cfg.ent_combat if combat else cfg.ent_ooc
    g = ent_gate_decisions(cfg)
    f = _clamp01((decisions - offset - g) / (cfg.ent_end - g))
    return b0 * (b1 / b0) ** f


def phase(
    it: int, decisions: int, cfg: ScheduleCfg = DEFAULT, *, hold: bool = False, offset: int = 0
) -> str:
    """The warm-start phase (§7) of the update with index ``it``."""
    if not policy_on(it, cfg):
        return "V-warm"
    if decisions < cfg.u0_end:
        return "U0"
    if lam_ks(decisions, cfg, hold=hold, offset=offset) > 0.0:
        return "U1"
    return "U2"


def kl_control_on(ph: str) -> bool:
    """KL control (probe hard stop, factor m) runs in U1 and U2 only (§6)."""
    return ph in ("U1", "U2")


# ── state ────────────────────────────────────────────────────────────────


@dataclass
class Clocks:
    """Iteration and decision counters plus the λ_KS hold and pass (all in ``state.pt``)."""

    it: int = 0
    decisions: int = 0
    ks_hold: bool = False
    ks_offset: int = 0
    ks_passed: bool = False  # a learning check has passed: the λ_KS ramp is never re-held

    def hold_ks(self) -> bool:
        """A learning check failed (§7). Return True iff λ_KS is now held at 1.

        Before any check has passed, this holds λ_KS at 1 and returns True.
        Once one has passed, the ramp it started stands. A failed re-check (K2
        or a restart) then leaves the clocks unchanged and returns False, and
        the caller raises an OPERATOR alarm instead of resetting λ_KS to 1 and
        shifting the ramp.
        """
        if self.ks_passed:
            return False
        self.ks_hold = True
        return True

    def pass_learning_check(self, cfg: ScheduleCfg = DEFAULT) -> None:
        """A learning check passed: if λ_KS was held, its ramp starts now.

        The offset makes λ_KS continuous (1 at this decision count). A check
        that passes before u0_end leaves the ramp where the plan put it. Either
        way ``ks_passed`` is set for good, so `hold_ks` will not re-hold λ_KS.
        """
        if self.ks_hold:
            self.ks_offset = max(0, self.decisions - cfg.u0_end)
            self.ks_hold = False
        self.ks_passed = True

    def ks_decisions(self) -> int:
        return self.decisions - self.ks_offset

    def more(self, cfg: ScheduleCfg = DEFAULT) -> bool:
        return self.decisions < cfg.budget_decisions

    def to_dict(self) -> dict[str, Any]:
        return {
            "it": int(self.it),
            "decisions": int(self.decisions),
            "ks_hold": bool(self.ks_hold),
            "ks_offset": int(self.ks_offset),
            "ks_passed": bool(self.ks_passed),
        }

    @classmethod
    def from_dict(cls, d: Mapping[str, Any]) -> Clocks:
        return cls(
            int(d["it"]),
            int(d["decisions"]),
            bool(d["ks_hold"]),
            int(d["ks_offset"]),
            bool(d["ks_passed"]),
        )


@dataclass(eq=False)
class IterSchedule:
    """Every schedule value of one update, fixed at its start (`schedule_for`)."""

    it: int
    decisions: int
    ks_decisions: int
    t_wall: float
    phase: str
    policy_on: bool
    kl_control: bool
    kl_m: float
    anneal: float
    warmup: float
    lr_frac: float
    lr_base_trunk: float
    lr_base_value: float
    lam_ks: float
    ks_hold: bool
    ks_offset: int
    ks_passed: bool
    beta_act: float
    beta_combat: float
    beta_ooc: float
    #: λ_KS per decision type: lam_ks for types with a teacher, 0 for teacherless ones.
    ks_types: np.ndarray = field(repr=False)
    #: Per decision type: 0 while the entropy gate is closed, else the collapse boost.
    ent_gain: np.ndarray = field(repr=False)
    #: The lr hold phase is on: lr_frac is 0 and the KL factor m does not adapt.
    lr_hold: bool = False
    #: The "input" group's base lr (`ScheduleCfg.lr_input`); None: it follows the trunk.
    lr_base_input: float | None = None
    #: GH #1541's probability floor: its coefficient (0 = off), nominal floor, and the
    #: decision types it applies to (bool per type; None: none).
    floor_coef: float = 0.0
    floor_p: float = 0.0
    floor_on: np.ndarray | None = field(default=None, repr=False)
    floor_share: float = FLOOR_SHARE
    #: The ramp this coefficient is on (`floor_coef_at`), logged so a row names it.
    floor_start: int = 0
    floor_ramp: int = 0

    @property
    def lr_trunk(self) -> float:
        return self.lr_base_trunk * self.lr_frac

    @property
    def lr_value(self) -> float:
        return self.lr_base_value * self.lr_frac

    @property
    def lr_input(self) -> float:
        """The input layers' lr (the trunk's while there is no "input" group)."""
        base = self.lr_base_trunk if self.lr_base_input is None else self.lr_base_input
        return base * self.lr_frac

    def beta_rows(self, dtype: np.ndarray, is_combat: np.ndarray) -> np.ndarray:
        """Entropy coefficient per row: gain[dtype] × the row family's β."""
        fam = np.where(np.asarray(is_combat, bool), self.beta_combat, self.beta_ooc)
        return self.ent_gain[np.asarray(dtype, np.int64)] * fam

    def floor_rows(self, dtype: np.ndarray, n_classes: np.ndarray) -> np.ndarray:
        """The probability floor per row (GH #1541): min(floor_p, floor_share / n_classes)
        on rows of a floored type, 0 elsewhere and while the floor is off. `floor_coef`
        is already ramped (`floor_coef_at`)."""
        dt = np.asarray(dtype, np.int64)
        if self.floor_coef <= 0.0 or self.floor_p <= 0.0 or self.floor_on is None:
            return np.zeros(dt.shape, np.float32)
        nc = np.maximum(np.asarray(n_classes, np.float64), 1.0)
        p = np.minimum(self.floor_p, self.floor_share / nc)
        return np.where(self.floor_on[dt], p, 0.0).astype(np.float32)

    def ks_rows(self, dtype: np.ndarray, has_teacher: np.ndarray) -> np.ndarray:
        """Kickstarting weight per row: λ_KS of its type, 0 for rows without teacher output."""
        lam = self.ks_types[np.asarray(dtype, np.int64)]
        return np.where(np.asarray(has_teacher, bool), lam, 0.0)

    def log_fields(self, applied: Mapping[str, float] | None = None) -> dict[str, Any]:
        """Stats-row fields, including every input of `check_lr_row`.

        Pass ``applied`` = `ppo.e2e.algo.set_lr`'s return value: the lrs read back
        from the optimizer. The watchdog then checks what the optimizer used,
        not a recomputation.
        """
        lr_t = self.lr_trunk if applied is None else float(applied["trunk"])
        lr_v = self.lr_value if applied is None else float(applied["value"])
        # Without an "input" group the input layers sit in "trunk" and take its lr.
        lr_i = self.lr_input if applied is None else float(applied.get("input", lr_t))
        base_i = self.lr_base_trunk if self.lr_base_input is None else self.lr_base_input
        return {
            "it": self.it,
            "decisions": self.decisions,
            "ks_decisions": self.ks_decisions,
            "t_wall": self.t_wall,
            "phase": self.phase,
            "policy_on": self.policy_on,
            "kl_control": self.kl_control,
            "kl_m": self.kl_m,
            "anneal": self.anneal,
            "warmup": self.warmup,
            "lr_frac": self.lr_frac,
            "lr_hold": self.lr_hold,
            "lr_base_trunk": self.lr_base_trunk,
            "lr_base_value": self.lr_base_value,
            "lr_trunk": lr_t,
            "lr_value": lr_v,
            "lr_base_input": base_i,
            "lr_input": lr_i,
            "lam_ks": self.lam_ks,
            "ks_hold": self.ks_hold,
            "ks_offset": self.ks_offset,
            "ks_passed": self.ks_passed,
            "beta_act": self.beta_act,
            "beta_combat": self.beta_combat,
            "beta_ooc": self.beta_ooc,
            "ent_gated_types": int(np.sum(self.ent_gain <= 0.0)),
            "floor_coef": self.floor_coef,
            "floor_p": self.floor_p,
            "floor_share": self.floor_share,
            "floor_start": self.floor_start,
            "floor_ramp": self.floor_ramp,
        }


@dataclass
class KLControl:
    """The KL-adaptive lr factor m (§6 KL control), carried in ``state.pt``."""

    m: float = 1.0

    def stop(self, probe_kl: float, sched: IterSchedule, cfg: ScheduleCfg = DEFAULT) -> bool:
        """After an epoch: True iff the remaining epochs must be skipped (U1/U2, KL > kl_stop)."""
        return bool(sched.kl_control and probe_kl > cfg.kl_stop)

    def update(
        self, probe_kl: float | None, sched: IterSchedule, cfg: ScheduleCfg = DEFAULT
    ) -> float:
        """After the update: adapt m from the last probe KL; return the new m.

        No change in V-warm and U0, during the lr hold (lr 0 makes the KL
        trivially low), from kl_freeze_at on, or for a missing or non-finite KL.
        """
        if (
            not sched.kl_control
            or sched.lr_hold
            or probe_kl is None
            or not math.isfinite(probe_kl)
            or sched.t_wall >= cfg.kl_freeze_at
        ):
            return self.m
        m = self.m
        if probe_kl < cfg.kl_low:
            m *= cfg.kl_mult
        elif probe_kl > cfg.kl_high:
            m /= cfg.kl_mult
        self.m = min(cfg.kl_m_max, max(cfg.kl_m_min, m))
        return self.m

    def to_dict(self) -> dict[str, Any]:
        return {"m": float(self.m)}

    @classmethod
    def from_dict(cls, d: Mapping[str, Any]) -> KLControl:
        return cls(float(d["m"]))


class EntropyCollapse:
    """Per decision type: the entropy-collapse counters and boosts (§6 Entropy).

    A type whose mean raw entropy (nats, student-acted rows) stays below
    ``collapse_nats`` for ``collapse_iters`` consecutive iterations *with data*
    collapses: its gate lifts and its β gain grows by ``collapse_boost``, at most
    ``collapse_max`` times. An iteration without rows of a type leaves its
    counter unchanged; one at or above the threshold resets it.
    """

    def __init__(self, n_types: int = 256) -> None:
        self.low = np.zeros(n_types, np.int64)
        self.boosts = np.zeros(n_types, np.int64)
        self.lifted = np.zeros(n_types, bool)

    def gain(self, cfg: ScheduleCfg = DEFAULT) -> np.ndarray:
        return cfg.collapse_boost ** self.boosts.astype(np.float64)

    def update(
        self, ent_mean: np.ndarray | None, sched: IterSchedule, cfg: ScheduleCfg = DEFAULT
    ) -> np.ndarray:
        """Count one iteration's per-type mean raw entropy (NaN = no rows); return the types that fired.

        Runs only when the policy trained (not in V-warm).
        """
        if ent_mean is None or not sched.policy_on:
            return np.zeros(0, np.int64)
        e = np.asarray(ent_mean, np.float64)
        if e.shape != self.low.shape:
            raise ValueError(f"entropy per type has shape {e.shape}, want {self.low.shape}")
        seen = np.isfinite(e)
        low = seen & (e < cfg.collapse_nats)
        self.low[seen & ~low] = 0
        self.low[low] += 1
        fire = self.low >= cfg.collapse_iters
        self.low[fire] = 0
        self.boosts[fire & (self.boosts < cfg.collapse_max)] += 1
        self.lifted[fire] = True
        return np.flatnonzero(fire)

    def to_dict(self) -> dict[str, Any]:
        return {
            "low": self.low.tolist(),
            "boosts": self.boosts.tolist(),
            "lifted": self.lifted.tolist(),
        }

    @classmethod
    def from_dict(cls, d: Mapping[str, Any]) -> EntropyCollapse:
        c = cls(len(d["low"]))
        c.low[:] = d["low"]
        c.boosts[:] = d["boosts"]
        c.lifted[:] = d["lifted"]
        return c


def schedule_for(
    clocks: Clocks,
    t_wall: float,
    m: float,
    collapse: EntropyCollapse,
    cfg: ScheduleCfg = DEFAULT,
    teacher_types: np.ndarray | None = None,
) -> IterSchedule:
    """The schedule of the update with index ``clocks.it`` at ``clocks.decisions``.

    ``teacher_types[d]`` says whether decision type d has a teacher; a
    teacherless type gets λ_KS = 0, so its entropy gate is open from the start.
    Default: every type has one.
    """
    n_types = len(collapse.low)
    has_t = np.ones(n_types, bool) if teacher_types is None else np.asarray(teacher_types, bool)
    if has_t.shape != (n_types,):
        raise ValueError(f"teacher_types has shape {has_t.shape}, want ({n_types},)")
    d, it = int(clocks.decisions), int(clocks.it)
    hold, off = bool(clocks.ks_hold), int(clocks.ks_offset)
    ph = phase(it, d, cfg, hold=hold, offset=off)
    lam = lam_ks(d, cfg, hold=hold, offset=off)
    ks_types = np.where(has_t, lam, 0.0)
    # The gate is open once a type's λ_KS is at most ent_gate. The tolerance
    # (0.4 decisions) absorbs float rounding exactly at 374M.
    gate_open = (ks_types <= cfg.ent_gate + 1e-9) | collapse.lifted
    ent_gain = np.where(gate_open, collapse.gain(cfg), 0.0)
    a, w = anneal(t_wall, cfg), warmup(it, cfg)
    held = lr_hold(d, cfg)
    return IterSchedule(
        it=it,
        decisions=d,
        ks_decisions=d - off,
        t_wall=float(t_wall),
        phase=ph,
        policy_on=policy_on(it, cfg),
        kl_control=kl_control_on(ph),
        kl_m=float(m),
        anneal=a,
        warmup=w,
        lr_frac=0.0 if held else float(m) * a * w,
        lr_hold=held,
        lr_base_trunk=cfg.lr_trunk,
        lr_base_value=cfg.lr_value,
        lr_base_input=cfg.lr_input,
        lam_ks=lam,
        ks_hold=hold,
        ks_offset=off,
        ks_passed=bool(clocks.ks_passed),
        beta_act=beta_act(d, cfg),
        beta_combat=entropy_beta(d, True, cfg, offset=off),
        beta_ooc=entropy_beta(d, False, cfg, offset=off),
        ks_types=ks_types,
        ent_gain=ent_gain,
        floor_coef=floor_coef_at(d, cfg),
        floor_p=float(cfg.floor_p),
        floor_on=floor_mask(cfg.floor_types, n_types),
        floor_share=float(cfg.floor_share),
        floor_start=int(cfg.floor_start),
        floor_ramp=int(cfg.floor_ramp),
    )


def floor_coef_at(decisions: int, cfg: ScheduleCfg = DEFAULT) -> float:
    """The floor's coefficient at `decisions`: 0 before floor_start, then linear to
    floor_coef over floor_ramp decisions (a step when floor_ramp is 0)."""
    if cfg.floor_coef <= 0.0:
        return 0.0
    if cfg.floor_ramp > 0 and cfg.floor_start <= 0:
        # A resumed lane is far past decision 0: a ramp from 0 would be a step.
        raise ValueError(
            "floor_coef > 0 with a floor_ramp needs floor_start: the decision count the "
            "ramp starts at (the lane's current one; 1 for a fresh lane)"
        )
    if decisions < cfg.floor_start:
        return 0.0
    if cfg.floor_ramp <= 0:
        return float(cfg.floor_coef)
    return float(cfg.floor_coef) * _clamp01((decisions - cfg.floor_start) / cfg.floor_ramp)


def floor_mask(names: Iterable[str], n_types: int) -> np.ndarray:
    """Bool per decision type: in `names` (`contracts.DT` names; an unknown one raises)."""
    from .contracts import DT

    out = np.zeros(n_types, bool)
    for name in names:
        if name not in DT:
            raise ValueError(f"floor_types: unknown decision type {name!r}")
        out[DT[name]] = True
    return out


# ── the ops watchdog's closed-form lr check ──────────────────────────────

#: The stats-row keys `check_lr_row` reads (`IterSchedule.log_fields` writes them).
LR_LOG_KEYS = (
    "it",
    "t_wall",
    "kl_m",
    "anneal",
    "warmup",
    "lr_frac",
    "lr_base_trunk",
    "lr_base_value",
    "lr_trunk",
    "lr_value",
)


def check_lr_row(
    row: Mapping[str, Any], cfg: ScheduleCfg = DEFAULT, tol: float = 1e-6
) -> list[str]:
    """Problems with one logged stats row's lr; an empty list means it is on its closed form.

    Recomputes anneal(t_wall), warmup(it) and lr_frac = kl_m · anneal · warmup.
    Fractions must match within ``tol``; each group's lr must equal
    ``base · lr_frac`` within ``tol · base``, with the base taken from ``cfg``
    (`ScheduleCfg.base_lrs`: "input" only with lr_input set, and then its keys must
    be in the row; a base of 0 must log exactly 0). m must lie in [kl_m_min, kl_m_max].
    """
    groups = cfg.base_lrs()
    need = LR_LOG_KEYS + (("lr_base_input", "lr_input") if "input" in groups else ())
    missing = [k for k in need if k not in row]
    if missing:
        return [f"missing keys {missing}"]
    errs: list[str] = []
    a = anneal(float(row["t_wall"]), cfg)
    w = warmup(int(row["it"]), cfg)
    m = float(row["kl_m"])
    held = lr_hold(int(row.get("decisions", cfg.lr_hold_decisions)), cfg)
    frac = 0.0 if held else m * a * w
    for key, want in (("anneal", a), ("warmup", w), ("lr_frac", frac)):
        got = float(row[key])
        if not abs(got - want) <= tol:
            errs.append(f"{key} {got!r} != closed form {want!r}")
    for g, base in groups.items():
        got_base = float(row[f"lr_base_{g}"])
        if not abs(got_base - base) <= tol * base:
            errs.append(f"lr_base_{g} {got_base!r} != cfg {base!r}")
        got = float(row[f"lr_{g}"])
        if not abs(got - base * frac) <= tol * base:
            errs.append(f"lr_{g} {got!r} != closed form {base * frac!r}")
    if not (cfg.kl_m_min - tol <= m <= cfg.kl_m_max + tol):
        errs.append(f"kl_m {m!r} outside [{cfg.kl_m_min}, {cfg.kl_m_max}]")
    return errs


class LrWatch:
    """`check_lr_row` over consecutive stats rows, plus the KL-factor freeze.

    m may change only after an iteration whose t_wall precedes kl_freeze_at, so
    a row that follows one at or after the freeze must repeat its kl_m.
    """

    def __init__(self, cfg: ScheduleCfg = DEFAULT, tol: float = 1e-6) -> None:
        self.cfg, self.tol = cfg, tol
        self.prev: Mapping[str, Any] | None = None

    def check(self, row: Mapping[str, Any]) -> list[str]:
        errs = check_lr_row(row, self.cfg, self.tol)
        p = self.prev
        if (
            not errs
            and p is not None
            and int(row["it"]) == int(p["it"]) + 1
            and float(p["t_wall"]) >= self.cfg.kl_freeze_at
            and abs(float(row["kl_m"]) - float(p["kl_m"])) > self.tol
        ):
            errs.append(f"kl_m changed {p['kl_m']!r} -> {row['kl_m']!r} after the freeze")
        self.prev = row
        return errs

    def check_all(self, rows: Iterable[Mapping[str, Any]]) -> list[tuple[int, list[str]]]:
        """(row index, problems) for every row with a problem."""
        out = []
        for i, r in enumerate(rows):
            e = self.check(r)
            if e:
                out.append((i, e))
        return out
