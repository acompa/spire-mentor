"""The paired control arm: a plain host playing a referee episode's dice.

A host started with ``STS2_KEYED_RNG=1`` draws every random number from the
episode key, as the referee does, but shows the full state (enemy move state,
draw order, encounter ids, the second boss) and takes every command. Given
the referee episode's key (a dev season's key is known) and its label, it
plays the same game the referee arm plays for the same actions, so the two
arms meet the same dice episode for episode.

Screens the plain host settles itself are answered as the referee arm
answers them (``referee_game.host_answer``).
"""

from __future__ import annotations

import json
import os
from collections.abc import Callable
from typing import Any

from spire_bench.keys import episode_key, episode_label
from sts2_client import Game

from bench_agents.referee_game import host_answer


class KeyedLocalGame:
    def __init__(self, season_key: bytes, agent_id: str, episode_no: int) -> None:
        os.environ["STS2_KEYED_RNG"] = "1"
        self.game = Game()
        self.proc = self.game.proc
        self.label = episode_label(agent_id, episode_no)
        self.key = episode_key(season_key, agent_id, episode_no)
        self.episode_id = self.label
        self._seq = 0
        self._refused: set[tuple[int, str]] = set()
        # Called with each screen the bench answers itself (GH #1704).
        self.on_auto: Callable[[dict[str, Any]], None] | None = None

    def start(self, character: str, *, seed: str = "", ascension: int = 0, **_: Any) -> dict:
        """Starts the episode's twin; ``seed`` is ignored (the label is used)."""
        state = self.game.start(
            character,
            seed=self.label,
            ascension=ascension,
            act1="roll",
            interactive_potion_swap=True,
            rng_key=self.key,
        )
        return self._settle(state)

    def send(self, cmd: dict[str, Any]) -> dict[str, Any]:
        state = self.game.send(cmd)
        if state.get("type") == "error":
            return state
        return self._settle(state)

    def act(self, action: str, **args: Any) -> dict[str, Any]:
        cmd: dict[str, Any] = {"cmd": "action", "action": action}
        if args := {k: v for k, v in args.items() if v is not None}:
            cmd["args"] = args
        return self.send(cmd)

    def get_map(self) -> dict[str, Any]:
        return self.game.get_map()

    def world_errors(self) -> dict[str, Any]:
        return self.game.world_errors()

    def close(self) -> None:
        self.game.close()

    def _settle(self, state: dict[str, Any]) -> dict[str, Any]:
        for _ in range(200):
            self._seq += 1
            auto = host_answer(state, self._seq, self._refused)
            if auto is None:
                return state
            if self.on_auto is not None:
                self.on_auto(state)
            nxt = self.game.send({"cmd": "action", **auto})
            if nxt.get("type") == "error":
                self._refused.add((self._seq, json.dumps(auto, sort_keys=True)))
                self._seq -= 1
                continue
            state = nxt
        return state
