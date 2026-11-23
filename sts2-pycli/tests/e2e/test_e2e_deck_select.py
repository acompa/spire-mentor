"""The "deck-select" fill event (GH #1790, SCHEMA.md section 16): the game's deck select screens.

A run of a launch that declares the event plays a pick-N deck select as the game's
one select over the deck as it was when it opened: each pick takes a card off the
offer and the last applies them all (Morphic Grove and Trial transform both picks
after the second, `CardSelectCmd.FromDeckForTransformation(..., 2)`). The picked
cards are their own DECK tokens with K_PICKED, the options are the offer minus them,
and Smith's and Cook's cancel is never offered. A launch without the declaration
builds runs whose picks apply at once, so its rows stay byte-identical
(`test_e2e_chars.py::test_undeclared_ironclad_rows_unchanged`).
"""

from __future__ import annotations

import numpy as np
import pytest
import sts2_client
from ppo.e2e import contracts as C
from ppo.e2e import env as E
from ppo.e2e import factory
from ppo.e2e import fillcheck as FC
from ppo.e2e import tokens as TK
from ppo.e2e.rsenv_lockstep import Lockstep

SEED = 1_300_001_100  # the throwaway block (plan §3.1)
DECK = ["STRIKE_IRONCLAD", "DEFEND_IRONCLAD", "BASH", "ANGER", "CLASH", "HEADBUTT"]


@pytest.fixture(autouse=True, scope="module")
def _campaign_process():
    """The factory needs the process to select v0.111.0 (`STS2_GAME_VERSION`)."""
    with pytest.MonkeyPatch.context() as mp:
        mp.setattr(sts2_client, "GAME_VERSION", factory.GAME_VERSION)
        yield


