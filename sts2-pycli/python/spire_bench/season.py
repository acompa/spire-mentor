"""Season commit-reveal: where the season key comes from.

Before a season the operator publishes a commitment::

    {"type": "spire-bench-commitment", "season": "s0",
     "k_sha256": sha256(K), "drand": {"chain": ..., "round": N},
     "game_version": ..., "engine_hash": ..., "limits": {...}}

``K`` is a secret 32 random bytes; the drand round is one still in the future.
Once the round is out, ``season_key = HMAC-SHA256(K, randomness of round N)``.
The operator cannot steer the key (``K`` is fixed before the beacon value
exists) and nobody else can compute it (``K`` is secret). After the season the
operator publishes the reveal (the commitment, ``K`` and the randomness) with
every transcript, and anyone can check it and replay the episodes.

``season_id`` (sha256 of the commitment) is written into every transcript
header and so into every agent's receipt: a transcript from another season, or
another commitment, does not verify against this one.

    just bench-season-commit s0 <drand chain hash> <round> season/   # from sts2-pycli/
    just bench-season-reveal season/ <randomness hex>
"""

from __future__ import annotations

import argparse
import hashlib
import hmac
import json
import os
import secrets
import time
import urllib.request
from pathlib import Path
from typing import Any

from spire_bench.chain import digest
from spire_bench.worker import engine_hash

COMMITMENT = "spire-bench-commitment"
REVEAL = "spire-bench-reveal"
# drand's public HTTP relay. Only the round number and chain hash are sent.
DRAND_API = "https://api.drand.sh"


class SeasonError(ValueError):
    """A commitment, secret or reveal that does not check out."""


def season_id(commitment: dict[str, Any]) -> str:
    return digest(commitment)


def season_key(k: bytes, randomness: bytes) -> bytes:
    if len(k) != 32:
        raise SeasonError("K must be 32 bytes")
    if len(randomness) != 32:
        raise SeasonError("drand randomness must be 32 bytes")
    return hmac.new(k, randomness, hashlib.sha256).digest()


def make_commitment(
    season: str,
    drand_chain: str,
    drand_round: int,
    game_version: str,
    limits: dict[str, Any],
    engine: str | None = None,
) -> tuple[dict[str, Any], bytes]:
    """A new commitment and its secret ``K``."""
    k = secrets.token_bytes(32)
    commitment = {
        "type": COMMITMENT,
        "season": season,
        "k_sha256": hashlib.sha256(k).hexdigest(),
        "drand": {"chain": drand_chain, "round": int(drand_round)},
        "game_version": game_version,
        "engine_hash": engine or engine_hash(game_version),
        "limits": limits,
        "created": int(time.time()),
    }
    return commitment, k


def open_season(commitment: dict[str, Any], k: bytes, randomness: bytes) -> tuple[bytes, str]:
    """``(season_key, season_id)``; raises when ``K`` is not the committed one."""
    if commitment.get("type") != COMMITMENT:
        raise SeasonError("not a commitment")
    if hashlib.sha256(k).hexdigest() != commitment.get("k_sha256"):
        raise SeasonError("K does not match the commitment")
    return season_key(k, randomness), season_id(commitment)


def episode_counts(state_dir: str | Path) -> dict[str, int]:
    """How many episodes each agent started, from the gateway's episode index."""
    nos: dict[str, set[int]] = {}
    with open(Path(state_dir) / "episodes.jsonl", encoding="utf-8") as fh:
        for line in fh:
            row = json.loads(line)
            if row["type"] == "start":
                nos.setdefault(row["agent_id"], set()).add(int(row["episode_no"]))
    for agent, got in nos.items():
        if got != set(range(len(got))):
            raise SeasonError(f"the index numbers {agent}'s episodes with a gap")
    return {agent: len(got) for agent, got in sorted(nos.items())}


def make_reveal(
    commitment: dict[str, Any], k: bytes, randomness: bytes, episodes: dict[str, int]
) -> dict[str, Any]:
    """The reveal: the commitment, ``K``, the randomness, and how many episodes
    each agent started. Each agent can hold that count against its receipts;
    the leaderboard requires exactly that many transcripts per agent.
    """
    open_season(commitment, k, randomness)
    return {
        "type": REVEAL,
        "commitment": commitment,
        "k": k.hex(),
        "drand_randomness": randomness.hex(),
        "episodes": dict(episodes),
    }


