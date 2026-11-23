#!/usr/bin/env python3
"""Play seeded Blitz runs on the C# oracle alone and report what happened.

GH #1347, stage 1. The Rust engine does not know Blitz yet, so there is no
lockstep to run (``run_parity.py --game-mode blitz`` says so and stops). This
driver answers the question stage 1 has to answer by itself: does the C# host
play a Blitz run from Neow to the end, through every merchant ware, without a
crash, a hang or a stuck decision?

The policy is not trying to win. Per seed it is deterministic, and across seeds
it varies what it takes: the merchant's wares in a seeded order (the cursed
ware first on every other seed), a cancelled deck pick or skipped card reward
now and then, the chest's two relics in either order.

``--keep-alive`` tops the player's HP up through the oracle's ``set_player``
debug command, so runs reach acts 2 and 3. That is a cheat, not gameplay: it is
for exercising the host's later rooms, and the report says when it was on. It
stops after 60 rounds of one fight, so a fight the policy cannot win still ends.

Usage::

    uv run --frozen python python/blitz_playthrough.py --seeds 4 --keep-alive \\
        --out ../logs/2026-11-14-blitz/playthrough
"""

from __future__ import annotations

import argparse
import concurrent.futures as cf
import json
import os
import random
import sys
import time
from collections import Counter
from pathlib import Path
from typing import Any

import sts2_client
from sts2_client import Game, add_game_version_arg, apply_game_version_arg

CHARACTERS = ("Ironclad", "Silent", "Defect", "Regent", "Necrobinder")

#: `BlitzMerchantEntryType`, in enum order; the last six are the cursed kinds.
WARE_KINDS = (
    "CardUpgrade",
    "UpgradedCardReward",
    "RandomUpgrades",
    "CardTransform",
    "CardRemoval",
    "CardRewards",
    "ColorlessCardReward",
    "Potions",
    "CardEnchant",
    "EnchantedCardReward",
    "PrismaticCardReward",
    "RareCardReward",
    "UpgradedRareCard",
    "Relic",
    "Glam",
    "TwoMoreChoices",
    "CardRewardOfEachRarity",
    "UpgradedRareCardReward",
    "TwoRelics",
    "FullHealAndMaxHp",
)

MAX_STEPS = 12000
#: --keep-alive stops topping HP up after this many rounds of one fight.
KEEP_ALIVE_ROUNDS = 60
#: The same decision, unchanged, this many times in a row is a stuck run.
STUCK_REPEATS = 60


