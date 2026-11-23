import { render, screen, fireEvent, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import BlitzShop from "../BlitzShop";
import ChestRewards from "../ChestRewards";
import CardSelect from "../CardSelect";
import DecisionScreen from "../DecisionScreen";
import type { Step } from "../../api";
import type { BlitzWare } from "../../blitz";

function step(decision: string, state: Record<string, unknown>, chosen: Step["chosen_action"] = null): Step {
  return {
    idx: 0, decision,
    state: { decision, game_mode: "blitz", ...state } as unknown as Step["state"],
    scores: [], pv: null, chosen_action: chosen, source: "live",
    meta: { turn: null, step_no: 0, floor: 4, act: 1 },
  };
}

const ware = (over: Partial<BlitzWare>): BlitzWare => ({
  index: 1, kind: "Relic", title: "Relic", description: "Obtain [blue]1[/blue] random [gold]Relic[/gold].",
  cursed: false, rare: false, status: "Available", enabled: true, ...over,
});

const WARES: BlitzWare[] = [
  ware({ index: 1, kind: "CardUpgrade", title: "Card Upgrade Service",
    description: "[gold]Upgrade[/gold] a card in your [gold]Deck[/gold]." }),
  ware({ index: 2, kind: "RareCardReward", title: "Rare Card Reward", rare: true,
    description: "Receive [blue]1[/blue] rare card reward." }),
  ware({ index: 3, kind: "Potions", title: "Potions", status: "Used", enabled: false }),
  ware({ index: 4, kind: "Relic", title: "Relic" }),
  ware({ index: 5, kind: "CardRemoval", title: "Card Removal Service" }),
  ware({ index: 6, kind: "TwoRelics", title: "Cursed Relics", cursed: true,
    description: "Receive [red]1[/red] random [red]Curse[/red].\nObtain [blue]2[/blue] random [gold]Relics[/gold]." }),
];

const shop = (over: Record<string, unknown> = {}, chosen: Step["chosen_action"] = null) =>
  step("shop", {
    player: { name: "Ironclad", gold: 99 }, cards: [], relics: [], potions: [],
    blitz_choices_remaining: 2, blitz_wares: WARES, ...over,
  }, chosen);

describe("the Blitz merchant", () => {
  it("lays out the six wares as the game's icons, named with the game's title and text, tags stripped", () => {
    render(<BlitzShop step={shop()} onChoose={() => {}} />);
    const wares = within(screen.getByRole("group", { name: "The merchant's wares" })).getAllByRole("button");
    expect(wares).toHaveLength(6);
    const name0 = wares[0].getAttribute("aria-label") ?? "";
    expect(name0).toMatch(/^Card Upgrade Service: Upgrade a card in your Deck\./);
    expect(name0).not.toMatch(/\[gold\]/);
    expect(wares[5].getAttribute("aria-label")).toMatch(/Receive 1 random Curse\./);
    // GH #1597: the icon the game draws, named after the entry type.
    expect(wares[0].querySelector("img")?.getAttribute("src")).toMatch(/v0\.113\.0\/blitz\/entry_card_upgrade\.png$/);
  });

  it("explains a ware in a hover tip, as the game does", () => {
    render(<BlitzShop step={shop()} onChoose={() => {}} />);
    fireEvent.mouseEnter(screen.getByRole("button", { name: /^Card Upgrade Service/ }).parentElement!);
    const tip = screen.getByRole("tooltip");
    expect(tip).toHaveTextContent("Card Upgrade Service");
    expect(tip).toHaveTextContent("Upgrade a card in your Deck.");
  });

  it("marks the cursed and the rare ware", () => {
    render(<BlitzShop step={shop()} onChoose={() => {}} />);
    const cursed = screen.getByRole("button", { name: /Cursed Relics/ });
    expect(cursed).toHaveClass("is-cursed");
    expect(cursed.getAttribute("aria-label")).toMatch(/\(cursed\)/);
    const rare = screen.getByRole("button", { name: /Rare Card Reward/ });
    expect(rare).toHaveClass("is-rare");
    expect(rare.getAttribute("aria-label")).toMatch(/\(rare\)/);
    const plain = screen.getByRole("button", { name: /Card Upgrade Service/ });
    expect(plain).not.toHaveClass("is-cursed");
    expect(plain).not.toHaveClass("is-rare");
  });

  it("takes a ware by the index the engine lists it at", () => {
    const onChoose = vi.fn();
    render(<BlitzShop step={shop()} onChoose={onChoose} />);
    fireEvent.click(screen.getByRole("button", { name: /Cursed Relics/ }));
    expect(onChoose).toHaveBeenCalledWith("blitz_choice", { index: 6 });
    fireEvent.click(screen.getByRole("button", { name: /Card Removal Service/ }));
    expect(onChoose).toHaveBeenLastCalledWith("blitz_choice", { index: 5 });
  });

  it("hides a taken ware's icon and makes it inert", () => {
    const onChoose = vi.fn();
    render(<BlitzShop step={shop()} onChoose={onChoose} />);
    const used = screen.getByRole("button", { name: /Potions/ });
    expect(used).toBeDisabled();
    expect(used).toHaveClass("is-used");
    expect(used.getAttribute("aria-label")).toMatch(/Taken$/);
    fireEvent.click(used);
    expect(onChoose).not.toHaveBeenCalled();
  });

  it("says how many picks are left, and closes every ware when none are", () => {
    const first = render(<BlitzShop step={shop()} onChoose={() => {}} />);
    expect(screen.getByTestId("blitz-picks")).toHaveTextContent("You may choose 2 more items.");
    first.unmount();
    const closed = WARES.map((w) => (w.status === "Available"
      ? { ...w, status: "Disabled" as const, enabled: false } : w));
    const onChoose = vi.fn();
    render(<BlitzShop step={shop({ blitz_choices_remaining: 0, blitz_wares: closed })} onChoose={onChoose} />);
    expect(screen.getByTestId("blitz-picks")).toHaveTextContent("You can't choose any more items.");
    const wares = within(screen.getByRole("group", { name: "The merchant's wares" })).getAllByRole("button");
    for (const w of wares) expect(w).toBeDisabled();
    expect(screen.getByRole("button", { name: /Card Upgrade Service.*No picks left$/ })).toHaveClass("is-disabled");
    // Leaving is always possible.
    fireEvent.click(screen.getByRole("button", { name: /Leave shop/ }));
    expect(onChoose).toHaveBeenCalledWith("leave_room", {});
  });

  it("is read-only in Replay and marks the ware the run took", () => {
    render(<BlitzShop step={shop({}, { action: "blitz_choice", args: { index: 4 } } as Step["chosen_action"])} />);
    for (const b of screen.getAllByRole("button")) expect(b).toBeDisabled();
    const taken = screen.getByRole("button", { name: /^Relic/ });
    expect(taken).toHaveClass("is-chosen");
    expect(taken).toHaveTextContent("Taken here");
  });

  it("is what DecisionScreen shows for a Blitz shop, and only for one", () => {
    const first = render(<DecisionScreen step={shop()} onChoose={() => {}} />);
    expect(screen.getByRole("group", { name: "The merchant's wares" })).toBeInTheDocument();
    first.unmount();
    // A standard shop state carries no `blitz_choices_remaining`.
    const standard = step("shop", { player: { name: "X", gold: 0 }, cards: [], relics: [], potions: [] });
    render(<DecisionScreen step={standard} onChoose={() => {}} />);
    expect(screen.queryByRole("group", { name: "The merchant's wares" })).toBeNull();
    expect(screen.getByText("Nothing left on the rug")).toBeInTheDocument();
  });
});

describe("the Blitz chest", () => {
  const relics = [
    { index: 0, type: "relic", name: "Bronze Scales", description: "Whenever you take damage, deal [blue]3[/blue] back.", image: "/assets/relic_atlas/bronze_scales.png" },
    { index: 1, type: "relic", name: "Tuning Fork", description: "A fork.", image: null },
  ];
  const chest = (over: Record<string, unknown> = {}, chosen: Step["chosen_action"] = null) =>
    step("rewards", { player: { name: "Ironclad" }, act: 2, rewards: relics, chest_picks_left: 2, ...over }, chosen);

  it("offers both relics in round one and takes one by its listed index", () => {
    const onChoose = vi.fn();
    const { container } = render(<ChestRewards step={chest()} onChoose={onChoose} />);
    expect(screen.getByRole("heading", { name: "Choose a Relic" })).toBeInTheDocument();
    expect(screen.getByTestId("chest-rounds")).toHaveTextContent(/two relics/);
    expect(container.querySelector(".gs-treasure-rig")).toHaveAttribute("data-act", "2");
    expect(screen.getByRole("button", { name: /Bronze Scales/ })).toHaveTextContent("deal 3 back.");
    fireEvent.click(screen.getByRole("button", { name: /Tuning Fork/ }));
    expect(onChoose).toHaveBeenCalledWith("take_reward", { reward_index: 1 });
  });

  it("keeps the remaining relic's index in round two", () => {
    const onChoose = vi.fn();
    render(<ChestRewards step={chest({ rewards: [relics[1]], chest_picks_left: 1 })} onChoose={onChoose} />);
    expect(screen.getByRole("heading", { name: "One More Relic" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Bronze Scales/ })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /Tuning Fork/ }));
    expect(onChoose).toHaveBeenCalledWith("take_reward", { reward_index: 1 });
  });

  it("skips with the one Skip button", () => {
    const onChoose = vi.fn();
    render(<ChestRewards step={chest()} onChoose={onChoose} />);
    fireEvent.click(screen.getByRole("button", { name: "Skip" }));
    expect(onChoose).toHaveBeenCalledWith("skip_reward", {});
  });

  it("is read-only in Replay and marks the relic the run took", () => {
    render(<ChestRewards step={chest({}, { action: "take_reward", args: { reward_index: 0 } } as Step["chosen_action"])} />);
    for (const b of screen.getAllByRole("button")) expect(b).toBeDisabled();
    expect(screen.getByRole("button", { name: /Bronze Scales/ })).toHaveClass("is-chosen");
    expect(screen.getByText("taken here")).toBeInTheDocument();
  });

  it("is what DecisionScreen shows for the chest, and only for the chest", () => {
    const first = render(<DecisionScreen step={chest()} onChoose={() => {}} />);
    expect(screen.getByRole("group", { name: "Treasure chest" })).toBeInTheDocument();
    first.unmount();
    // A `rewards` screen with no `chest_picks_left` is not this chest: it
    // keeps the generic action list.
    const other = step("rewards", {
      player: { name: "Ironclad" }, rewards: relics,
      options: [{ label: "Continue", action: "proceed", args: {}, enabled: true }],
    });
    render(<DecisionScreen step={other} onChoose={() => {}} />);
    expect(screen.queryByRole("group", { name: "Treasure chest" })).toBeNull();
    expect(screen.getByRole("button", { name: /Continue/ })).toBeInTheDocument();
  });
});

