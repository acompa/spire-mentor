"""The pieces of stop 4 that the check and its tests share.

`trace_run` plays one run greedily with the checkpoint's policy head and
records the critic's value at every stored decision. `hand_gae` and
`hand_run_chain` are the advantage arithmetic written out as plain loops, so
they can be compared with the repository's `ppo.e2e.algo.compute_returns`.
"""

from __future__ import annotations

import os

# The training processes select the lane build before the engine loads
# (plan section 8); the tokenizer reads these at import.
os.environ.setdefault("STS2_GAME_VERSION", "v0.111.0")
os.environ.setdefault("STS2_RUNAI_POTION_SWAP", "1")
os.environ.setdefault("STS2_PPO_FEATURES", "card_mods,reward_upgrade")

from dataclasses import dataclass  # noqa: E402

import numpy as np  # noqa: E402

ROOM_NAMES = (
    "none", "unassigned", "unknown", "monster", "elite", "boss",
    "shop", "rest_site", "treasure", "ancient", "event",
)  # fmt: skip


# ── one traced run ────────────────────────────────────────────────────────


@dataclass
class Trace:
    """One run, one entry per stored decision (a "row"), in order.

    `value` is what training uses for the row: `V_combat` on combat rows and
    `V_run` on run rows. `reward[t]` is all reward from row t up to row t+1
    (or to the end), the repository's ``reward`` column.
    """

    seed: int
    won: bool
    outcome: str
    is_combat: np.ndarray  # bool
    fight: np.ndarray  # int: fight index for combat rows, -1 for run rows
    room: np.ndarray  # int: index into ROOM_NAMES (the room the row is in)
    hp_frac: np.ndarray  # float: hp / max_hp at the row
    act: np.ndarray
    floor: np.ndarray
    reward: np.ndarray  # float64
    value: np.ndarray  # float64
    v_combat: np.ndarray
    v_run: np.ndarray

    def __len__(self) -> int:
        return len(self.value)

    def returns_to_go(self) -> np.ndarray:
        """G_t: the reward actually collected from row t to the end of the run."""
        return np.cumsum(self.reward[::-1])[::-1]

    def table(self) -> dict[str, np.ndarray]:
        """This run as the table `compute_returns` reads: one run, ended."""
        n = len(self)
        done = np.zeros(n, bool)
        done[-1] = True
        return {
            "run_uid": np.zeros(n, np.int64),
            "t": np.arange(n, dtype=np.int64),
            "is_combat": self.is_combat.copy(),
            "reward": self.reward.copy(),
            "done": done,
            "bootstrap": np.zeros(n, bool),
            "actor": np.zeros(n, np.int64),
        }


_NET = None


def _net(ckpt: str):
    global _NET
    if _NET is None:
        import torch
        from ppo.e2e import net as N

        torch.set_num_threads(1)
        _NET, _ = N.load_checkpoint(ckpt)
        _NET.eval()
    return _NET


def trace_run(seed: int, ckpt: str) -> Trace:
    """Play `seed` greedily with the checkpoint and read both value heads at
    every decision.

    The rows come from the Rust training env (`RsE2EEnv`, what the rollout
    workers use) with ``priv_=True``, so each row carries the privileged
    (PRIV) tokens the critic was trained on. `E2ENet.forward` feeds them to
    the value module only.
    """
    import torch
    from ppo.e2e import contracts as C
    from ppo.e2e import net as N
    from ppo.e2e import tokens as TK
    from ppo.e2e.env import EnvConfig
    from ppo.e2e.rsenv import RsDecision, RsE2EEnv

    net = _net(ckpt)
    env = RsE2EEnv(seed, cfg=EnvConfig(f9_p=0.0), detail=True)
    x_room, x_hp = C.XC["X_ROOM"], C.XF["R_HP_FRAC"]
    rows: list[tuple] = []
    d = env.reset()
    with torch.no_grad():
        while isinstance(d, RsDecision):
            tk = d.tok
            out = net.forward(N.to_batch(TK.row_batch([tk], run_uid=[0], t=[d.row]), "cpu"))
            kind, idx = d.unit
            rows.append(
                (
                    d.is_combat,
                    idx if kind == "fight" else -1,
                    int(tk.cat[0, x_room]),
                    float(tk.cont[0, x_hp]),
                    int(d.act),
                    int(d.floor),
                    float(d.reward),  # reward since the previous row
                    float(out.value[0]),
                    float(out.v_combat[0]),
                    float(out.v_run[0]),
                )
            )
            d = env.step(int(out.logits[0].argmax()))
    term = d
    cols = list(zip(*rows))
    before = np.asarray(cols[6], np.float64)
    reward = np.append(before[1:], term.reward)
    reward[0] += before[0]
    return Trace(
        seed=seed,
        won=bool(term.won),
        outcome=str(term.outcome),
        is_combat=np.asarray(cols[0], bool),
        fight=np.asarray(cols[1], np.int64),
        room=np.asarray(cols[2], np.int64),
        hp_frac=np.asarray(cols[3], np.float64),
        act=np.asarray(cols[4], np.int64),
        floor=np.asarray(cols[5], np.int64),
        reward=reward,
        value=np.asarray(cols[7], np.float64),
        v_combat=np.asarray(cols[8], np.float64),
        v_run=np.asarray(cols[9], np.float64),
    )


