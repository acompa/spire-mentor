"""Stage the native SimAdvisor (GH #1426) into ``dist/SimAdvisor/``.

The advisor the mod launches is one Rust binary (``sts2-rs/crates/sim-advisor``)
plus the data it reads beside itself::

    dist/SimAdvisor/
        SimAdvisor.exe, SimAdvisor-linux-x64, SimAdvisor-macos-arm64, ...
                                   one binary per platform, sharing the data
        data/ppo/champions.toml + every declared champion's .onnx
        data/e2e/<Instinct weights>.e2ew   (the all-character net and the Fool's)
        localization_eng/*.json.gz
        localization_zhs/*.json.gz      (the game's Chinese tables, for a game in Chinese)

The same rules as the PyInstaller spec it replaced:
only the champions ``data/ppo/champions.toml`` declares, each checked against
its recorded hash; Instinct's weights required unless
``SIMADVISOR_NO_INSTINCT=1`` (the public export ships none); the localization
tables gzipped, because the game reads every ``.json`` under a mod's folder as
a mod manifest.

Usage::

    uv run --no-project python scripts/stage_advisor.py <path to the built binary> [--out dist/SimAdvisor] [--smoke]
    # another platform's build (e.g. from CI) into the same dist:
    uv run --no-project python scripts/stage_advisor.py <binary> --add --platform macos-arm64
"""

from __future__ import annotations

import argparse
import gzip
import os
import platform
import re
import shutil
import subprocess
import sys
from pathlib import Path

PYCLI = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(PYCLI / "python"))

import champions  # noqa: E402

#: Where the binary looks for the Instinct weights. Read from its own constant
#: so the two cannot drift: a hand-kept copy here staged d300 while the binary
#: wanted d500 (#1521), and the dist shipped without a working Instinct.
_INSTINCT_RS = PYCLI.parent / "sts2-rs" / "crates" / "sts2-advisor" / "src" / "instinct_advisor.rs"


def instinct_relpath(const: str = "WEIGHTS_RELPATH") -> Path:
    """``sts2_advisor::instinct_advisor::<const>``, read from its source
    (``WEIGHTS_RELPATH``: the all-character net; ``FOOL_WEIGHTS_RELPATH``:
    the Fool's, GH #1535)."""
    m = re.search(
        rf'pub const {const}: &str = "([^"]+)";', _INSTINCT_RS.read_text(encoding="utf-8")
    )
    if m is None:
        raise SystemExit(f"no {const} in {_INSTINCT_RS}")
    return Path(m.group(1))


def host_platform() -> str:
    """This machine as ``<os>-<arch>``: ``windows-x64``, ``linux-x64``, ``macos-arm64``."""
    system = {"win32": "windows", "darwin": "macos"}.get(sys.platform, sys.platform)
    machine = platform.machine().lower()
    arch = {"x86_64": "x64", "amd64": "x64", "aarch64": "arm64"}.get(machine, machine)
    return f"{system}-{arch}"


def shipped_name(plat: str) -> str:
    """The binary's name in ``SimAdvisor/``, where every platform's binary
    shares one copy of the data. The mod picks it by OS and architecture
    (``AdvisorChild.BinaryName`` in the mod); Windows keeps the historical
    ``SimAdvisor.exe``."""
    return "SimAdvisor.exe" if plat.startswith("windows-") else f"SimAdvisor-{plat}"


def add_binary(binary: Path, out: Path, plat: str) -> Path:
    """Copy ``binary`` into ``out`` under its shipped name, executable."""
    dest = out / shipped_name(plat)
    shutil.copy2(binary, dest)
    dest.chmod(0o755)
    return dest


def stage(binary: Path, out: Path, plat: str | None = None) -> list[str]:
    """Assemble ``out`` around ``binary``; the lines describing what went in."""
    if os.environ.get("STS2_WB_PPO_ONNX"):
        raise SystemExit("unset STS2_WB_PPO_ONNX before staging: it overrides every champion")
    if not binary.is_file():
        raise SystemExit(f"no advisor binary at {binary}")
    if out.exists():
        shutil.rmtree(out)
    (out / "data" / "ppo").mkdir(parents=True)
    notes: list[str] = []
    notes.append(f"binary: {add_binary(binary, out, plat or host_platform()).name}")

    shutil.copy2(champions.manifest_path(), out / "data" / "ppo")
    for character in champions.characters():
        # Raises naming the file when the manifest is ahead of the tree: a
        # release must not fall back to an older net.
        src = champions.onnx_path(character)
        if src is None:
            raise SystemExit(f"no champion net for {character!r} in the manifest")
        mismatch = champions.verify(character)
        if mismatch:
            raise SystemExit(f"champion hash mismatch for {character!r}: {mismatch}")
        shutil.copy2(src, out / "data" / "ppo")
        notes.append(f"champion: {character} -> {os.path.basename(src)}")

    for const in ("WEIGHTS_RELPATH", "FOOL_WEIGHTS_RELPATH"):
        relpath = instinct_relpath(const)
        instinct = PYCLI / relpath
        if instinct.is_file():
            (out / relpath.parent).mkdir(parents=True, exist_ok=True)
            shutil.copy2(instinct, out / relpath)
            notes.append(f"Instinct: {instinct.name}")
        elif os.environ.get("SIMADVISOR_NO_INSTINCT") == "1":
            notes.append(f"no Instinct weights {relpath.name} (SIMADVISOR_NO_INSTINCT=1)")
        else:
            raise SystemExit(
                f"no Instinct weights at {instinct} (set SIMADVISOR_NO_INSTINCT=1 to build without it)"
            )

    for lang in ("localization_eng", "localization_zhs"):
        loc = sorted((PYCLI / lang).glob("*.json"))
        if not loc:
            raise SystemExit(f"no {lang}/*.json found")
        (out / lang).mkdir()
        for p in loc:
            with (
                p.open("rb") as src,
                gzip.GzipFile(out / lang / (p.name + ".gz"), "wb", mtime=0) as dst,
            ):
                shutil.copyfileobj(src, dst)
        notes.append(f"{lang}: {len(loc)} tables")
    return notes


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=(__doc__ or "").splitlines()[0])
    ap.add_argument("binary", type=Path, help="the built SimAdvisor binary")
    ap.add_argument("--out", type=Path, default=PYCLI / "dist" / "SimAdvisor")
    ap.add_argument(
        "--platform",
        default=None,
        help="the binary's <os>-<arch> (default: this machine's, e.g. linux-x64)",
    )
    ap.add_argument(
        "--add",
        action="store_true",
        help="only add this binary to an already staged --out (another platform's build)",
    )
    ap.add_argument("--smoke", action="store_true", help="then run the staged binary's --smoke")
    args = ap.parse_args(argv)
    out = args.out.resolve()
    plat = args.platform or host_platform()
    if args.add:
        if not (out / "data" / "ppo" / "champions.toml").is_file():
            raise SystemExit(f"--add needs a staged {out}; stage a full build there first")
        print(f"binary: {add_binary(args.binary.resolve(), out, plat).name}")
    else:
        for line in stage(args.binary.resolve(), out, plat):
            print(line)
    size = sum(f.stat().st_size for f in args.out.rglob("*") if f.is_file())
    print(f"staged {args.out} ({size / 2**20:.0f} MB)")
    if args.smoke:
        return subprocess.run([str(out / shipped_name(plat)), "--smoke"], check=False).returncode
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