describe("a Blitz ware's deck pick", () => {
  const cards = [
    { index: 0, id: "CARD.A", name: "A", type: "Attack", cost: 1, image: null, description: "" },
    { index: 1, id: "CARD.B", name: "B", type: "Skill", cost: 1, image: null, description: "" },
  ];
  const pick = (state: Record<string, unknown>) => step("card_select", {
    player: { name: "Ironclad" }, cards, min_select: 1, max_select: 1, context: "remove", ...state,
  });

  it("offers Cancel, which backs out to the merchant", () => {
    const onChoose = vi.fn();
    render(<CardSelect step={pick({ can_skip: true })} onChoose={onChoose} />);
    expect(screen.queryByRole("button", { name: "Skip" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onChoose).toHaveBeenCalledWith("skip_select", {});
  });

  it("leaves every other select's Skip alone", () => {
    // A mandatory Blitz pick (Neow's three cards) has no way out...
    const first = render(<CardSelect step={pick({ can_skip: false })} onChoose={() => {}} />);
    expect(screen.queryByRole("button", { name: /Skip|Cancel/ })).toBeNull();
    first.unmount();
    // ...and a standard run's skippable select still says Skip.
    const standard = pick({ can_skip: true, game_mode: undefined });
    render(<CardSelect step={standard} onChoose={() => {}} />);
    expect(screen.getByRole("button", { name: "Skip" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Cancel" })).toBeNull();
  });

  it("takes Neow's three of ten in one submit", () => {
    const ten = Array.from({ length: 10 }, (_, i) => ({
      index: i, id: `CARD.N${i}`, name: `N${i}`, type: "Attack", cost: 1, image: null, description: "",
    }));
    const onChoose = vi.fn();
    render(<CardSelect step={pick({ cards: ten, min_select: 3, max_select: 3, context: "grant", can_skip: false })}
      onChoose={onChoose} />);
    for (const name of ["N0", "N4"]) {
      fireEvent.click(screen.getByText(name).closest("div[data-played], div") as HTMLElement);
    }
    expect(onChoose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("N9").closest("div[data-played], div") as HTMLElement);
    expect(onChoose).toHaveBeenCalledTimes(1);
    expect(onChoose).toHaveBeenCalledWith("select_cards", { indices: "0,4,9" });
  });
});
