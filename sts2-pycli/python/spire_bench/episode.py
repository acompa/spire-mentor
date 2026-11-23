"""One episode: its worker, sequence numbers, deadlines, outcome and transcript.

Rules (``spire-bench/PROTOCOL.md``, "Gateway"):

* Every observation carries ``seq``. An action names the ``seq`` it answers.
  Resending the last applied action returns the same reply; any other stale
  ``seq`` is refused. There is no rewind.
* An action the host refuses leaves the decision and ``seq`` unchanged. After
  ``max_invalid`` refusals in a row the episode is lost.
* No action within ``decision_deadline`` seconds, or no finish within
  ``run_deadline``, loses the episode.
* A host that fails to start, or a gateway restart, voids the episode: the
  referee failed and the agent could not have caused it.
* A ``referee_fault`` (a draw that bypassed the keyed generator) or a host
  that dies while applying the agent's action LOSES the episode
  (``referee_fault_during_action``, ``worker_error_during_action``). Voiding
  it would let an agent that found a crashing action wipe a run it is losing.
  The end row records the triggering command, so an operator can review it.

The transcript is append-only JSONL: a header, one line per command with the
sha256 of the reply, and an end line with the outcome and the fingerprint
(``spire_bench.chain``: the header's identity, every reply, and the trigger
of a host failure). The episode key is not in it; it is revealed
with the season key.
"""

from __future__ import annotations

import asyncio
import hashlib
import json
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from spire_bench.chain import add_reply, add_trigger, digest, new_chain
from spire_bench.protocol import PROTOCOL_VERSION
from spire_bench.worker import RefereeWorker, WorkerError

RUNNING = "running"
WON = "won"
LOST = "lost"
VOID = "void"


@dataclass
class Limits:
    decision_deadline: float = 600.0
    run_deadline: float = 6 * 3600.0
    max_invalid: int = 20


def _digest(reply: dict[str, Any]) -> str:
    return digest(reply)


