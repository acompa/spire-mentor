"""The Spire Bench gateway: HTTP in front of a pool of referee workers.

Run it::

    just bench-serve bench.json 8640   # from sts2-pycli/

``bench.json`` (a season server; a dev server gives ``"season_key": "<hex>"``
instead of ``season``, and its limits in the file)::

    {
      "season": {"dir": "season/", "drand_randomness": "<hex>"},
      "agents": {"<bearer token>": "<agent id>"},
      "state_dir": "bench-state",
      "max_workers": 8
    }

A season server takes its limits from the commitment and refuses to start
when ``K`` does not match it, or when its engine is not the committed one.

Endpoints (``spire-bench/PROTOCOL.md``, "Gateway"): ``GET /v0/info``,
``POST /v0/episodes``, ``POST /v0/step``, ``GET /v0/episodes/{id}``,
``GET /v0/scores``. Every request but ``/v0/info`` and ``/v0/scores`` carries
``Authorization: Bearer <token>``.
"""

from __future__ import annotations

import argparse
import asyncio
import contextlib
import hmac
import json
import time
from collections.abc import AsyncIterator
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import sts2_client
from fastapi import FastAPI, Header, HTTPException
from pydantic import BaseModel, Field

from spire_bench.episode import RUNNING, VOID, Episode, Limits
from spire_bench.keys import episode_key, episode_label
from spire_bench.protocol import PROTOCOL_VERSION
from spire_bench.scoring import score
from spire_bench.season import SeasonError, open_season
from spire_bench.worker import RefereeWorker, WorkerError, engine_hash

CHARACTERS = ("Ironclad", "Silent", "Defect", "Regent", "Necrobinder")
MAX_ASCENSION = 10


@dataclass
class Config:
    season_key: bytes
    agents: dict[str, str]
    state_dir: Path
    max_workers: int = 8
    max_batch: int = 256
    limits: Limits = field(default_factory=Limits)
    reaper_interval: float = 1.0
    # sha256 of the season's commitment; None for a dev server keyed directly.
    season_id: str | None = None

    @classmethod
    def load(cls, path: str | Path) -> Config:
        raw = json.loads(Path(path).read_text(encoding="utf-8"))
        if "season" in raw:
            return cls._load_season(raw)
        return cls(
            season_key=bytes.fromhex(raw["season_key"]),
            agents=dict(raw["agents"]),
            state_dir=Path(raw.get("state_dir", "bench-state")),
            max_workers=int(raw.get("max_workers", 8)),
            max_batch=int(raw.get("max_batch", 256)),
            limits=Limits(
                decision_deadline=float(raw.get("decision_deadline", 600)),
                run_deadline=float(raw.get("run_deadline", 6 * 3600)),
                max_invalid=int(raw.get("max_invalid", 20)),
            ),
        )

    @classmethod
    def _load_season(cls, raw: dict[str, Any]) -> Config:
        """A season server: the key comes from the commitment's ``K`` and the
        drand round's randomness, and the limits are the committed ones.

        ``"season": {"dir": "season/", "drand_randomness": "<hex>"}``; the dir
        holds ``commitment.json`` and ``K.secret`` (``spire_bench.season``).
        """
        season = raw["season"]
        d = Path(season["dir"])
        commitment = json.loads((d / "commitment.json").read_text(encoding="utf-8"))
        k = bytes.fromhex((d / "K.secret").read_text(encoding="utf-8").strip())
        key, sid = open_season(commitment, k, bytes.fromhex(season["drand_randomness"]))
        if commitment["game_version"] != sts2_client.GAME_VERSION:
            raise SeasonError(
                f"the season commits to {commitment['game_version']}, "
                f"this server runs {sts2_client.GAME_VERSION}"
            )
        if commitment["engine_hash"] != engine_hash(commitment["game_version"]):
            raise SeasonError("this server's engine is not the committed one")
        lim = commitment["limits"]
        return cls(
            season_key=key,
            agents=dict(raw["agents"]),
            state_dir=Path(raw.get("state_dir", "bench-state")),
            max_workers=int(raw.get("max_workers", 8)),
            max_batch=int(raw.get("max_batch", 256)),
            limits=Limits(
                decision_deadline=float(lim["decision_deadline"]),
                run_deadline=float(lim["run_deadline"]),
                max_invalid=int(lim["max_invalid"]),
            ),
            season_id=sid,
        )


class StartRequest(BaseModel):
    character: str
    ascension: int = Field(ge=0, le=MAX_ASCENSION)
    count: int = Field(default=1, ge=1, le=256)