class Playthrough:
    """One seeded Blitz run: the policy and the record of what it met."""

    def __init__(self, game: Game, seed: str, index: int, keep_alive: bool) -> None:
        self.game = game
        self.rng = random.Random(f"blitz:{seed}")
        self.cursed_first = index % 2 == 0
        self.keep_alive = keep_alive
        self.shops: list[dict[str, Any]] = []
        self.chests: list[dict[str, Any]] = []
        self.follow_ups: Counter[str] = Counter()
        self.map_choice_counts: Counter[int] = Counter()
        self._shop: dict[str, Any] | None = None
        self._pending_ware: tuple[int, str, int] | None = None

    # ── one decision ─────────────────────────────────────────────────────────

    def step(self, state: dict[str, Any]) -> dict[str, Any]:
        dec = state.get("decision")
        if dec == "map_select":
            self._shop = None  # a ware's follow-up screens belong to its shop
        if self._pending_ware is not None and dec != "shop":
            self.follow_ups[f"{self._pending_ware[1]}:{dec}"] += 1
        handler = getattr(self, f"_on_{dec}", None)
        if handler is None:
            raise RuntimeError(f"no policy for decision {dec!r}")
        return handler(state)

    def _on_map_select(self, state: dict[str, Any]) -> dict[str, Any]:
        choices = state.get("choices") or []
        self.map_choice_counts[len(choices)] += 1
        if not choices:
            raise RuntimeError("map_select with no choices")
        choice = choices[0]
        return self.game.select_map_node(choice["col"], choice["row"])

    def _on_combat_play(self, state: dict[str, Any]) -> dict[str, Any]:
        # Past KEEP_ALIVE_ROUNDS the fight is a stalemate this policy cannot
        # break (an immortal player whose few cards do no damage); stop topping
        # up so it ends in a death instead of running to the step cap.
        in_time = int(state.get("round") or 0) <= KEEP_ALIVE_ROUNDS
        if self.keep_alive and in_time and state["player"]["hp"] < 500:
            self.game.set_player(hp=999, max_hp=999)
        actions = self.game.legal_actions().get("actions") or []
        plays = [a for a in actions if a.get("action") == "play_card"]
        if not plays:
            return self.game.end_turn()
        play = plays[0]
        return self.game.play_card(play["card_index"], play.get("target_index"))

    def _on_event_choice(self, state: dict[str, Any]) -> dict[str, Any]:
        options = [o for o in state.get("options") or [] if not o.get("is_locked")]
        if not options:
            return self.game.leave_room()
        return self.game.choose_option(self.rng.choice(options)["index"])

    def _on_rest_site(self, state: dict[str, Any]) -> dict[str, Any]:
        options = [o for o in state.get("options") or [] if o.get("is_enabled", True)]
        if not options:
            return self.game.leave_room()
        return self.game.choose_option(self.rng.choice(options)["index"])

    def _on_card_select(self, state: dict[str, Any]) -> dict[str, Any]:
        cards = state.get("cards") or []
        low = int(state.get("min_select") or 0)
        cancel_ok = bool(state.get("cancelable")) or low == 0
        if not cards or (cancel_ok and self.rng.random() < 0.15):
            return self.game.skip_select()
        want = min(max(low, 1), len(cards))
        picked = self.rng.sample([c["index"] for c in cards], want)
        return self.game.select_cards(",".join(str(i) for i in sorted(picked)))

    def _on_card_reward(self, state: dict[str, Any]) -> dict[str, Any]:
        cards = state.get("cards") or []
        if not cards or (state.get("can_skip", True) and self.rng.random() < 0.15):
            return self.game.skip_card_reward()
        return self.game.select_card_reward(self.rng.choice(cards)["index"])

    def _on_bundle_select(self, state: dict[str, Any]) -> dict[str, Any]:
        return self.game.select_bundle(self.rng.choice(state["bundles"])["index"])

    def _on_potion_swap(self, state: dict[str, Any]) -> dict[str, Any]:
        option = self.rng.choice(state["options"])
        return self.game.act("potion_swap", discard_slot=option.get("discard_slot"))

    def _on_rewards(self, state: dict[str, Any]) -> dict[str, Any]:
        rewards = state["rewards"]
        if "chest_picks_left" in state:
            if state["chest_picks_left"] == len(rewards) == 2:
                self.chests.append(
                    {"act": state["context"]["act"], "offered": [r["id"] for r in rewards]}
                )
                self.chests[-1]["taken"] = []
            chest = self.chests[-1]
            if self.rng.random() < 0.1:
                chest["skipped"] = True
                return self.game.act("skip_reward", reward_index=rewards[0]["index"])
            reward = self.rng.choice(rewards)
            chest["taken"].append(reward["id"])
            return self.game.act("take_reward", reward_index=reward["index"])
        # --interactive-rewards: a fight's or a ware's reward screen.
        if self.rng.random() < 0.1:
            return self.game.proceed()
        return self.game.act("take_reward", reward_index=self.rng.choice(rewards)["index"])

    def _on_shop(self, state: dict[str, Any]) -> dict[str, Any]:
        wares = [o for o in state.get("options") or [] if o.get("action") == "blitz_choice"]
        if "blitz_choices_remaining" not in state or len(wares) != 6:
            raise RuntimeError(f"not a Blitz shop: {sorted(state)}")
        left = int(state["blitz_choices_remaining"])
        if self._shop is None:
            order = [w["index"] for w in wares]
            self.rng.shuffle(order)
            if self.cursed_first:
                cursed = [w["index"] for w in wares if w["item"]["cursed"]]
                order = cursed + [i for i in order if i not in cursed]
            self._shop = {
                "act": state["context"]["act"],
                "offered": [w["item"]["blitz_kind"] for w in wares],
                "taken": [],
                "declined": [],
                "order": order,
            }
            self.shops.append(self._shop)
        shop = self._shop
        by_index = {w["index"]: w for w in wares}

        if self._pending_ware is not None:
            index, kind, left_before = self._pending_ware
            self._pending_ware = None
            if by_index[index].get("status") == "Used":
                shop["taken"].append(kind)
                expected = left_before + 1 if kind == "TwoMoreChoices" else left_before - 1
                if left != expected:
                    raise RuntimeError(
                        f"{kind}: picks {left_before} -> {left}, expected {expected}"
                    )
            else:
                # A cancelled deck pick, or RandomUpgrades with nothing to upgrade.
                shop["declined"].append(kind)
                if left != left_before:
                    raise RuntimeError(f"{kind} was not taken but picks {left_before} -> {left}")

        while shop["order"]:
            index = shop["order"].pop(0)
            ware = by_index[index]
            if left <= 0 or not ware.get("enabled"):
                continue
            kind = ware["item"]["blitz_kind"]
            self._pending_ware = (index, kind, left)
            return self.game.blitz_choice(index)
        if left > 0 and any(w.get("enabled") for w in wares) and not shop["declined"]:
            raise RuntimeError(f"leaving with {left} picks and wares still enabled")
        if left <= 0 and any(w.get("enabled") for w in wares):
            raise RuntimeError("no picks left but a ware is still enabled")
        return self.game.act("leave_shop")


