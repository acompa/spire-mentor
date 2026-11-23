"""The version factory: the only construction site for e2e campaign runs.

Plan section 3.1 (WP1, GH #1097). Every run of an end-to-end lane is built
here, on the lane build (`ppo.e2e.build`: v0.111.0 unless
`STS2_E2E_GAME_VERSION` names another, GH #1535), in agent mode, in a process
started with `STS2_GAME_VERSION` naming the same build:

    rm = new_run(seed)                     # a fresh run, checked
    check_run(rm)                          # any run the env did not build (a clone)
    cs = check_fight(rm.combat_state())    # at every fight start
    cs = scenario_state(seed, ...)         # a scenario fight, on the lane build

Rust never reads `STS2_GAME_VERSION`, so a `RunManager(...)` or
`build_scenario_state(...)` call without `game_version=` silently simulates
v0.107.1. Each helper names the build explicitly and then reads it back from
the engine object. The Python side does read the selection (the combat
context's static tables, `PpoRunEnv`'s default build, the fight builders), so
every helper also requires this process to select the lane build
(`require_selection`). A mismatch raises `sts2_client.GameVersionMismatch` and
any other broken invariant an `AssertionError`. Neither is a `ValueError`,
on purpose: fight builders read a `ValueError` as "this row does not build"
and skip the row. The checks are explicit raises, not `assert` statements, so
`python -O` keeps them.

`tests/e2e/test_version_guard.py` fails on any engine call without
`game_version=` in this package, `ppo/combat_ppo/`, `ppo/rollout_starts.py`
and `ppo/combat_bench/`.
"""

from __future__ import annotations

import os
from collections.abc import Collection
from typing import Any

import sts2_client

from . import build as _build
from . import contracts as C

GAME_VERSION = _build.GAME_VERSION
# The campaign's character. A process plays one: `STS2_E2E_CHARACTER` names it
# before the process starts (spawned workers inherit it, as with the game
# version); unset, it is Ironclad. Another character's rows carry the "chars"
# fill event (GH #1373, SCHEMA.md section 13), so its learner declares
# `--filled` with "chars" and `--character`. An all-character lane (`--mix`,
# `ppo.e2e.mix`, GH #1197) instead names each run's character to `new_run` and the
# lane's set to `check_run`.
CHARACTER_ENV = "STS2_E2E_CHARACTER"
# The lane build's roster (`build.characters`): The Fool from v0.113.0 on.
CHARACTERS = _build.CHARACTERS


def _character() -> str:
    raw = os.environ.get(CHARACTER_ENV, "").strip()
    if not raw:
        return CHARACTERS[0]
    by_lower = {c.lower(): c for c in CHARACTERS}
    if raw.lower() not in by_lower:
        raise ValueError(f"{CHARACTER_ENV}={raw!r}: not one of {CHARACTERS}")
    return by_lower[raw.lower()]


CHARACTER = _character()

# Whether this process's Ironclad rows carry the "chars" fill event (orbs and
# Doom on an Ironclad run). Another character's rows always do. The learner
# sets it from `--filled` before it spawns its workers; unset, Ironclad rows
# are the frozen schema's.
CHARS_ENV = "STS2_E2E_CHARS"


def fill_chars() -> bool:
    """Whether a row of this process's `CHARACTER` carries the "chars" event."""
    return CHARACTERS[0] != CHARACTER or os.environ.get(CHARS_ENV, "") == "1"


def set_fill_chars(on: bool) -> None:
    """Declare (or drop) the "chars" event for this process and its children."""
    if on:
        os.environ[CHARS_ENV] = "1"
    else:
        os.environ.pop(CHARS_ENV, None)


# Whether this process's runs offer the potion popup (the "ooc-potion" fill event,
# GH #1606, SCHEMA.md section 14): drink a potion out of combat, discard one. The
# learner sets it from `--filled` before it spawns its workers; unset, the runs
# offer neither and their rows are the frozen schema's.
OOC_POTION_ENV = "STS2_E2E_OOC_POTION"


def fill_ooc_potion() -> bool:
    """Whether this process's runs carry the "ooc-potion" event."""
    return os.environ.get(OOC_POTION_ENV, "") == "1"


def set_fill_ooc_potion(on: bool) -> None:
    """Declare (or drop) the "ooc-potion" event for this process and its children."""
    if on:
        os.environ[OOC_POTION_ENV] = "1"
    else:
        os.environ.pop(OOC_POTION_ENV, None)