class Action(BaseModel):
    episode_id: str
    seq: int
    action: str
    args: dict[str, Any] | None = None


class StepRequest(BaseModel):
    actions: list[Action]


class Bench:
    """The gateway's state: episodes, the episode index and the worker slots."""

    def __init__(self, config: Config) -> None:
        self.config = config
        self.game_version = sts2_client.GAME_VERSION
        self.engine = engine_hash(self.game_version)
        self.episodes: dict[str, Episode] = {}
        self.records: dict[str, dict[str, Any]] = {}
        self.next_no: dict[str, int] = {}
        self.transcripts = config.state_dir / "transcripts"
        self.transcripts.mkdir(parents=True, exist_ok=True)
        self.index = config.state_dir / "episodes.jsonl"
        self._load_index()

    # ── the episode index ───────────────────────────────────────────────────
    def _append_index(self, row: dict[str, Any]) -> None:
        with open(self.index, "a", encoding="utf-8") as fh:
            fh.write(json.dumps(row, sort_keys=True) + "\n")

    def _load_index(self) -> None:
        if self.index.exists():
            with open(self.index, encoding="utf-8") as fh:
                for line in fh:
                    row = json.loads(line)
                    if row["type"] == "start":
                        rec = {
                            k: row[k] for k in ("episode_id", "agent_id", "character", "ascension")
                        }
                        self.records[row["episode_id"]] = {**rec, "status": RUNNING}
                        no = int(row["episode_no"])
                        self.next_no[row["agent_id"]] = max(
                            self.next_no.get(row["agent_id"], 0), no + 1
                        )
                    elif row["type"] == "end":
                        self.records[row["episode_id"]]["status"] = row["status"]
        # Episodes a previous server left running died with it: the referee
        # failed, not the agent. One that wrote its transcript's end row before
        # the index caught up keeps that outcome.
        for eid, rec in self.records.items():
            if rec["status"] != RUNNING:
                continue
            path = self.transcripts / f"{eid}.jsonl"
            last: dict[str, Any] = {}
            if path.exists():
                lines = path.read_text(encoding="utf-8").splitlines()
                last = json.loads(lines[-1]) if lines else {}
            if last.get("type") == "end":
                rec["status"] = last["status"]
                reason = last["reason"]
            else:
                rec["status"], reason = VOID, "server_restart"
                with open(path, "a", encoding="utf-8") as fh:
                    end = {"type": "end", "status": VOID, "reason": reason, "t": time.time()}
                    fh.write(json.dumps(end, sort_keys=True) + "\n")
            self._append_index(
                {"type": "end", "episode_id": eid, "status": rec["status"], "reason": reason}
            )

    def agent_for(self, authorization: str | None) -> str:
        token = (authorization or "").removeprefix("Bearer ").strip()
        for known, agent in self.config.agents.items():
            if token and hmac.compare_digest(token.encode(), known.encode()):
                return agent
        raise HTTPException(401, "unknown token")

    def running(self) -> int:
        return sum(1 for ep in self.episodes.values() if ep.status == RUNNING)

    async def _end(self, ep: Episode) -> None:
        self.records[ep.episode_id]["status"] = ep.status
        self._append_index(
            {"type": "end", "episode_id": ep.episode_id, "status": ep.status, "reason": ep.reason}
        )
        # Keep finished episodes queryable, but drop the worker handle.
        ep.worker = None

    async def _settle(self, ep: Episode) -> None:
        if ep.status != RUNNING and self.records[ep.episode_id]["status"] == RUNNING:
            await self._end(ep)

    # ── requests ────────────────────────────────────────────────────────────
    async def start(self, agent: str, req: StartRequest) -> list[dict[str, Any]]:
        if req.character not in CHARACTERS:
            raise HTTPException(400, f"character must be one of {', '.join(CHARACTERS)}")
        if self.running() + req.count > self.config.max_workers:
            raise HTTPException(
                429, f"{self.config.max_workers - self.running()} worker slots free"
            )
        eps = []
        for _ in range(req.count):
            no = self.next_no.get(agent, 0)
            self.next_no[agent] = no + 1
            label = episode_label(agent, no)
            ep = Episode(
                episode_id=label,
                agent_id=agent,
                episode_no=no,
                character=req.character,
                ascension=req.ascension,
                label=label,
                limits=self.config.limits,
                transcript_path=self.transcripts / f"{label}.jsonl",
                season_id=self.config.season_id,
            )
            self.episodes[label] = ep
            self.records[label] = {
                "episode_id": label,
                "agent_id": agent,
                "character": req.character,
                "ascension": req.ascension,
                "status": RUNNING,
            }
            self._append_index(
                {
                    "type": "start",
                    "episode_id": label,
                    "agent_id": agent,
                    "episode_no": no,
                    "character": req.character,
                    "ascension": req.ascension,
                    "t": time.time(),
                }
            )
            eps.append(ep)

        async def boot(ep: Episode) -> dict[str, Any]:
            ep.open_transcript(self.game_version, self.engine)
            try:
                worker = await RefereeWorker.spawn()
            except WorkerError as exc:
                await ep.finish(VOID, "worker_error", detail=str(exc))
            else:
                key = episode_key(self.config.season_key, agent, ep.episode_no)
                await ep.start(worker, key)
            await self._settle(ep)
            return ep.view()

        return list(await asyncio.gather(*(boot(ep) for ep in eps)))

    def owned(self, agent: str, episode_id: str) -> Episode | None:
        ep = self.episodes.get(episode_id)
        return ep if ep is not None and ep.agent_id == agent else None

    async def step(self, agent: str, req: StepRequest) -> list[dict[str, Any]]:
        if len(req.actions) > self.config.max_batch:
            raise HTTPException(400, f"at most {self.config.max_batch} actions per batch")
        seen: set[str] = set()

        async def one(a: Action) -> dict[str, Any]:
            if a.episode_id in seen:
                return {"episode_id": a.episode_id, "error": {"code": "duplicate_in_batch"}}
            seen.add(a.episode_id)
            ep = self.owned(agent, a.episode_id)
            if ep is None:
                return {"episode_id": a.episode_id, "error": {"code": "unknown_episode"}}
            out = await ep.step(a.seq, a.action, a.args)
            await self._settle(ep)
            return out

        return list(await asyncio.gather(*(one(a) for a in req.actions)))

    async def reap(self) -> None:
        now = time.time()
        for ep in list(self.episodes.values()):
            await ep.check_deadlines(now)
            await self._settle(ep)

    async def close(self) -> None:
        for ep in self.episodes.values():
            if ep.status == RUNNING:
                await ep.finish(VOID, "server_shutdown")
                await self._settle(ep)


