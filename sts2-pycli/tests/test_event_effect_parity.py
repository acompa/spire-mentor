"""Unit tests for the event-EFFECT parity comparison core (`event_parity`).

Pure-Python — no game engine, no wheel. These cover the effect-diff logic that
`effect_parity` applies to each resolved option (`_effect_fields` /
`_follow_options`): does it surface a missing potion / a no-op deck mutation / a
finish-vs-continue phase split / a multi-page follow-up divergence, and stay
silent on a faithful resolution. The live cross-engine resolution is exercised
by the harness itself (`event_parity.py --effect`), not here.
"""

from __future__ import annotations

from event_parity import _effect_fields, _follow_options
from run_parity import Snap


def _snap(
    *,
    phase: str = "map",
    hp: int = 80,
    max_hp: int = 80,
    gold: int = 99,
    deck: dict[str, int] | None = None,
    relics: list[str] | None = None,
    potions: list[str] | None = None,
) -> Snap:
    """A Snap with sensible defaults; deck_size is derived from the multiset."""
    deck = deck or {"STRIKE|0": 5}
    return Snap(
        phase=phase,
        hp=hp,
        max_hp=max_hp,
        block=0,
        gold=gold,
        deck=deck,
        deck_size=sum(deck.values()),
        relics=relics or ["BURNINGBLOOD"],
        potions=potions or [],
        enemies=(),
    )


def _view(options: list | None = None) -> dict:
    return {"options": options or []}


def test_faithful_resolution_has_no_fields() -> None:
    """Identical post-choice state on both engines → no divergence."""
    cs = _snap()
    rs = _snap()
    assert _effect_fields(cs, _view(), rs, _view()) == []


def test_missing_potion_is_flagged() -> None:
    """PotionCourier-shape: C# adds potions, Rust adds none."""
    cs = _snap(potions=["FOULPOTION", "FOULPOTION", "FOULPOTION"])
    rs = _snap(potions=[])
    fields = _effect_fields(cs, _view(), rs, _view())
    assert any("potions" in f for f in fields)


def test_noop_deck_mutation_is_flagged() -> None:
    """SpiritGrafter-shape: C# deck grows, Rust deck unchanged."""
    cs = _snap(deck={"STRIKE|0": 5, "DEFEND|0": 5, "BASH|0": 1})  # 11
    rs = _snap(deck={"STRIKE|0": 5, "DEFEND|0": 5})  # 10
    fields = _effect_fields(cs, _view(), rs, _view())
    assert any("deck" in f for f in fields)


def test_missing_relic_is_flagged() -> None:
    """WarHistorianRepy-shape: C# grants a relic Rust does not."""
    cs = _snap(relics=["BURNINGBLOOD", "VAMBRACE"])
    rs = _snap(relics=["BURNINGBLOOD"])
    fields = _effect_fields(cs, _view(), rs, _view())
    assert any("relics" in f for f in fields)


def test_finish_vs_continue_phase_split_is_flagged() -> None:
    """TinkerTime-shape: C# stays in the event (multi-page), Rust finishes."""
    cs = _snap(phase="event")
    rs = _snap(phase="map")
    fields = _effect_fields(cs, _view(options=[{"index": 0}]), rs, _view())
    assert any("phase" in f for f in fields)


def test_multipage_followup_option_divergence_is_flagged() -> None:
    """Both still in the event, but a chain branches to a different page."""
    cs = _snap(phase="event")
    rs = _snap(phase="event")
    cs_view = _view(options=[{"index": 0}, {"index": 1}])
    rs_view = _view(options=[{"index": 0}])
    fields = _effect_fields(cs, cs_view, rs, rs_view)
    assert any("followup_options" in f for f in fields)


def test_followup_count_only_compared_inside_event_phase() -> None:
    """Outside the event phase, a stray options list must NOT be compared."""
    cs = _snap(phase="map")
    rs = _snap(phase="map")
    cs_view = _view(options=[{"index": 0}, {"index": 1}])
    rs_view = _view(options=[])
    assert _effect_fields(cs, cs_view, rs, rs_view) == []


def test_follow_options_counts_and_tolerates_missing() -> None:
    assert _follow_options({"options": [{"index": 0}, {"index": 1}]}) == 2
    assert _follow_options({}) == 0
    assert _follow_options({"options": None}) == 0


# ─── the gated sweep's page walk (GH #1500) ─────────────────────────────────

import event_parity as ep  # noqa: E402


def _page(n: int, locked: tuple[int, ...] = ()) -> dict:
    return {"options": [{"text_key": f"O{i}", "is_locked": i in locked} for i in range(n)]}