# Whether this process's runs offer the game's reward screen (the "#1069" fill
# event, GH #1620, SCHEMA.md section 17): take or skip each reward of a won combat
# room, in any order, and the chest relic's skip. The learner sets it from
# `--filled` before it spawns its workers; unset, the runs auto-collect every
# reward and their rows are the frozen schema's.
REWARD_SCREEN_ENV = C.REWARD_SCREEN_ENV


def fill_reward_screen() -> bool:
    """Whether this process's runs carry the "#1069" (reward screen) event."""
    return C.reward_screen_declared()


def set_fill_reward_screen(on: bool) -> None:
    """Declare (or drop) the reward screen event for this process and its children."""
    if on:
        os.environ[REWARD_SCREEN_ENV] = "1"
    else:
        os.environ.pop(REWARD_SCREEN_ENV, None)


# Whether this process's runs play the game's deck select screens (the
# "deck-select" fill event, GH #1790, SCHEMA.md section 16): a pick-N deck select
# applies on its last pick, and the picks wait on the deck as it was. The learner
# sets it from `--filled` before it spawns its workers; unset, each pick applies
# at once and the rows are the frozen schema's.
DECK_SELECT_ENV = C.DECK_SELECT_ENV


def fill_deck_select() -> bool:
    """Whether this process's runs carry the "deck-select" event."""
    return C.deck_select_declared()


def set_fill_deck_select(on: bool) -> None:
    """Declare (or drop) the deck select event for this process and its children."""
    if on:
        os.environ[DECK_SELECT_ENV] = "1"
    else:
        os.environ.pop(DECK_SELECT_ENV, None)


ASCENSION = 10
MODE = "agent"
ACT1 = "roll"
# `RunManager` takes an i32 seed; campaign seeds are non-negative.
SEED_LIMIT = 2**31


def require_selection() -> None:
    """Raise unless this process selects the lane build (`GAME_VERSION`).

    Rust ignores the selection, but the Python side reads it: the combat
    context's static tables (`static_tables.card_types()`), `PpoRunEnv`'s
    default build, and the `rollout_starts` and `combat_bench` fights. A
    process on another selection would build lane-build runs here and encode
    them with another build's tables.

    Raises:
        sts2_client.GameVersionMismatch: The process selects another build.
    """
    if sts2_client.GAME_VERSION != GAME_VERSION:
        raise sts2_client.GameVersionMismatch(
            f"this process selects {sts2_client.GAME_VERSION}, the campaign needs "
            f"{GAME_VERSION}: set {sts2_client.GAME_VERSION_ENV}={GAME_VERSION} before "
            "the process starts (spawned workers inherit it) or call "
            "sts2_client.set_game_version"
        )


def new_run(
    seed: int, *, character: str | None = None, interactive_potion_swap: bool = True
) -> Any:
    """A fresh campaign run: `character` (None: the process's `CHARACTER`, Ironclad
    unless `STS2_E2E_CHARACTER` names another) at A10 on the lane build, agent mode, from
    Neow, act 1 rolled from the seed.

    Args:
        seed: The run seed, in [0, 2**31).
        character: The run's character, one of `CHARACTERS` (a mix lane draws it per
            run, `ppo.e2e.mix`); None is the process's.
        interactive_potion_swap: Whether a potion reward on a full belt
            surfaces a `potion_swap` decision. The student plays with it on
            (D4); an R1-style arm passes False.

    Raises:
        sts2_client.GameVersionMismatch: The process selects another build
            (`require_selection`), or the engine built one.
        AssertionError: The seed is out of range, or the engine built another
            kind of run (`check_run`), or `character` is not one of `CHARACTERS`.
    """
    import sts2_rs

    require_selection()
    if isinstance(seed, bool) or not isinstance(seed, int) or not 0 <= seed < SEED_LIMIT:
        raise AssertionError(f"seed {seed!r} outside [0, 2**31)")
    char = CHARACTER if character is None else str(character)
    if char not in CHARACTERS:
        raise AssertionError(f"character {char!r} not one of {CHARACTERS}")
    rm = sts2_rs.RunManager(
        seed,
        character=char,
        ascension_level=ASCENSION,
        neow_start=True,
        act1=ACT1,
        game_version=GAME_VERSION,
        mode=MODE,
        interactive_potion_swap=interactive_potion_swap,
        ooc_potion_actions=fill_ooc_potion(),
        reward_screen=fill_reward_screen(),
        deck_select_screens=fill_deck_select(),
    )
    return check_run(rm, characters=(char,), interactive_potion_swap=interactive_potion_swap)