@dataclass
class Episode:
    episode_id: str
    agent_id: str
    episode_no: int
    character: str
    ascension: int
    label: str
    limits: Limits
    transcript_path: Path
    season_id: str | None = None
    worker: RefereeWorker | None = None
    seq: int = 0
    observation: dict[str, Any] | None = None
    status: str = RUNNING
    reason: str | None = None
    started_at: float = field(default_factory=time.time)
    decided_at: float = field(default_factory=time.time)
    invalid_streak: int = 0
    lock: asyncio.Lock = field(default_factory=asyncio.Lock)
    _chain: Any = None
    _last: tuple[int, dict[str, Any]] | None = None

    # ── transcript ──────────────────────────────────────────────────────────
    def _write(self, row: dict[str, Any]) -> None:
        with open(self.transcript_path, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(row, sort_keys=True) + "\n")

    def _record(self, cmd: dict[str, Any], reply: dict[str, Any]) -> None:
        d = _digest(reply)
        add_reply(self._chain, d)
        self._write({"type": "command", "cmd": cmd, "reply_sha256": d, "t": time.time()})

    @property
    def fingerprint(self) -> str:
        return self._chain.hexdigest() if self._chain is not None else ""

    # ── lifecycle ───────────────────────────────────────────────────────────
    def open_transcript(self, game_version: str, engine: str) -> None:
        """Writes the header and seeds the reply chain with its identity.

        Called before the worker is spawned, so a start-up failure still
        leaves a transcript that says which episode failed.
        """
        header = {
            "type": "header",
            "protocol": PROTOCOL_VERSION,
            "episode_id": self.episode_id,
            "agent_id": self.agent_id,
            "episode_no": self.episode_no,
            "character": self.character,
            "ascension": self.ascension,
            "label": self.label,
            "game_version": game_version,
            "engine_hash": engine,
            "limits": {
                "decision_deadline": self.limits.decision_deadline,
                "run_deadline": self.limits.run_deadline,
                "max_invalid": self.limits.max_invalid,
            },
            "season_id": self.season_id,
            "t": time.time(),
        }
        self._chain = new_chain(header)
        self._write(header)

    async def start(self, worker: RefereeWorker, key_hex: str) -> None:
        self.worker = worker
        cmd = {
            "cmd": "start_run",
            "character": self.character,
            "ascension": self.ascension,
            "seed": self.label,
            "act1": "roll",
        }
        try:
            reply = await worker.send({**cmd, "rng_key": key_hex})
        except WorkerError as exc:
            await self.finish(VOID, "worker_error", detail=str(exc))
            return
        self._record(cmd, reply)
        await self._accept(reply, None)

    async def _accept(self, reply: dict[str, Any], trigger: dict[str, Any] | None) -> None:
        kind = reply.get("type")
        if kind != "decision":
            what = "referee_fault" if kind == "referee_fault" else "unexpected_reply"
            if trigger is None:
                await self.finish(VOID, what, detail=str(reply)[:500])
            else:
                await self.finish(
                    LOST, f"{what}_during_action", trigger=trigger, detail=str(reply)[:500]
                )
            return
        self.observation = reply
        self.decided_at = time.time()
        if reply.get("decision") == "game_over":
            await self.finish(WON if reply.get("victory") else LOST, "game_over")

    async def finish(
        self,
        status: str,
        reason: str,
        *,
        trigger: dict[str, Any] | None = None,
        detail: str | None = None,
        failure: str | None = None,
    ) -> None:
        if self.status != RUNNING:
            return
        self.status, self.reason = status, reason
        obs = self.observation or {}
        extra: dict[str, Any] = {}
        if trigger is not None:
            extra["trigger"] = trigger
        if detail is not None:
            extra["detail"] = detail
        if failure is not None:
            extra["failure"] = failure
        self._write(
            {
                **extra,
                "type": "end",
                "status": status,
                "reason": reason,
                "victory": status == WON,
                "act": obs.get("act"),
                "floor": obs.get("floor"),
                "seq": self.seq,
                "fingerprint": self.fingerprint,
                "world_errors": self.worker.world_errors if self.worker else 0,
                "t": time.time(),
            }
        )
        if self.worker is not None:
            await self.worker.close()

    # ── one action ──────────────────────────────────────────────────────────
    def view(self) -> dict[str, Any]:
        # `fingerprint` is the agent's receipt: the head of the reply chain the
        # transcript must reproduce. An agent that keeps the last one can check
        # its published transcript was not cut short or relabelled.
        out: dict[str, Any] = {
            "episode_id": self.episode_id,
            "seq": self.seq,
            "status": self.status,
            "fingerprint": self.fingerprint,
        }
        if self.observation is not None:
            out["observation"] = self.observation
        if self.status != RUNNING:
            out["outcome"] = {"status": self.status, "reason": self.reason}
        return out

    async def step(self, seq: int, action: str, args: dict[str, Any] | None) -> dict[str, Any]:
        async with self.lock:
            if self._last is not None and seq == self._last[0]:
                return self._last[1]  # idempotent resend of the last applied action
            if self.status != RUNNING:
                return {**self.view(), "error": {"code": "episode_over"}}
            if seq != self.seq:
                return {**self.view(), "error": {"code": "stale_seq"}}
            assert self.worker is not None
            cmd: dict[str, Any] = {"cmd": "action", "action": action}
            if args:
                cmd["args"] = args
            try:
                reply = await self.worker.send(cmd)
            except WorkerError as exc:
                # The trigger joins the chain, so the agent's last receipt
                # covers the action the referee blames.
                add_trigger(self._chain, cmd)
                await self.finish(
                    LOST,
                    "worker_error_during_action",
                    trigger=cmd,
                    detail=str(exc),
                    failure=exc.kind,
                )
                return self.view()
            self._record(cmd, reply)
            if reply.get("type") == "error":
                self.invalid_streak += 1
                if self.invalid_streak >= self.limits.max_invalid:
                    await self.finish(LOST, "invalid_actions")
                    return self.view()
                return {**self.view(), "error": {"code": reply.get("code", "invalid")}}
            self.invalid_streak = 0
            applied = self.seq
            self.seq += 1
            await self._accept(reply, cmd)
            out = self.view()
            self._last = (applied, out)
            return out

    async def check_deadlines(self, now: float) -> None:
        if self.status != RUNNING or self.lock.locked():
            return
        if now - self.decided_at > self.limits.decision_deadline:
            await self.finish(LOST, "decision_deadline")
        elif now - self.started_at > self.limits.run_deadline:
            await self.finish(LOST, "run_deadline")
