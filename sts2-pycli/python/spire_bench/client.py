"""The Spire Bench client SDK. Standard library only, so it can be copied out.

Batched, vector-env style::

    from spire_bench.client import SpireBench, VecEnv, random_action

    bench = SpireBench("http://127.0.0.1:8640", token="...")
    env = VecEnv(bench, character="Ironclad", ascension=10, n=8)
    obs = env.reset()
    while not env.all_done():
        obs = env.step([random_action(o) if o else None for o in obs])

Every observation lists its ``legal_actions``; :func:`random_action` picks one.

Keep :meth:`VecEnv.receipts` when the episodes end: each is the fingerprint the
published transcript of that episode must replay to (``spire_bench.verify``).
"""

from __future__ import annotations

import json
import random
import time
import urllib.error
import urllib.request
from typing import Any


class BenchError(RuntimeError):
    def __init__(self, status: int, detail: str) -> None:
        super().__init__(f"HTTP {status}: {detail}")
        self.status = status
        self.detail = detail


class SpireBench:
    """A thin client for the gateway's HTTP API."""

    def __init__(self, base_url: str, token: str, timeout: float = 300.0, retries: int = 3) -> None:
        self.base_url = base_url.rstrip("/")
        self.token = token
        self.timeout = timeout
        self.retries = retries

    def _call(self, method: str, path: str, body: Any = None) -> Any:
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(
            self.base_url + path,
            data=data,
            method=method,
            headers={"Authorization": f"Bearer {self.token}", "Content-Type": "application/json"},
        )
        for attempt in range(self.retries + 1):
            try:
                with urllib.request.urlopen(req, timeout=self.timeout) as resp:
                    return json.loads(resp.read())
            except urllib.error.HTTPError as exc:
                detail = exc.read().decode("utf-8", "replace")
                raise BenchError(exc.code, detail) from None
            except (urllib.error.URLError, TimeoutError, ConnectionError):
                # Resending is safe: a step names the seq it answers, and the
                # gateway returns the same reply for a repeated one.
                if attempt == self.retries or path == "/v0/episodes":
                    raise
                time.sleep(1.0 + attempt)
        raise AssertionError("unreachable")

    def info(self) -> dict[str, Any]:
        return self._call("GET", "/v0/info")

    def start(self, character: str, ascension: int, count: int = 1) -> list[dict[str, Any]]:
        body = {"character": character, "ascension": ascension, "count": count}
        return self._call("POST", "/v0/episodes", body)["episodes"]

    def step(self, actions: list[dict[str, Any]]) -> list[dict[str, Any]]:
        return self._call("POST", "/v0/step", {"actions": actions})["results"]

    def episode(self, episode_id: str) -> dict[str, Any]:
        return self._call("GET", f"/v0/episodes/{episode_id}")

    def scores(self) -> list[dict[str, Any]]:
        return self._call("GET", "/v0/scores")["scores"]


class VecEnv:
    """``n`` episodes stepped together; one HTTP request per step."""

    def __init__(self, bench: SpireBench, character: str, ascension: int, n: int) -> None:
        self.bench, self.character, self.ascension, self.n = bench, character, ascension, n
        self.states: list[dict[str, Any]] = []

    def reset(self) -> list[dict[str, Any] | None]:
        self.states = self.bench.start(self.character, self.ascension, self.n)
        return self.observations()

    def observations(self) -> list[dict[str, Any] | None]:
        """The observation to act on per episode; None once it is over."""
        return [s.get("observation") if s.get("status") == "running" else None for s in self.states]

    def receipts(self) -> dict[str, str]:
        """Episode id -> the last fingerprint the gateway sent for it."""
        return {s["episode_id"]: s["fingerprint"] for s in self.states if "fingerprint" in s}

    def all_done(self) -> bool:
        return all(s.get("status") != "running" for s in self.states)

    def step(self, actions: list[dict[str, Any] | None]) -> list[dict[str, Any] | None]:
        """``actions[i]`` is ``{"action": ..., "args": ...}`` or None to skip."""
        batch, where = [], []
        for i, (state, act) in enumerate(zip(self.states, actions, strict=True)):
            if act is None or state.get("status") != "running":
                continue
            batch.append({"episode_id": state["episode_id"], "seq": state["seq"], **act})
            where.append(i)
        if batch:
            for i, result in zip(where, self.bench.step(batch), strict=True):
                # A refused action keeps the decision; the result says why.
                self.states[i] = {**self.states[i], **result}
        return self.observations()


def random_action(obs: dict[str, Any], rnd: random.Random | None = None) -> dict[str, Any]:
    """A uniformly random legal action. In combat it plays or drinks 90% of the time."""
    rnd = rnd or random
    legal = obs["legal_actions"]
    if obs.get("decision") == "combat_play":
        active = [a for a in legal if a["action"] in ("play_card", "use_potion")]
        if active and rnd.random() < 0.9:
            legal = active
        else:
            legal = [a for a in legal if a["action"] == "end_turn"] or legal
    pick = rnd.choice(legal)
    act: dict[str, Any] = {"action": pick["action"]}
    if "choose" in pick:
        spec = pick["choose"]
        count = rnd.randint(spec["min"], spec["max"])
        act["args"] = {"indices": ",".join(str(i) for i in rnd.sample(spec["from"], count))}
    elif "args" in pick:
        act["args"] = pick["args"]
    return act


def main() -> int:
    """A random bot: plays ``--episodes`` episodes and prints the outcomes."""
    import argparse

    ap = argparse.ArgumentParser(description="Spire Bench random bot.")
    ap.add_argument("--url", default="http://127.0.0.1:8640")
    ap.add_argument("--token", required=True)
    ap.add_argument("--character", default="Ironclad")
    ap.add_argument("--ascension", type=int, default=0)
    ap.add_argument("--episodes", type=int, default=4)
    ap.add_argument("--seed", type=int, default=0)
    args = ap.parse_args()
    rnd = random.Random(args.seed)
    env = VecEnv(SpireBench(args.url, args.token), args.character, args.ascension, args.episodes)
    obs = env.reset()
    while not env.all_done():
        obs = env.step([random_action(o, rnd) if o else None for o in obs])
    for s in env.states:
        print(json.dumps({"episode_id": s["episode_id"], **s.get("outcome", {})}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