def check_run(
    rm: Any, *, characters: Collection[str] | None = None, interactive_potion_swap: bool = True
) -> Any:
    """Return `rm` once it reads back as a campaign run.

    Checks the build (`GAME_VERSION`), the mode (agent), the potion-swap flag, the
    character, the ascension, the potion popup ("ooc-potion"), the reward
    screen ("#1069") and the deck select screens ("deck-select"). Call it on
    every run the env holds that
    `new_run` did not just build: a clone, a practice snapshot, a run another
    module constructed. `characters` are the ones the lane plays (a mix lane's
    set, `ppo.e2e.mix`); None is the process's `CHARACTER` alone.

    Raises:
        sts2_client.GameVersionMismatch: `rm` simulates another build, or the
            process selects one (`require_selection`).
        AssertionError: `rm` differs in mode, flag, character or ascension.
    """
    require_selection()
    sts2_client.check_game_version(rm, GAME_VERSION, what="RunManager")
    lane = (CHARACTER,) if characters is None else tuple(characters)
    got = (rm.mode, rm.interactive_potion_swap, rm.character, rm.ascension_level)
    if got[:2] != (MODE, interactive_potion_swap) or got[3] != ASCENSION or got[2] not in lane:
        want = (MODE, interactive_potion_swap, lane[0] if len(lane) == 1 else lane, ASCENSION)
        raise AssertionError(
            "RunManager is not a campaign run: (mode, interactive_potion_swap, "
            f"character, ascension_level) = {got}, expected {want}"
        )
    if bool(rm.ooc_potion_actions) != fill_ooc_potion():
        raise AssertionError(
            f"RunManager ooc_potion_actions={rm.ooc_potion_actions}, the process "
            f'{"declares" if fill_ooc_potion() else "does not declare"} the "ooc-potion" '
            "fill event (factory.fill_ooc_potion)"
        )
    if bool(getattr(rm, "reward_screen", False)) != fill_reward_screen():
        raise AssertionError(
            f"RunManager reward_screen={getattr(rm, 'reward_screen', False)}, the process "
            f'{"declares" if fill_reward_screen() else "does not declare"} the "#1069" '
            "(reward screen) fill event (factory.fill_reward_screen)"
        )
    if bool(getattr(rm, "deck_select_screens", False)) != fill_deck_select():
        raise AssertionError(
            f"RunManager deck_select_screens={getattr(rm, 'deck_select_screens', False)}, "
            f"the process {'declares' if fill_deck_select() else 'does not declare'} the "
            '"deck-select" fill event (factory.fill_deck_select)'
        )
    return rm


def check_fight(cs: Any, *, what: str = "combat state") -> Any:
    """Return `cs` once it simulates the lane build: the fight-start check.

    Reads the `CombatState.game_version` getter, so a state from an
    unversioned `build_scenario_state` call or from another build's run fails here
    instead of playing the old build's cards and monsters.

    Raises:
        sts2_client.GameVersionMismatch: `cs` simulates another build (or is
            not a combat state at all, such as None outside combat), or the
            process selects one (`require_selection`).
    """
    require_selection()
    return sts2_client.check_game_version(cs, GAME_VERSION, what=what)


def scenario_state(*args: Any, game_version: str = GAME_VERSION, **kwargs: Any) -> Any:
    """`sts2_rs.build_scenario_state(*args, **kwargs)` on the lane build, checked.

    Takes `build_scenario_state`'s own arguments: seed, character, deck, hp,
    max_hp, relics, encounter, then its keywords.

    Raises:
        sts2_client.GameVersionMismatch: `game_version` names another build
            (this helper builds the lane build only), the engine built one, or the
            process selects one (`require_selection`).
    """
    import sts2_rs

    require_selection()
    # Not `normalize_game_version`: its ValueError on an unknown build is the
    # exception fight builders skip on.
    if str(game_version).removeprefix("v") != GAME_VERSION.removeprefix("v"):
        raise sts2_client.GameVersionMismatch(
            f"scenario_state builds {GAME_VERSION} only, not {game_version!r}"
        )
    cs = sts2_rs.build_scenario_state(*args, game_version=GAME_VERSION, **kwargs)
    return check_fight(cs, what="scenario state")