def _signature(state: dict[str, Any]) -> str:
    player = state.get("player") or {}
    ctx = state.get("context") or {}
    return json.dumps(
        [
            state.get("decision"),
            ctx.get("act"),
            ctx.get("floor"),
            state.get("round"),
            state.get("energy"),
            player.get("hp"),
            player.get("deck_size"),
            len(state.get("hand") or []),
            sum(e.get("hp", 0) for e in state.get("enemies") or []),
            state.get("blitz_choices_remaining"),
            len(state.get("rewards") or []),
            len(state.get("cards") or []),
            len(state.get("options") or []),
        ]
    )


def play_one(
    character: str,
    seed: str,
    index: int,
    ascension: int,
    keep_alive: bool,
    game_version: str,
    interactive: bool = False,
) -> dict[str, Any]:
    """Play one run to its end; never raises (the outcome says what went wrong)."""
    sts2_client.set_game_version(game_version)
    if interactive:
        # Read by the oracle at start-up (Referee.InteractiveRewards).
        os.environ["STS2_INTERACTIVE_REWARDS"] = "1"
    result: dict[str, Any] = {
        "character": character,
        "seed": seed,
        "ascension": ascension,
        "keep_alive": keep_alive,
        "interactive_rewards": interactive,
        "outcome": "error",
        "steps": 0,
    }
    started = time.monotonic()
    game = Game()
    run = Playthrough(game, seed, index, keep_alive)
    state: dict[str, Any] = {}
    try:
        state = game.start(
            character=character,
            seed=seed,
            ascension=ascension,
            game_mode="blitz",
            interactive_potion_swap=interactive,
        )
        last, repeats = "", 0
        for step in range(MAX_STEPS):
            result["steps"] = step
            if state.get("type") == "error":
                result["error"] = state.get("message")
                break
            if state.get("decision") == "game_over":
                result["outcome"] = "won" if state.get("victory") else "died"
                break
            sig = _signature(state)
            repeats = repeats + 1 if sig == last else 0
            last = sig
            if repeats >= STUCK_REPEATS:
                result["outcome"] = "stuck"
                result["error"] = f"decision unchanged {repeats} times: {sig}"
                break
            before = state
            state = run.step(state)
            if state.get("type") == "error":
                result["error"] = (
                    f"{state.get('message')} (answering {before.get('decision')} "
                    f"in a {before.get('context', {}).get('room_type')} room)"
                )
                state = before
                break
        else:
            result["outcome"] = "stuck"
            result["error"] = f"no end within {MAX_STEPS} steps"
    except Exception as exc:
        result["error"] = f"{type(exc).__name__}: {exc}"
    finally:
        ctx = state.get("context") or {}
        result["act"] = ctx.get("act")
        result["floor"] = ctx.get("floor")
        result["total_floor"] = ctx.get("total_floor")
        result["last_decision"] = state.get("decision")
        result["shops"] = [{k: v for k, v in s.items() if k != "order"} for s in run.shops]
        result["chests"] = run.chests
        result["follow_ups"] = dict(run.follow_ups)
        result["map_choice_counts"] = dict(run.map_choice_counts)
        # Faults the host swallowed (HostFaults.cs) and exceptions the game
        # logged: a run can end "won" or "died" with either, so both are kept.
        try:
            faults = game.send({"cmd": "host_faults"})
            result["host_faults"] = faults.get("counts") or {}
            result["host_fault_samples"] = faults.get("samples") or {}
        except Exception as exc:
            result["host_faults"] = {"unavailable": str(exc)}
        try:
            result["world_errors"] = game.world_errors()
        except Exception as exc:
            result["world_errors"] = {"count": None, "error": str(exc)}
        game.close()
        result["seconds"] = round(time.monotonic() - started, 1)
    return result