def check_reveal(reveal: dict[str, Any]) -> tuple[bytes, str, dict[str, Any]]:
    """``(season_key, season_id, commitment)`` from a published reveal.

    It checks ``K`` against the commitment. Whether ``drand_randomness`` really
    is the committed round's is the reader's check, against drand itself
    (``fetch_randomness``, or drand's own tooling for the signature).
    """
    if reveal.get("type") != REVEAL:
        raise SeasonError("not a reveal")
    commitment = reveal["commitment"]
    key, sid = open_season(
        commitment, bytes.fromhex(reveal["k"]), bytes.fromhex(reveal["drand_randomness"])
    )
    return key, sid, commitment


def fetch_randomness(chain: str, round_no: int, api: str = DRAND_API) -> bytes:
    """The randomness of one drand round, from the public relay.

    drand's randomness is sha256 of the round's signature; that is checked
    here. The BLS signature itself is checked with drand's tooling.
    """
    url = f"{api}/{chain}/public/{int(round_no)}"
    with urllib.request.urlopen(url, timeout=30) as resp:
        beacon = json.loads(resp.read().decode("utf-8"))
    if int(beacon["round"]) != int(round_no):
        raise SeasonError(f"drand answered round {beacon['round']}, not {round_no}")
    randomness = bytes.fromhex(beacon["randomness"])
    if hashlib.sha256(bytes.fromhex(beacon["signature"])).digest() != randomness:
        raise SeasonError("drand randomness is not sha256 of its signature")
    return randomness


def _write_json(path: Path, obj: dict[str, Any]) -> None:
    path.write_text(json.dumps(obj, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser(description="Spire Bench season commit-reveal.")
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("commit", help="new K and a commitment to publish")
    c.add_argument("season")
    c.add_argument("drand_chain")
    c.add_argument("drand_round", type=int)
    c.add_argument("out_dir")
    c.add_argument("--game-version", default=None)
    c.add_argument("--decision-deadline", type=float, default=600.0)
    c.add_argument("--run-deadline", type=float, default=6 * 3600.0)
    c.add_argument("--max-invalid", type=int, default=20)
    r = sub.add_parser("reveal", help="the reveal to publish after the season")
    r.add_argument("season_dir")
    r.add_argument("--state", required=True, help="the gateway's state_dir")
    r.add_argument("--randomness", help="hex; fetched from drand when omitted")
    k = sub.add_parser("check", help="check a published reveal")
    k.add_argument("reveal")
    k.add_argument("--fetch", action="store_true", help="also compare the randomness with drand")
    args = ap.parse_args()

    if args.cmd == "commit":
        import sts2_client

        out = Path(args.out_dir)
        out.mkdir(parents=True, exist_ok=True)
        if (out / "K.secret").exists():
            raise SystemExit(f"{out / 'K.secret'} exists; refusing to overwrite a season")
        limits = {
            "decision_deadline": args.decision_deadline,
            "run_deadline": args.run_deadline,
            "max_invalid": args.max_invalid,
        }
        version = args.game_version or sts2_client.GAME_VERSION
        commitment, k_bytes = make_commitment(
            args.season, args.drand_chain, args.drand_round, version, limits
        )
        fd = os.open(out / "K.secret", os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w", encoding="utf-8") as fh:
            fh.write(k_bytes.hex() + "\n")
        _write_json(out / "commitment.json", commitment)
        print(
            f"publish {out / 'commitment.json'} (season_id {season_id(commitment)}); keep K.secret"
        )
        return 0
    if args.cmd == "reveal":
        d = Path(args.season_dir)
        commitment = json.loads((d / "commitment.json").read_text(encoding="utf-8"))
        k_bytes = bytes.fromhex((d / "K.secret").read_text(encoding="utf-8").strip())
        drand = commitment["drand"]
        randomness = (
            bytes.fromhex(args.randomness)
            if args.randomness
            else fetch_randomness(drand["chain"], drand["round"])
        )
        reveal = make_reveal(commitment, k_bytes, randomness, episode_counts(args.state))
        _write_json(d / "reveal.json", reveal)
        print(f"publish {d / 'reveal.json'} with the transcripts")
        return 0
    reveal = json.loads(Path(args.reveal).read_text(encoding="utf-8"))
    _, sid, commitment = check_reveal(reveal)
    if args.fetch:
        drand = commitment["drand"]
        if fetch_randomness(drand["chain"], drand["round"]).hex() != reveal["drand_randomness"]:
            raise SystemExit("the randomness is not the committed drand round's")
    print(f"reveal ok: season {commitment['season']}, season_id {sid}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