def _fake_engines(monkeypatch, pages: dict, *, rs_override: dict | None = None) -> None:
    """``pages[path] = (phase, view)`` for both engines; ``rs_override`` swaps
    the Rust result of some paths."""
    rs_override = rs_override or {}
    monkeypatch.setattr(ep, "_rust_options", lambda *a: ep._view_options(pages[()][1]))
    monkeypatch.setattr(ep, "_cs_options", lambda *a: ep._view_options(pages[()][1]))

    def effect(side: str):
        def run(character, seed, asc, name, path):
            phase, view = (rs_override if side == "rs" else {}).get(path, pages[path])
            return _snap(phase=phase), view

        return run

    monkeypatch.setattr(ep, "_rust_effect", effect("rs"))
    monkeypatch.setattr(ep, "_cs_effect", effect("cs"))


def test_walk_descends_into_follow_up_pages(monkeypatch) -> None:
    pages = {
        (): ("event", _page(2)),
        (0,): ("event", _page(2, locked=(1,))),
        (1,): ("map", _view()),
        (0, 0): ("map", _view()),
    }
    _fake_engines(monkeypatch, pages)
    rows = ep.walk_event("Ironclad", "S", 0, "Ev", "EV")
    by = {r.key: r.status for r in rows}
    assert by == {"Ev[0]": "match", "Ev[1]": "match", "Ev[0/0]": "match", "Ev[0/1]": "skip:locked"}


def test_walk_reports_a_diff_and_does_not_descend_past_it(monkeypatch) -> None:
    pages = {(): ("event", _page(1)), (0,): ("event", _page(1)), (0, 0): ("map", _view())}
    _fake_engines(monkeypatch, pages, rs_override={(0,): ("map", _view())})
    rows = ep.walk_event("Ironclad", "S", 0, "Ev", "EV")
    assert [(r.key, r.status) for r in rows] == [("Ev[0]", "diff")]
    assert rows[0].failed


def test_walk_caps_depth_and_reports_it(monkeypatch) -> None:
    loop = ("event", _page(1))
    pages = {(): loop, (0,): loop, (0, 0): loop, (0, 0, 0): loop}
    _fake_engines(monkeypatch, pages)
    rows = ep.walk_event("Ironclad", "S", 0, "Ev", "EV", max_depth=3)
    assert [r.status for r in rows] == ["match", "match", "match", "truncated"]


def test_walk_fails_a_menu_mismatch(monkeypatch) -> None:
    monkeypatch.setattr(ep, "_rust_options", lambda *a: [("A", True)])
    monkeypatch.setattr(ep, "_cs_options", lambda *a: [("A", True), ("B", True)])
    rows = ep.walk_event("Ironclad", "S", 0, "Ev", "EV")
    assert [(r.status, r.failed) for r in rows] == [("menu", True)]


def test_known_diffs_are_allowlisted_not_failed(monkeypatch) -> None:
    pages = {(): ("event", _page(1)), (0,): ("map", _view())}
    _fake_engines(monkeypatch, pages, rs_override={(0,): ("event", _view())})
    monkeypatch.setitem(ep.KNOWN_EFFECT_DIFFS, ("Ev", (0,)), "oracle artifact")
    rows = ep.walk_event("Ironclad", "S", 0, "Ev", "EV")
    assert rows[0].status == "allowlisted"
    assert not rows[0].failed


def test_floor_fails_on_a_lost_path() -> None:
    rows = [ep.EffectRow("Ironclad", "S", "Ev", (0,), "A", "match")]
    assert ep.floor_problems(rows, ["Ev[0]"], "Ironclad") == []
    assert ep.floor_problems(rows, ["Ev[0]", "Ev[1]"], "Ironclad")


def test_the_fool_joins_the_roster_on_v0113() -> None:
    assert "Fool" not in ep.characters_for("v0.111.0")
    assert "Fool" in ep.characters_for("v0.113.0")


def test_an_event_no_path_exercised_fails_coverage(monkeypatch) -> None:
    import content_universe

    monkeypatch.setattr(content_universe, "events", lambda _v: ("A", "B", "C", "Neow"))
    rows = [
        ep.EffectRow("Ironclad", "S", "A", (0,), "X", "match"),
        ep.EffectRow("Ironclad", "S", "B", (), "<entry>", "skip:context"),
        ep.EffectRow("Ironclad", "S", "C", (0,), "X", "diff"),
    ]
    problems = ep.event_coverage_problems(rows, ["Ironclad"], "v0.111.0")
    assert problems == ["Ironclad: no path of ['C'] matched on both engines"]
