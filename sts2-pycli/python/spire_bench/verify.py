"""Replays an episode transcript on a local referee host and checks it.

Once the season key is revealed, anyone can recompute each episode key and
check that the recorded commands, played on the same engine, give the same
replies (the fingerprint) and the same outcome::

    just bench-verify <season key hex> transcripts/*.jsonl   # from sts2-pycli/

Every result carries the replayed ``fingerprint`` (``spire_bench.chain``: it
covers the header's identity, so an edited header changes it too). The gateway hands an agent
the same value with every episode state (its receipt), so an agent can check
that the published transcript of its episode was not cut short or relabelled:
the last receipt it got must equal the fingerprint here. A deadline loss is
only checkable that way, since timing does not replay.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import sys
from pathlib import Path
from typing import Any

from spire_bench.chain import add_reply, add_trigger, new_chain
from spire_bench.episode import _digest
from spire_bench.keys import episode_key, episode_label
from spire_bench.season import check_reveal
from spire_bench.worker import RefereeWorker, WorkerError, engine_hash


def _check_header(header: dict[str, Any], commands: list[dict[str, Any]]) -> str | None:
    """Why the header does not describe the episode the commands play, or None."""
    label = episode_label(header["agent_id"], int(header["episode_no"]))
    if header.get("label") != label or header.get("episode_id") != label:
        return f"episode id / label do not match {label}"
    if not commands:
        return None
    starts = [i for i, row in enumerate(commands) if row["cmd"].get("cmd") == "start_run"]
    if starts != [0]:
        return "the first command, and only it, must be start_run"
    start = commands[0]["cmd"]
    if "rng_key" in start:
        return "the transcript carries a key"
    for field, want in (("character", "character"), ("ascension", "ascension"), ("seed", "label")):
        if start.get(field) != header.get(want):
            return f"start_run {field} differs from the header"
    return None


def _is_over(reply: dict[str, Any]) -> bool:
    return reply.get("type") == "decision" and reply.get("decision") == "game_over"


def _mid_game(reply: dict[str, Any]) -> bool:
    return reply.get("type") == "decision" and not _is_over(reply)


def _check_outcome(
    end: dict[str, Any],
    header: dict[str, Any],
    commands: list[dict[str, Any]],
    replies: list[dict[str, Any]],
    trigger_failed: bool | None = None,
) -> str | None:
    """Why the end row's claim does not follow from the replay, or None.

    ``replies`` are the replayed replies, one per command. ``trigger_failed``
    says whether resending the end row's trigger made the host fail (None if
    it was not resent).
    """
    status, reason = end.get("status"), end.get("reason")
    last = replies[-1] if replies else {}
    trigger = end.get("trigger")
    if status == "won":
        return (
            None
            if _is_over(last) and last.get("victory") is True
            else "won without a replayed victory"
        )
    if status == "lost":
        if reason == "game_over":
            return (
                None
                if _is_over(last) and last.get("victory") is False
                else "lost without a replayed defeat"
            )
        if reason in ("decision_deadline", "run_deadline"):
            # Not replayable: the agent's last receipt is the check.
            return None if _mid_game(last) else "a deadline loss that did not end mid-game"
        if reason == "invalid_actions":
            limit = int((header.get("limits") or {}).get("max_invalid", 0))
            streak = 0
            for reply in reversed(replies):
                if reply.get("type") != "error":
                    break
                streak += 1
            return (
                None
                if limit and streak == limit
                else f"{streak} refused actions in a row, limit {limit}"
            )
        if reason in ("referee_fault_during_action", "unexpected_reply_during_action"):
            if not commands or trigger != commands[-1]["cmd"] or len(commands) < 2:
                return "the trigger is not the last command"
            want_fault = reason == "referee_fault_during_action"
            got_fault = last.get("type") == "referee_fault"
            odd = last.get("type") not in ("decision", "error", "referee_fault")
            return (
                None if (got_fault if want_fault else odd) else "the trigger did not fail on replay"
            )
        if reason == "worker_error_during_action":
            if trigger is None or not _mid_game(last):
                return "no trigger, or the game was not mid-way"
            return None if trigger_failed else "the trigger did not crash the host on replay"
        return f"unknown loss reason {reason!r}"
    if status == "void":
        if reason == "worker_error":
            return None if not commands else "a start-up void with commands"
        if reason in ("referee_fault", "unexpected_reply"):
            if len(commands) != 1:
                return "a start-up void must hold only start_run"
            bad = (
                last.get("type") == "referee_fault"
                if reason == "referee_fault"
                else last.get("type") != "decision"
            )
            return None if bad else "start_run did not fail on replay"
        if reason in ("server_restart", "server_shutdown"):
            return None if not replies or _mid_game(last) else "a restart void after the game ended"
        return f"unknown void reason {reason!r}"
    return f"unknown status {status!r}"


def _against_commitment(header: dict[str, Any], commitment: dict[str, Any]) -> str | None:
    """Why the header does not run under the committed parameters, or None."""
    for field in ("game_version", "engine_hash"):
        if header.get(field) != commitment.get(field):
            return f"{field} is not the committed one"

    def norm(limits: dict[str, Any]) -> dict[str, float]:
        return {k: float(v) for k, v in limits.items()}

    if norm(header.get("limits") or {}) != norm(commitment.get("limits") or {}):
        return "the limits are not the committed ones"
    return None


def _trigger_shape(trigger: Any) -> str | None:
    """Why a trigger is not a plain agent action, or None."""
    if not isinstance(trigger, dict) or trigger.get("cmd") != "action":
        return "the trigger is not an action"
    if set(trigger) - {"cmd", "action", "args"} or not isinstance(trigger.get("action"), str):
        return "the trigger carries more than an action and its args"
    if "args" in trigger and not isinstance(trigger["args"], dict):
        return "the trigger's args are not an object"
    return None


async def verify(
    path: str | Path,
    season_key: bytes,
    *,
    max_invalid: int | None = None,
    season_id: str | None = None,
    committed: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Returns ``{"ok": bool, ...}``, with ``why`` when not ok.

    ``max_invalid``, when given (the season's published limit), must equal the
    header's: the header alone could claim a looser one. ``season_id``, when
    given, must be the header's: a transcript of another season fails.
    ``committed`` (the season's commitment), when given, must agree with the
    header's game version, engine hash and limits.
    """
    rows = [json.loads(line) for line in Path(path).read_text(encoding="utf-8").splitlines()]
    if not rows or rows[0].get("type") != "header":
        return {"ok": False, "why": "no header", "path": str(path)}
    header, commands = rows[0], [r for r in rows if r["type"] == "command"]
    ends = [r for r in rows if r["type"] == "end"]
    out: dict[str, Any] = {
        "episode_id": header.get("episode_id"),
        "agent_id": header.get("agent_id"),
        "episode_no": int(header.get("episode_no", -1)),
        "character": header.get("character"),
        "ascension": header.get("ascension"),
        "commands": len(commands),
    }
    if len(ends) != 1 or rows[-1] is not ends[0]:
        return {**out, "ok": False, "why": "the transcript must end with exactly one end row"}
    end = ends[0]
    if (why := _check_header(header, commands)) is not None:
        return {**out, "ok": False, "why": why}
    if season_id is not None and header.get("season_id") != season_id:
        return {**out, "ok": False, "why": "the transcript is not of this season"}
    if committed is not None and (why := _against_commitment(header, committed)) is not None:
        return {**out, "ok": False, "why": why}
    if max_invalid is not None and (header.get("limits") or {}).get("max_invalid") != max_invalid:
        return {**out, "ok": False, "why": f"max_invalid is not the season's {max_invalid}"}
    if end.get("reason") == "worker_error_during_action":
        if (why := _trigger_shape(end.get("trigger"))) is not None:
            return {**out, "ok": False, "why": why}
        if end.get("failure") not in ("exited", "timeout"):
            return {**out, "ok": False, "why": "unknown host failure kind"}
    if "fingerprint" not in end and end.get("reason") != "server_restart":
        return {**out, "ok": False, "why": "the end row has no fingerprint"}
    local = engine_hash(header["game_version"])
    if local != header["engine_hash"]:
        return {
            **out,
            "ok": False,
            "why": f"engine differs: local {local}, transcript {header['engine_hash']}",
        }
    key = episode_key(season_key, header["agent_id"], int(header["episode_no"]))
    chain = new_chain(header)
    replies: list[dict[str, Any]] = []
    trigger_failed: bool | None = None
    if commands or end.get("trigger") is not None:
        worker = await RefereeWorker.spawn()
        try:
            for i, row in enumerate(commands):
                cmd = dict(row["cmd"])
                if i == 0:
                    cmd["rng_key"] = key
                reply = await worker.send(cmd)
                d = _digest(reply)
                if d != row["reply_sha256"]:
                    return {**out, "ok": False, "why": f"reply {i} differs", "cmd": row["cmd"]}
                add_reply(chain, d)
                replies.append(reply)
            if end.get("reason") == "worker_error_during_action":
                # Only the same kind of failure counts: a crash is not a timeout.
                try:
                    await worker.send(end["trigger"])
                    trigger_failed = False
                except WorkerError as exc:
                    trigger_failed = exc.kind == end["failure"]
                add_trigger(chain, end["trigger"])
        finally:
            await worker.close()
    fingerprint = chain.hexdigest()
    out["fingerprint"] = fingerprint
    if "fingerprint" in end and fingerprint != end["fingerprint"]:
        return {**out, "ok": False, "why": "fingerprint differs"}
    if (why := _check_outcome(end, header, commands, replies, trigger_failed)) is not None:
        return {**out, "ok": False, "why": why}
    return {**out, "ok": True, "status": end["status"], "reason": end["reason"]}


