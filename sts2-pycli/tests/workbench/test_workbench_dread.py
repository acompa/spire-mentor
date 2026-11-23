"""Dread mode in the workbench (GH #1346).

The wire contract shared with the Rust ``sts2-workbench`` port:

* ``/api/live/start`` takes an optional ``game_mode`` (``"standard"``
  default, or ``"dread"``). A Dread run is an ascension-10 run on v0.113.0 on
  the Rust engine; any other combination is a 400 that says why.
* The live state carries ``game_mode`` and ``dominion`` (the dominion's model
  entry, or ``None``) in a Dread run, and neither key in a standard one.
* Saves and the run-log meta carry ``game_mode`` only off standard; a file
  without the key is a standard run.
"""

from __future__ import annotations

import pytest
from fake_game import FakeGame  # shared stub from tests/workbench/fake_game.py
from fastapi.testclient import TestClient
from workbench import saves as saves_mod
from workbench.live_runs import RunIdentity
from workbench.server import create_app
from workbench.timeline import TimelineStore

V111 = "v0.111.0"
V113 = "v0.113.0"


def _client(tmp_path, monkeypatch=None) -> TestClient:
    if monkeypatch is not None:
        from workbench import branching

        monkeypatch.setattr(branching, "Game", FakeGame)
    images = tmp_path / "images"
    images.mkdir(exist_ok=True)
    logs = tmp_path / "logs"
    logs.mkdir(exist_ok=True)
    return TestClient(create_app(images_root=images, logs_dir=logs))


def _rust() -> None:
    pytest.importorskip("sts2_rs")


def _body(**extra) -> dict:
    return {
        "character": "Ironclad",
        "seed": "DREAD",
        "backend": "rust",
        "act1": "overgrowth",
        "game_version": V113,
        **extra,
    }


def _start(c: TestClient, **extra) -> dict:
    r = c.post("/api/live/start", json=_body(**extra))
    assert r.status_code == 200, r.text
    return r.json()


def _first_enabled_action(c: TestClient, step: dict) -> dict:
    """Take the first enabled option (a card select picks card 0; a screen
    with nothing to pick proceeds), as the Rust twin's ``advance`` does."""
    if step["decision"] == "card_select":
        action, args = "select_cards", {"indices": "0"}
    else:
        opts = [o for o in step["state"].get("options") or [] if o.get("enabled", True)]
        action, args = (opts[0]["action"], opts[0]["args"]) if opts else ("proceed", {})
    r = c.post("/api/live/action", json={"action": action, "args": args})
    assert r.status_code == 200, r.text
    assert r.json()["error"] is None, r.json()["error"]
    return r.json()["step"]


# ── validation ──────────────────────────────────────────────────────────────


@pytest.mark.parametrize(
    ("route", "body", "needle"),
    [
        ("/api/live/start", _body(game_mode="dread", game_version=V111), V113),
        ("/api/live/start", _body(game_mode="dread", game_version="v0.107.1"), V113),
        ("/api/live/start", _body(game_mode="nightmare"), "unknown game_mode"),
        (
            "/api/live/start-faithful",
            {"character": "Ironclad", "seed": "run_1", "game_version": V113, "game_mode": "dread"},
            "Dread",
        ),
        (
            "/api/live/import-state",
            {"payload": "{}", "game_version": V113, "game_mode": "dread"},
            "Dread",
        ),
    ],
)
def test_a_dread_start_the_engine_cannot_build_is_a_400(tmp_path, route, body, needle):
    c = _client(tmp_path)
    r = c.post(route, json=body)
    assert r.status_code == 400, r.text
    assert needle in r.json()["detail"]
    # Nothing was started.
    assert c.get("/api/live/state").status_code != 200


def test_dread_on_the_csharp_engine_is_a_400(tmp_path, monkeypatch):
    import sts2_client

    c = _client(tmp_path, monkeypatch)
    r = c.post(
        "/api/live/start",
        json=_body(backend="csharp", game_version=sts2_client.GAME_VERSION, game_mode="dread"),
    )
    assert r.status_code == 400, r.text
    assert "rust" in r.json()["detail"]


# ── payload defaults ────────────────────────────────────────────────────────