# ── advantages by hand ────────────────────────────────────────────────────


def hand_gae(v: list[float], r: list[float], v_after: float, lam: float) -> list[float]:
    """GAE over one unbroken chain of rows, gamma = 1, written as a loop.

    `v_after` is the value of whatever follows the last row (0 at the end of
    the run). Surprise: delta_t = r_t + V_{t+1} - V_t. Advantage, from the
    back: A_t = delta_t + lam * A_{t+1}.
    """
    n = len(v)
    adv = [0.0] * n
    later = 0.0
    for t in range(n - 1, -1, -1):
        v_next = v[t + 1] if t + 1 < n else v_after
        delta = r[t] + v_next - v[t]
        later = delta + lam * later
        adv[t] = later
    return adv


def hand_two_chain(tr: Trace, lam_c: float, lam_r: float) -> np.ndarray:
    """Every row's advantage under the repository's two chains, as loops.

    Combat chain: delta = r_t + V(next row) - V_t, continued with lam_c only
    while the next row is a combat row. A fight's last row therefore stops
    at V of the next run row: the fight hands its result to the run.
    Run chain: run rows skip over fights. Between run row i and the next run
    row j, delta = (all reward from i up to j) + V_j - V_i, continued with lam_r.
    """
    v, r, comb = tr.value.tolist(), tr.reward.tolist(), tr.is_combat.tolist()
    n = len(v)
    adv = np.zeros(n)
    # Combat chain: each maximal block of consecutive combat rows.
    t = 0
    while t < n:
        if not comb[t]:
            t += 1
            continue
        end = t
        while end + 1 < n and comb[end + 1]:
            end += 1
        v_after = v[end + 1] if end + 1 < n else 0.0
        adv[t : end + 1] = hand_gae(v[t : end + 1], r[t : end + 1], v_after, lam_c)
        t = end + 1
    # Run chain: the run rows alone, each carrying the reward up to the next.
    runs = [i for i in range(n) if not comb[i]]
    seg_r = [sum(r[i : (runs[k + 1] if k + 1 < len(runs) else n)]) for k, i in enumerate(runs)]
    adv_run = hand_gae([v[i] for i in runs], seg_r, 0.0, lam_r)
    adv[runs] = adv_run
    return adv


def weight_of_later_surprise(k: int, lam: float) -> float:
    """The weight a surprise k rows later gets in this row's advantage, read
    off the repository's `compute_returns`: a combat chain of k + 1 rows with
    zero values everywhere and one unit of reward on the last row."""
    from ppo.e2e import algo

    n = k + 1
    r = np.zeros(n)
    r[-1] = 1.0
    done = np.zeros(n, bool)
    done[-1] = True
    tab = {
        "run_uid": np.zeros(n, np.int64),
        "t": np.arange(n),
        "is_combat": np.ones(n, bool),
        "reward": r,
        "done": done,
        "bootstrap": np.zeros(n, bool),
        "actor": np.zeros(n, np.int64),
    }
    return float(algo.compute_returns(tab, np.zeros(n), lam_c=lam, hold_iters=0).adv[0])


# ── fights ────────────────────────────────────────────────────────────────


@dataclass(frozen=True)
class Fight:
    seed: int
    room: str
    act: int
    floor: int
    hp_frac: float  # at the fight's first decision
    v_before: float  # V_run at the run row just before the fight
    v_after: float  # V_run at the run row just after it (0 if the run ended)
    reward: float  # reward collected from the row before up to the row after
    run_ended: bool

    @property
    def dv(self) -> float:
        return self.v_after - self.v_before

    @property
    def delta(self) -> float:
        """The run chain's one-step surprise across the fight."""
        return self.reward + self.v_after - self.v_before


def fights_of(tr: Trace) -> list[Fight]:
    """Each fight whose previous row is a run row (so act 3's second boss,
    which follows the first with no run row between, is left out)."""
    out: list[Fight] = []
    n = len(tr)
    t = 0
    while t < n:
        if not tr.is_combat[t]:
            t += 1
            continue
        start = t
        while t + 1 < n and tr.is_combat[t + 1] and tr.fight[t + 1] == tr.fight[start]:
            t += 1
        end = t
        t += 1
        if start == 0 or tr.is_combat[start - 1]:
            continue
        nxt = end + 1
        ended = nxt >= n
        if not ended and tr.is_combat[nxt]:
            continue  # chains straight into another fight (act 3's double boss)
        out.append(
            Fight(
                seed=tr.seed,
                room=ROOM_NAMES[tr.room[start]],
                act=int(tr.act[start]),
                floor=int(tr.floor[start]),
                hp_frac=float(tr.hp_frac[start]),
                v_before=float(tr.value[start - 1]),
                v_after=0.0 if ended else float(tr.value[nxt]),
                reward=float(tr.reward[start - 1 : nxt].sum()),
                run_ended=ended,
            )
        )
    return out