@pytest.fixture
def declared(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv(factory.DECK_SELECT_ENV, "1")


def test_the_event_moves_no_hash() -> None:
    assert C.FILL_DECK_SELECT == "deck-select" and C.FILL_DECK_SELECT in C.FILL_EVENTS
    assert C.XF["K_PICKED"] in C.SECTION_LIVE[C.SEC["DECK"]][1]
    spec = C.schema_spec()
    assert C.XF["K_PICKED"] not in spec["section_live"]["DECK"][1]


def test_the_factory_builds_the_screens_only_when_declared(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.delenv(factory.DECK_SELECT_ENV, raising=False)
    rm = factory.new_run(SEED)
    assert not rm.deck_select_screens
    monkeypatch.setenv(factory.DECK_SELECT_ENV, "1")
    with pytest.raises(AssertionError, match="deck-select"):
        factory.check_run(rm)
    rm = factory.new_run(SEED)
    assert rm.deck_select_screens and factory.check_run(rm) is rm


def _transform_two() -> object:
    rm = factory.new_run(SEED)
    rm.debug_clear_pending_choices()
    assert rm.debug_set_master_deck(DECK) == []
    rm.debug_queue_deck_transforms(2, False)
    return rm


def _deck_tokens(tok: TK.Tok) -> tuple[np.ndarray, np.ndarray]:
    deck = tok.sec == C.SEC["DECK"]
    return tok.cat[deck, C.XC["X_ID_MAIN"]], tok.cont[deck, C.XF["K_PICKED"]]


def _option_ids(tok: TK.Tok) -> set[int]:
    return {int(tok.cat[t, C.XC["X_ID_MAIN"]]) for t in tok.cls_tok}


@pytest.mark.usefixtures("declared")
def test_a_transform_two_waits_for_its_second_pick() -> None:
    rm = _transform_two()
    deck = rm.master_deck_detailed()
    env = E.E2EEnv(SEED, rm=rm)
    d = env.reset()
    assert isinstance(d, E.E2EDecision) and d.dtype == C.DT["CARD_TRANSFORM"]
    t1 = TK.encode(d)
    assert len(t1.cls_tok) == len(DECK)
    ids, picked = _deck_tokens(t1)
    assert not picked.any()
    first = int(t1.cat[t1.cls_tok[0], C.XC["X_ID_MAIN"]])
    d = env.step(0)
    # The second pick: the deck is unchanged, the first card is marked and not offered.
    assert isinstance(d, E.E2EDecision) and d.dtype == C.DT["CARD_TRANSFORM"]
    assert env.rm.master_deck_detailed() == deck
    t2 = TK.encode(d)
    ids, picked = _deck_tokens(t2)
    assert len(ids) == len(DECK)
    marked = picked > 0.5
    assert int(marked.sum()) == 1 and int(ids[marked][0]) == first
    assert len(t2.cls_tok) == len(DECK) - 1 and first not in _option_ids(t2)
    assert len(env.rm.deck_select_picked) == 1
    d = env.step(0)
    # Both applied: no select open, no card marked.
    assert not env.rm.deck_select_picked and env.rm.pending_deck_transforms == 0
    if isinstance(d, E.E2EDecision) and not d.is_combat:
        assert not _deck_tokens(TK.encode(d))[1].any()


@pytest.mark.usefixtures("declared")
def test_smith_offers_no_cancel() -> None:
    rm = factory.new_run(SEED)
    rm.debug_clear_pending_choices()
    rm.debug_enter_rest_site([])
    smith = [oid for oid, _ in rm.rest_site_options()].index("SMITH")
    rm.take_action({"action": "rest_option", "idx": smith})
    assert rm.deck_select_cancelable
    assert {"action": "confirm_deck_selection"} in rm.available_actions()
    d = E.E2EEnv(SEED, rm=rm).reset()
    assert isinstance(d, E.E2EDecision)
    assert all(a.get("action") != "confirm_deck_selection" for a in d.actions), d.actions
    assert {a.get("action") for a in d.actions} == {"smith_card"}


@pytest.mark.usefixtures("declared")
def test_a_declared_segment_validates_and_the_fill_check_sees_the_mark() -> None:
    env = E.E2EEnv(SEED, rm=_transform_two(), cfg=E.EnvConfig(f9_p=0.0))
    act = FC.random_actor(SEED)
    toks, d = [], env.reset()
    while isinstance(d, E.E2EDecision):
        toks.append(TK.encode(d, priv=True))
        d = env.step(act(d))
    rb = TK.row_batch(toks, run_uid=[0] * len(toks), t=list(range(len(toks))))
    C.validate_row_batch(rb)
    assert ((rb["sec"] == C.SEC["DECK"]) & (rb["cont"][:, C.XF["K_PICKED"]] != 0)).sum() >= 1
    wp = list(FC.wheel_filled())
    assert C.FILL_DECK_SELECT in FC.check(rb, wp).stray
    assert C.FILL_DECK_SELECT not in FC.check(rb, [*wp, C.FILL_DECK_SELECT]).stray


@pytest.mark.usefixtures("declared")
@pytest.mark.parametrize("setup", ["transform", "upgrade", "removal", "smith"])
def test_lockstep_from_an_open_select(setup: str) -> None:
    """The Rust training env's rows equal the Python env's through a pick-N select."""
    rm = factory.new_run(SEED)
    rm.debug_clear_pending_choices()
    assert rm.debug_set_master_deck(DECK) == []
    if setup == "transform":
        rm.debug_queue_deck_transforms(2, False)
    elif setup == "upgrade":
        rm.debug_queue_deck_upgrades(3)
    elif setup == "removal":
        rm.debug_queue_deck_removals(2)
    else:
        rm.debug_enter_rest_site([])
        smith = [oid for oid, _ in rm.rest_site_options()].index("SMITH")
        rm.take_action({"action": "rest_option", "idx": smith})
    ls = Lockstep(f9_p=0.0)
    picks: list[int] = []

    def choose(d: object, k: int) -> int:
        n = len(d.members)  # type: ignore[attr-defined]
        c = (k * 7 + 3) % n
        picks.append(c)
        return c

    ls.segment(SEED, rm, E.RUN, choose, capture=False)
    assert ls.stats["rows"] > 20, dict(ls.stats)
    assert ls.mismatches == 0, {"stats": dict(ls.stats), "examples": ls.examples[:5]}