async def verify_all(
    paths: list[str | Path],
    season_key: bytes,
    *,
    max_invalid: int | None = None,
    season_id: str | None = None,
    committed: dict[str, Any] | None = None,
) -> list[dict[str, Any]]:
    """Verifies each transcript; a second transcript of the same episode fails.

    One episode number, one transcript: otherwise a publisher could run an
    episode twice and show the better run.
    """
    results: list[dict[str, Any]] = []
    seen: dict[tuple[str, int], str] = {}
    for path in paths:
        result = await verify(
            path, season_key, max_invalid=max_invalid, season_id=season_id, committed=committed
        )
        if result.get("agent_id") is not None:
            ident = (result["agent_id"], result["episode_no"])
            if ident in seen:
                result = {**result, "ok": False, "why": f"same episode as {seen[ident]}"}
            seen.setdefault(ident, str(path))
        results.append(result)
    return results


def season_args(reveal_path: str | Path) -> tuple[bytes, dict[str, Any]]:
    """The season key and the ``verify`` keywords a published reveal implies."""
    reveal = json.loads(Path(reveal_path).read_text(encoding="utf-8"))
    key, sid, commitment = check_reveal(reveal)
    return key, {
        "season_id": sid,
        "max_invalid": int(commitment["limits"]["max_invalid"]),
        "committed": commitment,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description="Verify Spire Bench transcripts.")
    who = ap.add_mutually_exclusive_group(required=True)
    who.add_argument("--season", help="the published reveal.json (spire_bench.season)")
    who.add_argument("--season-key", help="a dev server's season key (hex)")
    ap.add_argument("--max-invalid", type=int, help="the season's published refused-action limit")
    ap.add_argument("transcripts", nargs="+")
    args = ap.parse_args()
    if args.season:
        key, kw = season_args(args.season)
    else:
        key, kw = bytes.fromhex(args.season_key), {"max_invalid": args.max_invalid}
    results = asyncio.run(verify_all(args.transcripts, key, **kw))
    for result in results:
        print(json.dumps(result), flush=True)
    bad = sum(not r["ok"] for r in results)
    print(f"{len(results) - bad}/{len(results)} verified", file=sys.stderr)
    return 1 if bad else 0


if __name__ == "__main__":
    raise SystemExit(main())
