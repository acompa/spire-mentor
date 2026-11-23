"""The "#1069" fill event (GH #1620, SCHEMA.md section 17): the game's reward screen.

A run of a launch that declares the event lists a won combat room's rewards on the
game's reward screen: take or skip each (`take_reward` / `skip_reward`, naming the
reward in `reward`, `id`, `amount`), in any order, and `proceed_rewards` to leave
the rest. A treasure chest's relic has its own skip. A launch without the
declaration builds runs that auto-collect, so its rows stay byte-identical
(`test_e2e_chars.py::test_undeclared_ironclad_rows_unchanged`).
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

SEED = 1_300_000_900  # the throwaway block (plan §3.1)


@pytest.fixture(autouse=True, scope="module")
def _campaign_process():
    """The factory needs the process to select v0.111.0 (`STS2_GAME_VERSION`)."""
    with pytest.MonkeyPatch.context() as mp:
        mp.setattr(sts2_client, "GAME_VERSION", factory.GAME_VERSION)
        yield


def test_the_event_is_the_reserved_one_and_moves_no_hash() -> None:
    assert C.FILL_REWARD_SCREEN == "#1069" and C.FILL_REWARD_SCREEN in C.FILL_EVENTS
    assert C.SECTIONS[C.REWARD_SCREEN_SECTION] == "RES_OPT_REWARD"
    spec = C.schema_spec()
    assert "RES_OPT_REWARD" not in spec["section_live"]
    statuses = {k[1]: k[2] for k in spec["action_kinds"]}
    for name in ("TAKE_REWARD_POTION", "SKIP_REWARD_POTION", "TAKE_REWARD_GOLD"):
        assert statuses[name] == "reserved:#1069"
    # The spares the event names stay spare in the hashed table.
    for k in C.REWARD_KIND_NAMES:
        assert C.ACTION_KINDS[k].status == "spare"
        assert C.AK[C.REWARD_KIND_NAMES[k]] == k == C.AK[C.ACTION_KINDS[k].name]
    dts = {d[1]: (d[2], d[4]) for d in spec["decision_types"]}
    assert dts["REWARD_SCREEN"] == ("reserved:#1069", [48, 49, 50])
    assert dts["TREASURE_CHOICE"] == ("reserved:treasure", [33, 51])


def test_the_screen_classifies_by_the_reward_each_action_names() -> None:
    gold = {"action": "take_reward", "idx": 0, "reward": "gold", "amount": 12}
    relic = {"action": "take_reward", "idx": 1, "reward": "relic", "id": "Gorget"}
    card = {"action": "take_reward", "idx": 2, "reward": "card"}
    skips = [dict(a, action="skip_reward") for a in (gold, relic, card)]
    proceed = {"action": "proceed_rewards", "chest": False}
    discard = {"action": "discard_potion", "potion_slot": 0}
    dtype, kinds = C.classify_ooc([gold, relic, card, *skips, proceed, discard])
    assert dtype == C.DT["REWARD_SCREEN"]
    assert [C.kind_name(k) for k in kinds] == [
        "TAKE_REWARD_GOLD",
        "TAKE_REWARD_RELIC",
        "TAKE_REWARD_CARD",
        "SKIP_REWARD_GOLD",
        "SKIP_REWARD_RELIC",
        "SKIP_REWARD_CARD",
        "PROCEED_REWARDS",
        "DISCARD_POTION",
    ]
    chest = {"action": "take_reward", "idx": 0, "reward": "chest_relic", "id": "Gorget"}
    leave = dict(chest, action="skip_reward")
    assert C.classify_ooc([chest, leave, {"action": "proceed_rewards", "chest": True}]) == (
        C.DT["TREASURE_CHOICE"],
        [C.AK["COLLECT_TREASURE"], C.AK["SKIP_TREASURE"], C.AK["SKIP_TREASURE"]],
    )
    with pytest.raises(C.SchemaError, match="no known reward"):
        C.classify_ooc([{"action": "take_reward", "idx": 0}, proceed])


def test_the_factory_builds_the_screen_only_when_declared(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv(factory.REWARD_SCREEN_ENV, raising=False)
    rm = factory.new_run(SEED)
    assert not rm.reward_screen
    monkeypatch.setenv(factory.REWARD_SCREEN_ENV, "1")
    with pytest.raises(AssertionError, match="reward screen"):
        factory.check_run(rm)
    rm = factory.new_run(SEED)
    assert rm.reward_screen and factory.check_run(rm) is rm


def _declared_rows(monkeypatch: pytest.MonkeyPatch, n: int = 10) -> dict[str, np.ndarray]:
    monkeypatch.setenv(factory.REWARD_SCREEN_ENV, "1")
    rb, _terms = FC.sample_rows(n, SEED)
    return rb


def test_a_declared_run_tokenizes_and_validates(monkeypatch: pytest.MonkeyPatch) -> None:
    rb = _declared_rows(monkeypatch)
    C.validate_row_batch(rb)
    dtypes = set(rb["dtype"].tolist())
    assert C.DT["REWARD_SCREEN"] in dtypes, dtypes
    kinds = set(rb["cls_kind"].tolist())
    assert {C.AK["TAKE_REWARD_GOLD"], C.AK["PROCEED_REWARDS"]} <= kinds, kinds
    opt = (rb["tok_flags"] & np.uint8(C.TF_OPTION)) != 0
    reward = opt & (rb["sec"] == C.REWARD_SCREEN_SECTION)
    assert reward.any()
    kind_col = rb["cat"][:, C.XC["X_KIND"]]
    gold = reward & (kind_col == C.AK["TAKE_REWARD_GOLD"])
    assert (rb["cont"][gold, C.XF["O_PRICE"]] > 0).all()  # the amount
    has_id = reward & np.isin(
        kind_col, [C.AK[n] for n in ("TAKE_REWARD_POTION", "TAKE_REWARD_RELIC")]
    )
    assert (rb["cat"][has_id, C.XC["X_ID_MAIN"]] != 0).all()
    # The fill check ties the section, kinds and types to the declaration.
    wp = list(FC.wheel_filled())
    assert C.FILL_REWARD_SCREEN in FC.check(rb, wp).stray
    assert C.FILL_REWARD_SCREEN not in FC.check(rb, [*wp, C.FILL_REWARD_SCREEN]).stray


def test_an_undeclared_net_keeps_the_screen_inputs_at_zero() -> None:
    old, new = N.layout(), N.layout([C.FILL_REWARD_SCREEN])
    assert C.REWARD_SCREEN_SECTION in old.reserved_sections
    assert C.REWARD_SCREEN_SECTION not in new.reserved_sections
    screen = C.REWARD_FILL_KINDS
    assert not screen & set(old.live_kinds) and set(new.live_kinds) >= screen
    types = C.REWARD_SCREEN_DTYPES
    assert not types & set(old.live_dtypes) and set(new.live_dtypes) >= types


def test_lockstep_declared(monkeypatch: pytest.MonkeyPatch) -> None:
    """The Rust training env's rows equal the Python env's on runs with the screen."""
    monkeypatch.setenv(factory.REWARD_SCREEN_ENV, "1")
    ls = Lockstep(f9_p=0.0)
    run_seeds(ls, SEED, 8, pinned=False, practice=0)
    assert ls.stats["rows"] > 150, dict(ls.stats)
    assert ls.mismatches == 0, {"stats": dict(ls.stats), "examples": ls.examples[:5]}
