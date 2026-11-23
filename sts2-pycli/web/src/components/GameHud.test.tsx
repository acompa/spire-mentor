import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import GameHud from "./GameHud";
import type { CombatState } from "../api";

function state(over: Partial<CombatState> = {}): CombatState {
  return {
    decision: "map_select",
    ascension: 10,
    player: { name: "Ironclad", hp: 70, max_hp: 80, gold: 99, potions: [], relics: [] },
    ...over,
  } as unknown as CombatState;
}

describe("GameHud", () => {
  it("carries no Dread marker in a standard run", () => {
    render(<GameHud state={state()} />);
    expect(screen.getByTestId("game-hud")).toBeInTheDocument();
    expect(screen.queryByTestId("hud-dread")).toBeNull();
  });

  it("marks a Dread run and names the dominion in force (GH #1346)", () => {
    render(<GameHud state={state({ game_mode: "dread", dominion: "KAISER_CRAB_DOMINION" })} />);
    const mark = screen.getByTestId("hud-dread");
    expect(mark).toHaveTextContent("Dread");
    expect(mark).toHaveTextContent("Kaiser Crab's Dominion");
    expect(mark).toHaveAccessibleName("Dread, Kaiser Crab's Dominion");
  });

  it("shows the marker alone where no dominion is in force (a boss room)", () => {
    render(<GameHud state={state({ game_mode: "dread", dominion: null })} />);
    const mark = screen.getByTestId("hud-dread");
    expect(mark).toHaveTextContent("Dread");
    expect(mark).not.toHaveTextContent("Dominion");
    expect(mark).toHaveAccessibleName("Dread");
  });

  // ---- Blitz mode (GH #1347) ----
  it("carries no Blitz marker in a standard or a Dread run", () => {
    const first = render(<GameHud state={state()} />);
    expect(screen.queryByTestId("hud-blitz")).toBeNull();
    // The standard flame reads as it always did.
    expect(screen.getByRole("img", { name: "A10" })).toHaveAttribute("title", "Ascension 10");
    first.unmount();
    render(<GameHud state={state({ game_mode: "dread", dominion: null })} />);
    expect(screen.queryByTestId("hud-blitz")).toBeNull();
  });

  it("marks a Blitz run and titles the flame with the Blitz ladder", () => {
    render(<GameHud state={state({ game_mode: "blitz", ascension: 3 })} />);
    const mark = screen.getByTestId("hud-blitz");
    expect(mark).toHaveTextContent("Blitz");
    expect(mark).toHaveAccessibleName("Blitz");
    expect(screen.queryByTestId("hud-dread")).toBeNull();
    // Blitz level 3 is Tough Enemies, not the standard ladder's Poverty.
    const flame = screen.getByRole("img", { name: "Blitz A3" });
    expect(flame).toHaveTextContent("3");
    expect(flame).toHaveAttribute("title", "Blitz ascension 3 · Tough Enemies, Tight Belt, Weary Traveler");
    expect(flame.getAttribute("title")).not.toMatch(/Poverty|Swarming/);
  });

  it("says a level-0 Blitz run has no modifiers", () => {
    render(<GameHud state={state({ game_mode: "blitz", ascension: 0 })} />);
    expect(screen.getByRole("img", { name: "Blitz A0" }))
      .toHaveAttribute("title", "Blitz ascension 0 · no modifiers");
  });
});