def summarize(results: list[dict[str, Any]]) -> dict[str, Any]:
    offered: Counter[str] = Counter()
    taken: Counter[str] = Counter()
    declined: Counter[str] = Counter()
    follow_ups: Counter[str] = Counter()
    map_counts: Counter[str] = Counter()
    host_faults: Counter[str] = Counter()
    for r in results:
        host_faults.update(
            {k: v for k, v in r.get("host_faults", {}).items() if isinstance(v, int)}
        )
        for shop in r["shops"]:
            offered.update(shop["offered"])
            taken.update(shop["taken"])
            declined.update(shop["declined"])
        follow_ups.update(r["follow_ups"])
        map_counts.update({str(k): v for k, v in r["map_choice_counts"].items()})
    return {
        "runs": len(results),
        "outcomes": dict(Counter(r["outcome"] for r in results)),
        "finished": sum(r["outcome"] in ("won", "died") for r in results),
        "reached_act": dict(Counter(str(r.get("act")) for r in results)),
        "shops": sum(len(r["shops"]) for r in results),
        "chests": sum(len(r["chests"]) for r in results),
        "chests_both_taken": sum(len(c["taken"]) == 2 for r in results for c in r["chests"]),
        "wares_offered": {k: offered[k] for k in WARE_KINDS},
        "wares_taken": {k: taken[k] for k in WARE_KINDS},
        "wares_declined": {k: declined[k] for k in WARE_KINDS if declined[k]},
        "wares_never_taken": [k for k in WARE_KINDS if not taken[k]],
        "follow_ups": dict(sorted(follow_ups.items())),
        "map_select_choice_counts": dict(map_counts),
        "runs_with_world_errors": sum(
            bool((r.get("world_errors") or {}).get("count")) for r in results
        ),
        "world_error_samples": sorted(
            {line for r in results for line in (r.get("world_errors") or {}).get("sample") or []}
        )[:40],
        "host_faults": dict(sorted(host_faults.items())),
        "problems": [
            {k: r.get(k) for k in ("character", "seed", "outcome", "act", "floor", "error")}
            for r in results
            if r["outcome"] not in ("won", "died")
        ],
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--characters", default=",".join(CHARACTERS))
    ap.add_argument("--seeds", type=int, default=4, help="seeds per character")
    ap.add_argument("--seed-prefix", default="BLITZ")
    ap.add_argument("--ascension", type=int, default=0, help="the Blitz level, 0..5")
    ap.add_argument("--keep-alive", action="store_true", help="top HP up (a cheat; see above)")
    ap.add_argument(
        "--interactive-rewards",
        action="store_true",
        help="play every reward screen (STS2_INTERACTIVE_REWARDS=1) and full-belt potion "
        "swaps, instead of the host taking gold, relics and potions itself",
    )
    ap.add_argument("-j", "--jobs", type=int, default=4)
    ap.add_argument("--out", required=True, help="directory for runs.jsonl and summary.json")
    add_game_version_arg(ap)
    args = ap.parse_args()
    version = apply_game_version_arg(args)

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    jobs = [
        (
            character,
            f"{args.seed_prefix}{n}",
            n,
            args.ascension,
            args.keep_alive,
            version,
            args.interactive_rewards,
        )
        for character in args.characters.split(",")
        for n in range(args.seeds)
    ]
    results: list[dict[str, Any]] = []
    with (
        (out / "runs.jsonl").open("w", encoding="utf-8") as log,
        cf.ProcessPoolExecutor(max_workers=max(1, args.jobs)) as pool,
    ):
        for done, r in enumerate(cf.as_completed([pool.submit(play_one, *j) for j in jobs]), 1):
            result = r.result()
            results.append(result)
            log.write(json.dumps(result) + "\n")
            log.flush()
            print(
                f"[{done}/{len(jobs)}] {result['character']} {result['seed']}: "
                f"{result['outcome']} act {result.get('act')} floor {result.get('floor')} "
                f"({result['steps']} steps, {result['seconds']}s)"
                + (f" :: {result['error']}" if result.get("error") else ""),
                flush=True,
            )
    summary = summarize(results)
    summary["game_version"] = version
    summary["ascension"] = args.ascension
    summary["keep_alive"] = args.keep_alive
    summary["interactive_rewards"] = args.interactive_rewards
    (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(summary, indent=2))
    return 0 if not summary["problems"] else 1


if __name__ == "__main__":
    sys.exit(main())
