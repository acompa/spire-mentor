"""Our search agent (PPO run policy + MCTS fights) through the Spire Bench SDK.

    uv run python python/bench_agents/search_agent.py --url http://127.0.0.1:8640 \\
        --token <token> --character Ironclad --ascension 10 --episodes 4

``--instinct`` plays Instinct (the e2e student, no search) instead
(``bench_agents.instinct_policy``); the search and PPO answer only where it
cannot, and each such decision counts as a fallback.

Each episode is ``headless_eval.play_one_headless_session`` against a
:class:`RefereeGame`: the same decision stack as the headless rung, fed only
the referee's observation (``bench_agents.referee_game``).
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import random
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from headless_eval import play_one_headless_session
from realgame_mcts import DEFAULT_SIMS_BOSS, DEFAULT_SIMS_REGULAR
from realgame_ppo import PpoPolicyCache
from spire_bench.client import SpireBench
from sts2_hint.recommender import RecommendConfig

from bench_agents.instinct_policy import InstinctPolicy
from bench_agents.keyed_game import KeyedLocalGame
from bench_agents.referee_game import RefereeGame


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--url", default="http://127.0.0.1:8640")
    ap.add_argument("--token", default="")
    ap.add_argument("--character", default="Ironclad")
    ap.add_argument("--ascension", type=int, default=0)
    ap.add_argument("--episodes", type=int, default=1)
    ap.add_argument("--sims-regular", type=int, default=DEFAULT_SIMS_REGULAR)
    ap.add_argument("--sims-boss", type=int, default=DEFAULT_SIMS_BOSS)
    ap.add_argument("--model", default=None)
    ap.add_argument("--max-run-seconds", type=float, default=3600.0)
    ap.add_argument("--out", type=Path, default=None, help="JSONL of outcomes and receipts")
    ap.add_argument(
        "--local",
        action="store_true",
        help="the control arm: the same agent on a plain headless host, which "
        "shows the full state (world RNG, enemy move state, draw order)",
    )
    ap.add_argument("--seed-base", type=int, default=0, help="--local seeds")
    ap.add_argument(
        "--paired-season-key",
        default=None,
        help="--local only: play the referee episodes' dice (a dev season's key, hex) "
        "on a keyed host with the full view, episode --seed-base + i of --paired-agent",
    )
    ap.add_argument("--paired-agent", default=None, help="the referee arm's agent id")
    ap.add_argument("--instinct", action="store_true", help="play Instinct, not the search")
    args = ap.parse_args(argv)
    if args.paired_season_key and not (args.local and args.paired_agent):
        ap.error("--paired-season-key needs --local and --paired-agent")
    logging.basicConfig(level=logging.INFO, format="%(levelname)s %(name)s: %(message)s")
    # The referee keeps its dice: the mirror cannot carry the world's RNG, so a
    # random roll it predicts differently is expected, not a bug. OBSERVE
    # re-seeds the fight from the observation instead of aborting the run.
    os.environ.setdefault("STS2_MIRROR_OBSERVE", "1")
    # The plain host would play the Crystal Sphere itself, by its own lattice;
    # interactive, the digger plays it in both arms.
    os.environ.setdefault("STS2_CRYSTAL_SPHERE_INTERACTIVE", "1")

    mcts_cfg = RecommendConfig(
        character=args.character, hidden_info="beliefdraw", c_puct=2.5, draw_widening_c=2.0
    )
    ppo = PpoPolicyCache(args.model)
    bench = None if args.local else SpireBench(args.url, args.token)
    version = os.environ.get("STS2_GAME_VERSION", "v0.111.0")
    instinct = InstinctPolicy(args.character, args.ascension, version) if args.instinct else None
    for i in range(args.episodes):
        seed = args.seed_base + i
        if bench is not None:
            game: Any = RefereeGame(bench)
        elif args.paired_season_key:
            game = KeyedLocalGame(
                bytes.fromhex(args.paired_season_key), args.paired_agent, args.seed_base + i
            )
        else:
            game = None
        if instinct is not None:
            instinct.new_run()
        outcome = play_one_headless_session(
            random.Random(seed),
            run_index=i,
            character=args.character,
            seed=seed,
            mcts_cfg=mcts_cfg,
            ppo_policies=ppo,
            sims_regular=args.sims_regular,
            sims_boss=args.sims_boss,
            max_run_seconds=args.max_run_seconds,
            ascension=args.ascension,
            game=game,
            instinct=instinct,
        )
        row = {
            "arm": "referee" if bench is not None else "local",
            "agent": "instinct" if instinct is not None else "search",
            "episode_id": None if game is None else game.episode_id,
            "seed": seed if game is None else None,
            "status": getattr(game, "status", None),
            "result": outcome.result,
            "act": outcome.act,
            "floor": outcome.floor,
            "decisions": outcome.decisions,
            "fallbacks": outcome.fallback_count,
            "error": outcome.error,
            "receipt": getattr(game, "fingerprint", None),
            "mirror": {
                k: (outcome.mirror_stats or {}).get(k) for k in ("reconciles", "desyncs", "tainted")
            },
            "seconds": round(outcome.duration_s, 1),
            "instinct_misses": dict(instinct.unavailable) if instinct is not None else None,
        }
        print(json.dumps(row), flush=True)
        if args.out:
            with open(args.out, "a", encoding="utf-8") as fh:
                fh.write(json.dumps(row) + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
