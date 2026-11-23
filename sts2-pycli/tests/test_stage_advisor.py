"""The packaging script stages the Instinct net the binary loads (#1426)."""

from __future__ import annotations

import importlib.util
from pathlib import Path

import realgame_instinct

PYCLI = Path(__file__).resolve().parent.parent


def _stage_advisor():
    spec = importlib.util.spec_from_file_location(
        "stage_advisor", PYCLI / "scripts" / "stage_advisor.py"
    )
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_the_staged_instinct_net_is_the_one_both_advisors_load() -> None:
    relpath = _stage_advisor().instinct_relpath()
    assert relpath == realgame_instinct.WEIGHTS_RELPATH
    assert (PYCLI / relpath).is_file()


def test_each_platform_ships_under_the_name_the_mod_launches() -> None:
    """``AdvisorChild.BinaryName`` in the mod expects exactly these."""
    sa = _stage_advisor()
    assert sa.shipped_name("windows-x64") == "SimAdvisor.exe"
    assert sa.shipped_name("linux-x64") == "SimAdvisor-linux-x64"
    assert sa.shipped_name("macos-arm64") == "SimAdvisor-macos-arm64"
    os_name, _, arch = sa.host_platform().partition("-")
    assert os_name in {"windows", "linux", "macos"}
    assert arch