def test_a_file_without_the_key_is_a_standard_run():
    base = {"character": "Ironclad", "seed": "S", "ascension": 10}
    assert RunIdentity.from_save(base).game_mode == "standard"
    assert RunIdentity.from_save({**base, "game_mode": "dread"}).game_mode == "dread"
    rec = {**base, "backend": "rust", "act1": "roll", "run_mode": False}
    assert RunIdentity.from_recording(rec).game_mode == "standard"
    assert RunIdentity.from_recording({**rec, "game_mode": "dread"}).game_mode == "dread"


def test_write_save_names_the_mode_only_off_standard(tmp_path):
    kwargs = {
        "character": "Ironclad",
        "seed": "S",
        "ascension": 10,
        "backend": "rust",
        "run_mode": False,
        "progress": {},
        "line": [],
        "game_version": V113,
    }
    saves_mod.write_save(tmp_path, name="std", **kwargs)
    assert "game_mode" not in saves_mod.read_save(tmp_path, "std")
    saves_mod.write_save(tmp_path, name="dread", game_mode="dread", **kwargs)
    assert saves_mod.read_save(tmp_path, "dread")["game_mode"] == "dread"


# ── the engine ──────────────────────────────────────────────────────────────


def test_a_dread_start_is_an_ascension_10_run_that_names_its_dominion(tmp_path):
    _rust()
    c = _client(tmp_path)
    # The requested ascension is overridden, as on the game's Dread screen.
    step = _start(c, game_mode="dread", ascension=3)
    state = step["state"]
    assert state["game_mode"] == "dread"
    assert state["dominion"].endswith("_DOMINION")
    assert state["ascension"] == 10
    assert state["game_version"] == V113


@pytest.mark.parametrize("extra", [{}, {"game_mode": "standard"}, {"game_mode": None}])
def test_a_standard_run_names_neither_key(tmp_path, extra):
    _rust()
    c = _client(tmp_path)
    step = _start(c, ascension=10, **extra)
    assert "game_mode" not in step["state"]
    assert "dominion" not in step["state"]
    assert c.post("/api/live/save", json={"name": "std"}).status_code == 200
    payload = saves_mod.read_save(saves_mod.save_dir(tmp_path / "logs"), "std")
    assert payload is not None and "game_mode" not in payload
    log_path = c.post("/api/live/quit").json()["log_path"]
    ident = TimelineStore.load_jsonl(log_path).run_identity()
    assert ident is not None and "game_mode" not in ident


def test_save_seek_and_run_log_keep_a_dread_run(tmp_path):
    _rust()
    c = _client(tmp_path)
    step = _start(c, game_mode="dread")
    root_seed = step["state"]["bg_seed"]
    for _ in range(3):
        step = _first_enabled_action(c, step)
    before = c.get("/api/live/state").json()["state"]
    assert before["game_mode"] == "dread"

    # Seek to the root rebuilds a Dread run.
    tl = c.get("/api/live/timeline").json()
    r = c.post("/api/live/seek", json={"node_id": tl["nodes"][0]["id"]})
    assert r.status_code == 200, r.text
    root = r.json()["step"]["state"]
    assert root["game_mode"] == "dread" and root["ascension"] == 10
    assert root["bg_seed"] == root_seed
    r = c.post("/api/live/seek", json={"node_id": tl["active_leaf"]})
    assert r.status_code == 200, r.text

    # Save -> restore.
    assert c.post("/api/live/save", json={"name": "dread mid"}).status_code == 200
    payload = saves_mod.read_save(saves_mod.save_dir(tmp_path / "logs"), "dread mid")
    assert payload is not None
    assert payload["game_mode"] == "dread" and payload["ascension"] == 10
    _start(c)  # replace the live run with a standard one
    r = c.post("/api/saves/load", json={"name": "dread mid"})
    assert r.status_code == 200, r.text
    restored = r.json()["state"]
    assert restored["game_mode"] == "dread"
    assert restored["dominion"] == before["dominion"]
    assert restored["floor"] == before["floor"]
    assert restored["player"] == before["player"]

    # Run log meta -> "play from here".
    log_path = c.post("/api/live/quit").json()["log_path"]
    ident = TimelineStore.load_jsonl(log_path).run_identity()
    assert ident is not None and ident["game_mode"] == "dread"
    assert c.post("/api/load", json={"path": log_path}).status_code == 200
    assert c.get("/api/step/1").json()["state"]["game_mode"] == "dread"
    r = c.post("/api/live/from_replay", json={"idx": 1})
    assert r.status_code == 200, r.text
    assert r.json()["state"]["game_mode"] == "dread"
