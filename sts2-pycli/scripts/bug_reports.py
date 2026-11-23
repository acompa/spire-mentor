#!/usr/bin/env python3
"""Bug reports from the browser workbench: list them, pull one down.

The static site (Cloudflare Pages) stores reports in a Workers KV namespace
through its Pages Function (``web/functions/api/_bugreport.ts``). Reading them
back needs no server of ours:

    python scripts/bug_reports.py list [--limit N]
        every stored report's header, newest first — ``wrangler kv key list``
        over the namespace ``web/wrangler.toml`` binds (needs ``wrangler login``)
    python scripts/bug_reports.py get <id> [--out DIR] [--http]
        the dump, read from the KV namespace (``wrangler kv key get``; the
        site's ``/api/bugreport/<id>`` is behind its sign-in -- ``--http``
        reads that route instead, for a deployment without one), written
        to ``DIR/<id>.json``; a run log it carries is unpacked next to it as
        ``DIR/<id>.jsonl`` (the desktop workbench opens that from its logs dir),
        a persisted live tree as ``DIR/<id>.persist.json``. A report from the
        real game (the Spire Mentor mod's in-game "Report a bug", or the
        mod-bundled workbench's) also yields ``DIR/<id>.state.json`` -- the
        game position, which the workbench's Import state takes as pasted --
        and, from the mod, ``DIR/<id>.run_save.json`` (the game's start-of-room
        save) and ``DIR/<id>.<mod|game|advisor>.log`` (its log tails).

    python scripts/bug_reports.py resolve <id>... [--as fixed|wontfix|duplicate] [--note TEXT]
        flag reports as dealt with: a ``bugstatus/<id>`` key next to the
        report (the report itself is never rewritten), whose KV metadata
        carries the status, so ``list`` reads every flag in one call
    python scripts/bug_reports.py reopen <id>...
        drop the flag again

``list`` shows only the open reports unless given ``--all``, which adds a
status column. ``just bug-reports`` / ``just bug-reports-all`` /
``just bug-report <id>`` / ``just bug-resolve <id>... [--note TEXT]`` /
``just bug-reopen <id>...`` wrap them. Stdlib only.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tomllib
import urllib.request
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
PYCLI = HERE.parent
WEB = PYCLI / "web"
# The hosted workbench's origin. The real one is private (set STS2_WORKBENCH_SITE,
# which sts2-pycli/deploy/justfile exports); the default is a placeholder.
DEFAULT_SITE = os.environ.get("STS2_WORKBENCH_SITE", "https://your-workbench.example.com")
DEFAULT_OUT = PYCLI / "logs" / "bugreports"
KEY_PREFIX = "bugreports/"
STATUS_PREFIX = "bugstatus/"
STATUSES = ("fixed", "wontfix", "duplicate")
# `npx` is `npx.cmd` on Windows, which subprocess does not find by the bare name.
NPX = shutil.which("npx") or "npx"
ID_RE = re.compile(r"^wb-[0-9a-f]{16}$")
USER_AGENT = "sts2-bug-reports/1 (https://github.com/tfriedel/spire-mentor)"
BINDING = "BUGREPORTS"


def namespace_id(toml_text: str, binding: str = BINDING) -> str:
    """The KV namespace id ``binding`` points at in a Pages wrangler.toml
    (the production block, not ``[env.*]``)."""
    cfg = tomllib.loads(toml_text)
    for ns in cfg.get("kv_namespaces", []):
        if ns.get("binding") == binding:
            return str(ns["id"])
    raise SystemExit(f"no [[kv_namespaces]] with binding = {binding!r} in wrangler.toml")


def parse_key_list(stdout: str) -> list[dict[str, Any]]:
    """``wrangler kv key list`` output: a JSON array, possibly after a banner."""
    start = stdout.find("[")
    if start < 0:
        raise SystemExit(f"unexpected wrangler output: {stdout[:200]!r}")
    rows = json.loads(stdout[start:])
    if not isinstance(rows, list):
        raise SystemExit("unexpected wrangler output: not a list")
    return rows


def parse_kv_value(stdout: str) -> dict[str, Any]:
    """``wrangler kv key get --text`` output: the stored JSON, possibly after
    a banner line (wrangler's proxy warning)."""
    for i, line in enumerate(stdout.splitlines()):
        if line.lstrip().startswith("{"):
            dump = json.loads("\n".join(stdout.splitlines()[i:]))
            if isinstance(dump, dict):
                return dump
            break
    raise SystemExit(f"no report in wrangler's answer: {stdout.strip()[:200]!r}")


def listing(rows: list[dict[str, Any]], limit: int | None = None) -> list[dict[str, Any]]:
    """Flatten key rows into report headers, newest first."""
    out = []
    for r in rows:
        name = str(r.get("name", ""))
        if not name.startswith(KEY_PREFIX):
            continue
        meta = r.get("metadata") or {}
        if not isinstance(meta, dict):
            meta = {}
        out.append(
            {
                "id": name[len(KEY_PREFIX) :],
                "captured_at": meta.get("captured_at") or "",
                "mode": meta.get("mode") or "",
                "character": meta.get("character") or "",
                "screen": meta.get("screen") or "",
                "build": meta.get("build") or "",
                "note": meta.get("note") or "",
            }
        )
    out.sort(key=lambda h: h["captured_at"], reverse=True)
    return out[:limit] if limit else out


def status_map(rows: list[dict[str, Any]]) -> dict[str, dict[str, str]]:
    """``bugstatus/<id>`` key rows -> ``{id: {"status", "note", "at"}}``."""
    out: dict[str, dict[str, str]] = {}
    for r in rows:
        name = str(r.get("name", ""))
        if not name.startswith(STATUS_PREFIX):
            continue
        meta = r.get("metadata") or {}
        if not isinstance(meta, dict):
            meta = {}
        out[name[len(STATUS_PREFIX) :]] = {
            "status": str(meta.get("status") or "fixed"),
            "note": str(meta.get("note") or ""),
            "at": str(meta.get("at") or ""),
        }
    return out


def with_status(
    headers: list[dict[str, Any]], statuses: dict[str, dict[str, str]], *, show_all: bool
) -> list[dict[str, Any]]:
    """Stamp each header's status ("open" when unflagged); drop the flagged
    ones unless ``show_all``."""
    out = []
    for h in headers:
        st = statuses.get(h["id"])
        if not show_all and st is not None:
            continue
        out.append(
            {**h, "status": st["status"] if st else "open", "status_note": st["note"] if st else ""}
        )
    return out


def status_record(status: str, note: str, at: str) -> dict[str, str]:
    """What a flag stores, as both its value and its KV metadata (<= 1024 B)."""
    if status not in STATUSES:
        raise SystemExit(f"unknown status {status!r} (one of {', '.join(STATUSES)})")
    return {"status": status, "note": note[:300], "at": at}


def format_listing(headers: list[dict[str, Any]]) -> str:
    if not headers:
        return "no bug reports stored"
    show_status = any(h.get("status") not in (None, "open") for h in headers)
    lines = [
        f"{'id':22} {'captured':20} {'mode':6} {'character':11} {'screen':14} {'build':12} "
        + ("status    " if show_status else "")
        + "note"
    ]
    for h in headers:
        note = h["note"].replace("\n", " ")
        if len(note) > 60:
            note = note[:57] + "..."
        if h.get("status_note"):
            note = f"[{h['status_note']}] {note}"
        status = f"{h.get('status', 'open'):9} " if show_status else ""
        lines.append(
            f"{h['id']:22} {h['captured_at']:20} {h['mode']:6} {h['character']:11} "
            f"{h['screen']:14} {str(h['build'])[:12]:12} {status}{note}"
        )
    return "\n".join(lines)


def unpack(dump: dict[str, Any], out_dir: Path, report_id: str) -> list[Path]:
    """Write the dump and whatever run it carries; returns the paths written."""
    out_dir.mkdir(parents=True, exist_ok=True)
    written = []
    p = out_dir / f"{report_id}.json"
    p.write_text(json.dumps(dump, indent=1, sort_keys=True) + "\n", encoding="utf-8")
    written.append(p)
    repro = dump.get("repro") or {}
    log = repro.get("run_log") if isinstance(repro, dict) else None
    if isinstance(log, str) and log.strip():
        q = out_dir / f"{report_id}.jsonl"
        q.write_text(log if log.endswith("\n") else log + "\n", encoding="utf-8")
        written.append(q)
    persist = repro.get("persist") if isinstance(repro, dict) else None
    if isinstance(persist, dict):
        q = out_dir / f"{report_id}.persist.json"
        q.write_text(json.dumps(persist) + "\n", encoding="utf-8")
        written.append(q)
    position = game_position(dump)
    if position is not None:
        q = out_dir / f"{report_id}.state.json"
        q.write_text(json.dumps(position) + "\n", encoding="utf-8")
        written.append(q)
    run_save = repro.get("run_save") if isinstance(repro, dict) else None
    if isinstance(run_save, dict) and run_save.get("save") is not None:
        q = out_dir / f"{report_id}.run_save.json"
        q.write_text(json.dumps(run_save["save"]) + "\n", encoding="utf-8")
        written.append(q)
    logs = dump.get("logs")
    if isinstance(logs, dict):
        for name in ("mod", "game", "advisor"):
            text = logs.get(name)
            if isinstance(text, list):
                text = "\n".join(str(line) for line in text)
            if isinstance(text, str) and text:
                q = out_dir / f"{report_id}.{name}.log"
                q.write_text(text if text.endswith("\n") else text + "\n", encoding="utf-8")
                written.append(q)
    return written


def game_position(dump: dict[str, Any]) -> dict[str, Any] | None:
    """The live-game position a report carries (web/src/bugreport.ts
    ``reportGamePosition``): a mod report's ``state.game_state``, a
    bundled-workbench report's ``state.live_game``; None otherwise."""

    def position(v: Any) -> dict[str, Any] | None:
        if isinstance(v, dict) and v.get("type") in ("advice_request", "run_snapshot"):
            return v
        return None

    state = dump.get("state")
    meta = dump.get("meta")
    if not isinstance(state, dict):
        return None
    if isinstance(meta, dict) and meta.get("source") == "mod":
        found = position(state.get("game_state"))
        if found is not None:
            return found
    return position(state.get("live_game"))


def _kv(*args: str) -> subprocess.CompletedProcess[str]:
    """One ``wrangler kv`` call against the bound namespace. ``--remote``:
    wrangler 4 talks to a LOCAL dev store by default (and lists ``[]``)."""
    ns = namespace_id((WEB / "wrangler.toml").read_text(encoding="utf-8"))
    cmd = [NPX, "wrangler", "kv", *args, "--namespace-id", ns, "--remote"]
    return subprocess.run(cmd, cwd=WEB, capture_output=True, text=True, encoding="utf-8")


def _key_rows(prefix: str) -> list[dict[str, Any]]:
    proc = _kv("key", "list", "--prefix", prefix)
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        raise SystemExit(proc.returncode)
    return parse_key_list(proc.stdout)


def cmd_list(args: argparse.Namespace) -> int:
    headers = listing(_key_rows(KEY_PREFIX))
    headers = with_status(headers, status_map(_key_rows(STATUS_PREFIX)), show_all=args.all)
    print(format_listing(headers[: args.limit] if args.limit else headers))
    return 0


def _check_ids(ids: list[str]) -> None:
    for i in ids:
        if not ID_RE.match(i):
            raise SystemExit(f"not a report id: {i!r} (wb- + 16 hex)")


def cmd_resolve(args: argparse.Namespace) -> int:
    _check_ids(args.ids)
    at = datetime.now(UTC).strftime("%Y-%m-%dT%H:%M:%SZ")
    rec = json.dumps(status_record(args.status, args.note, at))
    for i in args.ids:
        proc = _kv("key", "put", STATUS_PREFIX + i, rec, "--metadata", rec)
        if proc.returncode != 0:
            sys.stderr.write(proc.stderr)
            return proc.returncode
        print(f"{i}: {args.status}" + (f" ({args.note})" if args.note else ""))
    return 0


def cmd_reopen(args: argparse.Namespace) -> int:
    _check_ids(args.ids)
    for i in args.ids:
        proc = _kv("key", "delete", STATUS_PREFIX + i)
        if proc.returncode != 0:
            sys.stderr.write(proc.stderr)
            return proc.returncode
        print(f"{i}: open")
    return 0


def cmd_get(args: argparse.Namespace) -> int:
    if not ID_RE.match(args.id):
        raise SystemExit(f"not a report id: {args.id!r} (wb- + 16 hex)")
    if args.http:
        url = f"{args.site.rstrip('/')}/api/bugreport/{args.id}"
        # Cloudflare's bot check answers 403 to the default `Python-urllib/x` agent.
        req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(req, timeout=60) as res:
            dump = json.load(res)
    else:
        ns = namespace_id((WEB / "wrangler.toml").read_text(encoding="utf-8"))
        cmd = [
            NPX,
            "wrangler",
            "kv",
            "key",
            "get",
            KEY_PREFIX + args.id,
            "--namespace-id",
            ns,
            "--remote",
            "--text",
        ]
        proc = subprocess.run(cmd, cwd=WEB, capture_output=True, text=True, encoding="utf-8")
        if proc.returncode != 0:
            sys.stderr.write(proc.stderr)
            return proc.returncode
        dump = parse_kv_value(proc.stdout)
    for p in unpack(dump, Path(args.out), args.id):
        print(p)
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    sub = ap.add_subparsers(dest="cmd", required=True)
    ls = sub.add_parser("list", help="list stored reports, newest first")
    ls.add_argument("--limit", type=int, default=50)
    ls.add_argument("--all", action="store_true", help="include the flagged (fixed, ...) ones")
    ls.set_defaults(fn=cmd_list)
    res = sub.add_parser("resolve", help="flag reports as dealt with")
    res.add_argument("ids", nargs="+")
    res.add_argument("--as", dest="status", choices=STATUSES, default="fixed")
    res.add_argument("--note", default="", help='e.g. "PR #1563"')
    res.set_defaults(fn=cmd_resolve)
    reo = sub.add_parser("reopen", help="drop the flag of reports")
    reo.add_argument("ids", nargs="+")
    reo.set_defaults(fn=cmd_reopen)
    get = sub.add_parser("get", help="download one report (+ its run) to --out")
    get.add_argument("id")
    get.add_argument("--out", default=str(DEFAULT_OUT))
    get.add_argument("--site", default=DEFAULT_SITE)
    get.add_argument(
        "--http",
        action="store_true",
        help="read the site's /api/bugreport/<id> instead of KV (a site without a sign-in)",
    )
    get.set_defaults(fn=cmd_get)
    args = ap.parse_args(argv)
    return args.fn(args)


if __name__ == "__main__":
    raise SystemExit(main())
