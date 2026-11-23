"""The "ooc-potion" fill event (GH #1606, SCHEMA.md section 14): the potion popup.

A run of a launch that declares the event offers, on every out-of-combat screen, a
`use_potion_ooc` per potion the game lets the player drink there and a
`discard_potion` per potion, after the screen's own actions. They ride inside the
screen's decision (kinds USE_POTION_OOC and DISCARD_POTION, tokens in
RES_OPT_POTION_USE). A launch without the declaration builds runs without the popup,
so its rows stay byte-identical (`test_e2e_chars.py::test_undeclared_ironclad_rows_unchanged`).
"""

from __future__ import annotations

import numpy as np
import pytest
import sts2_client
from ppo.e2e import contracts as C
from ppo.e2e import factory
from ppo.e2e import fillcheck as FC
from ppo.e2e import net as N
from ppo.e2e.rsenv_lockstep import Lockstep, run_seeds

SEED = 1_300_000_700  # the throwaway block (plan §3.1)
POPUP = {C.AK["USE_POTION_OOC"], C.AK["DISCARD_POTION"]}


@pytest.fixture(autouse=True, scope="module")
def _campaign_process():
    """The factory needs the process to select v0.111.0 (`STS2_GAME_VERSION`)."""
    with pytest.MonkeyPatch.context() as mp:
        mp.setattr(sts2_client, "GAME_VERSION", factory.GAME_VERSION)
        yield


def test_the_event_takes_a_reserved_section_and_moves_no_hash() -> None:
    assert C.FILL_OOC_POTION in C.FILL_EVENTS
    assert C.SECTIONS[C.OOC_POTION_SECTION] == "RES_OPT_POTION_USE"
    assert C.FILL_SLOTS[C.FILL_OOC_POTION] == {"sections": (C.OOC_POTION_SECTION,)}
    spec = C.schema_spec()
    assert "RES_OPT_POTION_USE" not in spec["section_live"]
    statuses = {k[1]: k[2] for k in spec["action_kinds"]}
    assert statuses["USE_POTION_OOC"] == statuses["DISCARD_POTION"] == "reserved:ooc-potion"
    assert "USE_POTION_OOC" not in spec["kind_section"]
    # The tokens' live columns (not hashed): kind, potion, position.
    assert C.SECTION_LIVE[C.OOC_POTION_SECTION][0] == tuple(
        C.XC[n] for n in ("X_KIND", "X_ID_MAIN", "X_POS")
    )


def test_the_popup_rides_on_the_screen_decision() -> None:
    move = {"action": "move", "col": 1}
    use = {"action": "use_potion_ooc", "potion_slot": 0}
    discard = {"action": "discard_potion", "potion_slot": 1}
    assert C.classify_ooc([move, use, discard]) == (
        C.DT["MAP"],
        [C.AK["MOVE"], C.AK["USE_POTION_OOC"], C.AK["DISCARD_POTION"]],
    )
    leave = {"action": "leave_shop"}
    assert C.classify_ooc([leave, discard])[0] == C.DT["SHOP"]
    # A deck pick preempts the belt in the engine; its type does not take the kinds.
    with pytest.raises(C.SchemaError, match="cannot offer"):
        C.check_kinds(C.DT["CARD_REMOVE"], [C.AK["REMOVE_DECK_CARD"], C.AK["DISCARD_POTION"]])


def test_the_factory_builds_the_popup_only_when_declared(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv(factory.OOC_POTION_ENV, raising=False)
    rm = factory.new_run(SEED)
    assert not rm.ooc_potion_actions
    monkeypatch.setenv(factory.OOC_POTION_ENV, "1")
    with pytest.raises(AssertionError, match="ooc-potion"):
        factory.check_run(rm)
    rm = factory.new_run(SEED)
    assert rm.ooc_potion_actions and factory.check_run(rm) is rm


def _declared_rows(monkeypatch: pytest.MonkeyPatch, n: int = 10) -> dict[str, np.ndarray]:
    monkeypatch.setenv(factory.OOC_POTION_ENV, "1")
    rb, _terms = FC.sample_rows(n, SEED)
    return rb


def test_a_declared_run_tokenizes_and_validates(monkeypatch: pytest.MonkeyPatch) -> None:
    rb = _declared_rows(monkeypatch)
    C.validate_row_batch(rb)
    kinds = set(rb["cls_kind"].tolist())
    # Random play rarely holds an AnyTime potion; the engine tests cover the use.
    assert C.AK["DISCARD_POTION"] in kinds, kinds
    popup = np.isin(rb["cat"][:, C.XC["X_KIND"]], list(POPUP)) & (rb["tok_flags"] != 0)
    assert popup.any()
    assert (rb["sec"][popup] == C.OOC_POTION_SECTION).all()
    assert (rb["cat"][popup, C.XC["X_ID_MAIN"]] != 0).all()  # the potion's id
    # Never on a combat row: the env plays fights from `cs.legal_actions()`.
    assert not (rb["is_combat"].any() and POPUP & set(_combat_kinds(rb)))
    # The fill check ties the section to the declaration.
    wp = list(FC.wheel_filled())
    assert C.FILL_OOC_POTION in FC.check(rb, wp).stray
    assert C.FILL_OOC_POTION not in FC.check(rb, [*wp, C.FILL_OOC_POTION]).stray


def _combat_kinds(rb: dict[str, np.ndarray]) -> list[int]:
    off = C.row_offsets(rb)
    out: list[int] = []
    for i in np.flatnonzero(rb["is_combat"]):
        out += rb["cls_kind"][off["cls"][i] : off["cls"][i + 1]].tolist()
    return out


def test_an_undeclared_net_keeps_the_popup_inputs_at_zero() -> None:
    old, new = N.layout(), N.layout([C.FILL_OOC_POTION])
    assert C.OOC_POTION_SECTION in old.reserved_sections
    assert C.OOC_POTION_SECTION not in new.reserved_sections
    assert not POPUP & set(old.live_kinds) and set(new.live_kinds) >= POPUP


def test_lockstep_declared(monkeypatch: pytest.MonkeyPatch) -> None:
    """The Rust training env's rows equal the Python env's on runs with the popup."""
    monkeypatch.setenv(factory.OOC_POTION_ENV, "1")
    ls = Lockstep(f9_p=0.0)
    run_seeds(ls, SEED, 8, pinned=False, practice=0)
    assert ls.stats["rows"] > 200
    assert ls.mismatches == 0, {"stats": dict(ls.stats), "examples": ls.examples[:5]}