def create_app(config: Config) -> FastAPI:
    bench = Bench(config)

    @contextlib.asynccontextmanager
    async def lifespan(_: FastAPI) -> AsyncIterator[None]:
        async def reaper() -> None:
            while True:
                await asyncio.sleep(config.reaper_interval)
                await bench.reap()

        task = asyncio.create_task(reaper())
        try:
            yield
        finally:
            task.cancel()
            await bench.close()

    app = FastAPI(title="Spire Bench gateway", version=PROTOCOL_VERSION, lifespan=lifespan)
    app.state.bench = bench

    @app.get("/v0/info")
    async def info() -> dict[str, Any]:
        return {
            "protocol": PROTOCOL_VERSION,
            "season_id": config.season_id,
            "game_version": bench.game_version,
            "engine_hash": bench.engine,
            "characters": list(CHARACTERS),
            "max_ascension": MAX_ASCENSION,
            "max_workers": config.max_workers,
            "max_batch": config.max_batch,
            "decision_deadline": config.limits.decision_deadline,
            "run_deadline": config.limits.run_deadline,
            "max_invalid": config.limits.max_invalid,
        }

    @app.post("/v0/episodes")
    async def start(req: StartRequest, authorization: str | None = Header(None)) -> dict[str, Any]:
        agent = bench.agent_for(authorization)
        return {"episodes": await bench.start(agent, req)}

    @app.post("/v0/step")
    async def step(req: StepRequest, authorization: str | None = Header(None)) -> dict[str, Any]:
        agent = bench.agent_for(authorization)
        return {"results": await bench.step(agent, req)}

    @app.get("/v0/episodes/{episode_id}")
    async def episode(episode_id: str, authorization: str | None = Header(None)) -> dict[str, Any]:
        agent = bench.agent_for(authorization)
        ep = bench.owned(agent, episode_id)
        if ep is None:
            raise HTTPException(404, "unknown episode")
        return ep.view()

    @app.get("/v0/scores")
    async def scores() -> dict[str, Any]:
        return {"scores": score(bench.records.values())}

    return app


def main() -> int:
    import uvicorn

    ap = argparse.ArgumentParser(description="Spire Bench gateway.")
    ap.add_argument("--config", required=True)
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=8640)
    args = ap.parse_args()
    uvicorn.run(create_app(Config.load(args.config)), host=args.host, port=args.port)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
