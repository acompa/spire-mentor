#!/usr/bin/env python3
"""Download the published Instinct weights (`just fetch-weights`).

The nets are not in the git repository. They are assets of GitHub releases,
and `weights/weights.json` lists every current file with its release tag, size
and sha256. This script downloads what is missing and checks each file against
the manifest. Standard library only.

    python scripts/fetch_weights.py                 # the runnable .e2ew of every net
    python scripts/fetch_weights.py --train         # also the .pt checkpoints
    python scripts/fetch_weights.py --net allchar   # one net (repeatable)
    python scripts/fetch_weights.py --list          # print the manifest, download nothing

A `.e2ew` goes to `sts2-pycli/data/e2e/`, where the workbench, the advisor and
the mod's staging script look for it; a `.pt` goes to `weights/`.
`SPIRE_MENTOR_WEIGHTS_URL` replaces the release download base
(`<base>/<tag>/<file>`), for a mirror.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
#: The manifest sits in `weights/` in the public repository; the development
#: repository keeps it in the export overlay.
MANIFESTS = (ROOT / "weights" / "weights.json", ROOT / "release" / "public-overlay" / "weights" / "weights.json")
CHUNK = 1 << 20


def load_manifest() -> dict:
    for path in MANIFESTS:
        if path.is_file():
            return json.loads(path.read_text(encoding="utf-8"))
    sys.exit("fetch-weights: no weights/weights.json found")


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as fh:
        while chunk := fh.read(CHUNK):
            digest.update(chunk)
    return digest.hexdigest()


def url_of(manifest: dict, entry: dict) -> str:
    base = os.environ.get("SPIRE_MENTOR_WEIGHTS_URL") or f"https://github.com/{manifest['repo']}/releases/download"
    return f"{base.rstrip('/')}/{entry['tag']}/{entry['name']}"


def fetch(manifest: dict, entry: dict) -> str:
    """Download one file unless a verified copy exists; returns what happened."""
    dest = ROOT / entry["dest"] / entry["name"]
    if dest.is_file() and sha256_of(dest) == entry["sha256"]:
        return "ok"
    dest.parent.mkdir(parents=True, exist_ok=True)
    part = dest.with_name(dest.name + ".part")
    digest = hashlib.sha256()
    with urllib.request.urlopen(url_of(manifest, entry)) as resp, part.open("wb") as out:
        while chunk := resp.read(CHUNK):
            digest.update(chunk)
            out.write(chunk)
    if digest.hexdigest() != entry["sha256"]:
        part.unlink()
        raise RuntimeError(f"sha256 mismatch for {entry['name']}: got {digest.hexdigest()}, want {entry['sha256']}")
    part.replace(dest)
    return "downloaded"


def main() -> int:
    ap = argparse.ArgumentParser(description="Download the published Instinct weights.")
    ap.add_argument("--net", action="append", help="net id from the manifest (repeatable; default: all)")
    ap.add_argument("--train", action="store_true", help="also download the .pt training checkpoints")
    ap.add_argument("--list", action="store_true", help="print the manifest's files and exit")
    args = ap.parse_args()

    manifest = load_manifest()
    ids = [net["id"] for net in manifest["nets"]]
    for wanted in args.net or []:
        if wanted not in ids:
            sys.exit(f"fetch-weights: unknown net {wanted!r} (known: {', '.join(ids)})")

    failed = 0
    for net in manifest["nets"]:
        if args.net and net["id"] not in args.net:
            continue
        print(f"{net['id']}: {net['title']} ({net['game_version']})")
        for entry in net["files"]:
            where = f"{entry['dest']}/{entry['name']}"
            if args.list:
                print(f"  [{entry['role']}] {where}  {entry['bytes']:,} bytes  {entry['tag']}  sha256 {entry['sha256']}")
                continue
            if entry["role"] == "train" and not args.train:
                continue
            try:
                print(f"  {fetch(manifest, entry)}: {where}")
            except (OSError, RuntimeError) as exc:
                failed += 1
                print(f"  FAILED: {where}: {exc}", file=sys.stderr)
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
