import { render, screen, fireEvent, within } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import RunLauncher from "./RunLauncher";
import { SEED_ALPHABET, SEED_LENGTH } from "../seed";

function renderLauncher(over: Partial<React.ComponentProps<typeof RunLauncher>> = {}) {
  const props: React.ComponentProps<typeof RunLauncher> = {
    character: "Ironclad", onCharacter: vi.fn(),
    seed: "Y3X2YZSBQJ", onSeed: vi.fn(),
    ascension: 0, onAscension: vi.fn(),
    act1: "roll", onAct1: vi.fn(),
    backend: "rust", onBackend: vi.fn(), backends: ["rust", "csharp"],
    onPlay: vi.fn(), onWatch: vi.fn(), busy: false, error: null,
    offCombatBrain: "heuristic", onOffCombatBrain: vi.fn(),
    combat: {
      preset: "benchmark", monsterSims: 1000, bossSims: 4000, threads: 1,
      onPreset: vi.fn(), onMonsterSims: vi.fn(), onBossSims: vi.fn(),
    },
    ...over,
  };
  return { ...render(<RunLauncher {...props} />), props };
}

describe("RunLauncher", () => {
  it("rolls a game-style seed when it opens with none", () => {
    const { props } = renderLauncher({ seed: "" });
    expect(props.onSeed).toHaveBeenCalledTimes(1);
    const rolled = (props.onSeed as ReturnType<typeof vi.fn>).mock.calls[0][0] as string;
    expect(rolled).toHaveLength(SEED_LENGTH);
    for (const ch of rolled) expect(SEED_ALPHABET).toContain(ch);
  });

  it("keeps a seed it was given, and re-rolls on demand", () => {
    const { props } = renderLauncher();
    expect(props.onSeed).not.toHaveBeenCalled();
    expect(screen.getByLabelText("seed")).toHaveValue("Y3X2YZSBQJ");
    fireEvent.click(screen.getByRole("button", { name: "new seed" }));
    expect(props.onSeed).toHaveBeenCalledTimes(1);
    expect((props.onSeed as ReturnType<typeof vi.fn>).mock.calls[0][0]).not.toBe("Y3X2YZSBQJ");
  });

  it("tells the player how a typed seed will actually run", () => {
    renderLauncher({ seed: "foo1oi" });
    expect(screen.getByText("F00101")).toBeInTheDocument();
  });

  it("offers the five characters as a radio group and reports the pick", () => {
    const { props } = renderLauncher();
    const radios = screen.getAllByRole("radio", { name: /Ironclad|Silent|Defect|Regent|Necrobinder/ });
    expect(radios).toHaveLength(5);
    expect(screen.getByRole("radio", { name: "Ironclad" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(screen.getByRole("radio", { name: "Necrobinder" }));
    expect(props.onCharacter).toHaveBeenCalledWith("Necrobinder");
  });

  it("names the ascension level and stacks the modifiers below it", () => {
    const { props } = renderLauncher({ ascension: 3 });
    expect(screen.getByText("Ascension 3 · Poverty")).toBeInTheDocument();
    // A3 stacks A1..A3: three modifier lines, the top one first.
    const mods = screen.getAllByRole("listitem");
    expect(mods).toHaveLength(3);
    expect(mods[0]).toHaveTextContent(/25% less/);
    expect(screen.getByRole("radio", { name: "ascension 3" })).toHaveAttribute("aria-checked", "true");
    fireEvent.click(screen.getByRole("radio", { name: "ascension 10" }));
    expect(props.onAscension).toHaveBeenCalledWith(10);
    // The game caps at A10 — no rung past it.
    expect(screen.queryByRole("radio", { name: "ascension 11" })).toBeNull();
  });

  it("explains both launchers and fires the right one", () => {
    const { props } = renderLauncher();
    const play = screen.getByRole("button", { name: "Play yourself" });
    const watch = screen.getByRole("button", { name: "Watch the AI play" });
    expect(play).toHaveAccessibleDescription(/You make every decision/);
    expect(watch).toHaveAccessibleDescription(/plays the whole run out/);
    fireEvent.click(play);
    expect(props.onPlay).toHaveBeenCalledTimes(1);
    fireEvent.click(watch);
    expect(props.onWatch).toHaveBeenCalledTimes(1);
    // The AI's knobs live with the AI launcher.
    expect(screen.getByRole("combobox", { name: "brain" })).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "combat" })).toBeInTheDocument();
  });

  it("hides the AI launcher and its knobs when run mode is off", () => {
    renderLauncher({ onWatch: undefined });
    expect(screen.queryByRole("button", { name: "Watch the AI play" })).toBeNull();
    expect(screen.queryByRole("combobox", { name: "brain" })).toBeNull();
    expect(screen.getByRole("button", { name: "Play yourself" })).toBeInTheDocument();
  });

  it("disables everything while a start is in flight and shows the error", () => {
    renderLauncher({ busy: true, error: "start failed: 500" });
    expect(screen.getByRole("button", { name: "Play yourself" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Running…" })).toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent("start failed: 500");
  });

  it("hides the game-version select when the server runs one build", () => {
    renderLauncher();
    expect(screen.queryByLabelText("version")).toBeNull();
    renderLauncher({ gameVersions: ["v0.107.1"], gameVersion: "v0.107.1", onGameVersion: vi.fn() });
    expect(screen.queryByLabelText("version")).toBeNull();
  });

  it("offers the game builds when there are two, and reports the pick", () => {
    const onGameVersion = vi.fn();
    renderLauncher({ gameVersions: ["v0.107.1", "v0.111.0"], gameVersion: "v0.107.1", onGameVersion });
    const sel = screen.getByLabelText("version");
    expect(sel).toHaveValue("v0.107.1");
    expect(screen.getByRole("option", { name: "v0.107.1 (previous stable)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "v0.111 (stable)" })).toBeInTheDocument();
    fireEvent.change(sel, { target: { value: "v0.111.0" } });
    expect(onGameVersion).toHaveBeenCalledWith("v0.111.0");
  });

  it("does not offer v0.107.1 once another build is picked (GH #1572)", () => {
    renderLauncher({ gameVersions: ["v0.107.1", "v0.111.0", "v0.113.0"], gameVersion: "v0.111.0", onGameVersion: vi.fn() });
    expect(screen.getByLabelText("version")).toHaveValue("v0.111.0");
    expect(screen.queryByRole("option", { name: "v0.107.1 (previous stable)" })).toBeNull();
    expect(screen.getByRole("option", { name: "v0.111 (stable)" })).toBeInTheDocument();
  });

  it("hides the select when v0.107.1 is the only other build", () => {
    renderLauncher({ gameVersions: ["v0.107.1", "v0.111.0"], gameVersion: "v0.111.0", onGameVersion: vi.fn() });
    expect(screen.queryByLabelText("version")).toBeNull();
  });

  it("pins the version to the default under the C# engine, which plays only that build", () => {
    renderLauncher({
      backend: "csharp", backends: ["rust", "csharp"],
      gameVersions: ["v0.107.1", "v0.111.0"], gameVersion: "v0.111.0", onGameVersion: vi.fn(),
    });
    const sel = screen.getByLabelText("version");
    expect(sel).toBeDisabled();
    expect(sel).toHaveValue("v0.107.1");
  });

  it("keeps the engine and act-1 selects reachable by label", () => {
    const { props } = renderLauncher();
    fireEvent.change(screen.getByLabelText("engine"), { target: { value: "csharp" } });
    expect(props.onBackend).toHaveBeenCalledWith("csharp");
    fireEvent.change(screen.getByLabelText("act 1"), { target: { value: "underdocks" } });
    expect(props.onAct1).toHaveBeenCalledWith("underdocks");
  });

  it("offers the e2e net as the advisor of a hand-played v0.111.0 run of its characters only", () => {
    const { unmount } = renderLauncher({ e2eAvailable: true, onAdvisor: vi.fn(), gameVersion: "v0.107.1" });
    expect(screen.queryByRole("combobox", { name: "advisor" })).toBeNull();
    unmount();
    const fool = renderLauncher({ e2eAvailable: true, onAdvisor: vi.fn(), character: "Fool", gameVersion: "v0.111.0" });
    expect(screen.queryByRole("combobox", { name: "advisor" })).toBeNull();
    fool.unmount();
    const silent = renderLauncher({ e2eAvailable: true, onAdvisor: vi.fn(), character: "Silent", gameVersion: "v0.111.0" });
    expect(screen.getByRole("combobox", { name: "advisor" })).toBeInTheDocument();
    silent.unmount();
    const { props } = renderLauncher({ e2eAvailable: true, onAdvisor: vi.fn(), gameVersion: "v0.111.0" });
    const pick = screen.getByRole("combobox", { name: "advisor" });
    expect(pick).toHaveValue("search");
    fireEvent.change(pick, { target: { value: "e2e" } });
    expect(props.onAdvisor).toHaveBeenCalledWith("e2e");
  });

  it("offers the e2e student only where the engine has it, and drops the sim knobs for it", () => {
    const { unmount } = renderLauncher();
    expect(screen.queryByRole("option", { name: /e2e/ })).toBeNull();
    unmount();
    const { props, unmount: un2 } = renderLauncher({ e2eAvailable: true });
    fireEvent.change(screen.getByRole("combobox", { name: "brain" }), { target: { value: "e2e" } });
    expect(props.onOffCombatBrain).toHaveBeenCalledWith("e2e");
    un2();
    renderLauncher({ e2eAvailable: true, offCombatBrain: "e2e" });
    expect(screen.getByRole("option", { name: "Instinct" })).toBeInTheDocument();
    // Instinct replaced the run-PPO brain in the picker.
    expect(screen.queryByRole("option", { name: "PPO" })).toBeNull();
    expect(screen.getByTestId("e2e-note")).toHaveTextContent("Ironclad · v0.111.0");
    expect(screen.queryByTitle(/Combat search budget/)).toBeNull();
  });

  // ---- Dread mode (GH #1346) ----
  const DREAD_BUILDS = ["v0.107.1", "v0.111.0", "v0.113.0"];

  it("hides the mode control when the server has no build with Dread", () => {
    renderLauncher({ onGameMode: vi.fn(), gameVersions: ["v0.107.1", "v0.111.0"], gameVersion: "v0.111.0" });
    expect(screen.queryByRole("radiogroup", { name: "Mode" })).toBeNull();
    // ...and when the page does not wire it at all.
    renderLauncher({ gameVersions: DREAD_BUILDS, gameVersion: "v0.113.0" });
    expect(screen.queryByRole("radiogroup", { name: "Mode" })).toBeNull();
  });

  it("offers Standard and Dread on the Rust engine at v0.113.0, and reports the pick", () => {
    const { props } = renderLauncher({ onGameMode: vi.fn(), gameVersions: DREAD_BUILDS, gameVersion: "v0.113.0" });
    expect(screen.getByRole("radio", { name: "Standard" })).toHaveAttribute("aria-checked", "true");
    const dread = screen.getByRole("radio", { name: "Dread" });
    expect(dread).toBeEnabled();
    expect(dread).toHaveAttribute("aria-checked", "false");
    fireEvent.click(dread);
    expect(props.onGameMode).toHaveBeenCalledWith("dread");
  });

  it("disables Dread with a short reason on another build or engine", () => {
    const first = renderLauncher({ onGameMode: vi.fn(), gameVersions: DREAD_BUILDS, gameVersion: "v0.111.0" });
    expect(screen.getByRole("radio", { name: "Dread" })).toBeDisabled();
    expect(screen.getByTestId("dread-note")).toHaveTextContent("Dread needs game build v0.113.0.");
    first.unmount();
    // A remembered Dread pick does not apply where Dread cannot run.
    renderLauncher({
      onGameMode: vi.fn(), gameMode: "dread", backend: "csharp",
      gameVersions: DREAD_BUILDS, gameVersion: "v0.113.0", ascension: 4,
    });
    expect(screen.getByRole("radio", { name: "Dread" })).toBeDisabled();
    expect(screen.getByRole("radio", { name: "Standard" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("dread-note")).toHaveTextContent("Dread needs the Rust engine.");
    expect(screen.getByRole("radio", { name: "ascension 4" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: "ascension 4" })).toBeEnabled();
  });

  it("locks the ascension at 10 in Dread, and leaves the AI launcher to standard runs", () => {
    const { props } = renderLauncher({
      onGameMode: vi.fn(), gameMode: "dread", gameVersions: DREAD_BUILDS, gameVersion: "v0.113.0", ascension: 2,
    });
    expect(screen.getByRole("radio", { name: "Dread" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("radio", { name: "ascension 10" })).toHaveAttribute("aria-checked", "true");
    for (const rung of screen.getAllByRole("radio", { name: /^ascension \d+$/ })) expect(rung).toBeDisabled();
    expect(screen.getByTestId("dread-note")).toHaveTextContent(/always ascension 10/);
    expect(screen.getByText(/^Ascension 10 · /)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Play yourself" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Watch the AI play" })).toBeDisabled();
    expect(screen.getByText(/AI runs are standard runs/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: "Standard" }));
    expect(props.onGameMode).toHaveBeenCalledWith("standard");
  });

  // ---- Blitz mode (GH #1347) ----
  const blitzProps = (over: Partial<React.ComponentProps<typeof RunLauncher>> = {}) => ({
    onGameMode: vi.fn(), onBlitzAscension: vi.fn(), gameVersions: DREAD_BUILDS, gameVersion: "v0.113.0", ...over,
  });

  it("offers Standard, Dread and Blitz on the Rust engine at v0.113.0", () => {
    const { props } = renderLauncher(blitzProps());
    const modes = within(screen.getByRole("radiogroup", { name: "Mode" })).getAllByRole("radio");
    expect(modes.map((m) => m.textContent)).toEqual(["Standard", "Dread", "Blitz"]);
    const blitz = screen.getByRole("radio", { name: "Blitz" });
    expect(blitz).toBeEnabled();
    expect(blitz).toHaveAttribute("aria-checked", "false");
    expect(screen.getByTestId("blitz-note")).toHaveTextContent(/^Blitz: /);
    fireEvent.click(blitz);
    expect(props.onGameMode).toHaveBeenCalledWith("blitz");
  });

  it("disables Blitz with a short reason on another build or engine", () => {
    const first = renderLauncher(blitzProps({ gameVersion: "v0.111.0" }));
    expect(screen.getByRole("radio", { name: "Blitz" })).toBeDisabled();
    expect(screen.getByTestId("blitz-note")).toHaveTextContent("Blitz needs game build v0.113.0.");
    first.unmount();
    // A remembered Blitz pick does not apply where Blitz cannot run.
    renderLauncher(blitzProps({ gameMode: "blitz", backend: "csharp", ascension: 7, blitzAscension: 2 }));
    expect(screen.getByRole("radio", { name: "Blitz" })).toBeDisabled();
    expect(screen.getByRole("radio", { name: "Standard" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("blitz-note")).toHaveTextContent("Blitz needs the Rust engine.");
    expect(screen.getByRole("radio", { name: "ascension 7" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getAllByRole("radio", { name: /^ascension \d+$/ })).toHaveLength(11);
  });

  it("shows the Blitz ladder, 0 to 5 with its own modifiers, in Blitz", () => {
    const { props } = renderLauncher(blitzProps({ gameMode: "blitz", ascension: 7, blitzAscension: 3 }));
    expect(screen.getByRole("radio", { name: "Blitz" })).toHaveAttribute("aria-checked", "true");
    const rungs = screen.getAllByRole("radio", { name: /^ascension \d+$/ });
    expect(rungs.map((r) => r.textContent)).toEqual(["0", "1", "2", "3", "4", "5"]);
    expect(screen.getByRole("radio", { name: "ascension 3" })).toHaveAttribute("aria-checked", "true");
    // Blitz level 3 is Tough Enemies; the standard A3 is Poverty.
    expect(screen.getByText("Blitz ascension 3 · Tough Enemies")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "ascension 3" })).toHaveAttribute("title", "A3 · Tough Enemies");
    const mods = screen.getAllByRole("listitem").map((li) => li.textContent);
    expect(mods).toEqual([
      "All enemies are harder to kill.",
      "Start each run with 1 less potion slot.",
      "Ancients only heal 80% of your missing HP.",
    ]);
    expect(screen.getByTestId("blitz-note")).toHaveTextContent(/own ascension ladder, 0 to 5/);
    expect(screen.queryByTestId("dread-note")).toBeNull();
    // A rung sets the Blitz level, never the standard ascension.
    fireEvent.click(screen.getByRole("radio", { name: "ascension 5" }));
    expect(props.onBlitzAscension).toHaveBeenCalledWith(5);
    expect(props.onAscension).not.toHaveBeenCalled();
    // AI runs are standard runs.
    expect(screen.getByRole("button", { name: "Play yourself" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Watch the AI play" })).toBeDisabled();
    expect(screen.getByText(/AI runs are standard runs/)).toBeInTheDocument();
  });

  it("brings the standard pick back when the mode returns to Standard", () => {
    const { rerender, props } = renderLauncher(blitzProps({ gameMode: "blitz", ascension: 7, blitzAscension: 3 }));
    rerender(<RunLauncher {...props} gameMode="standard" />);
    expect(screen.getAllByRole("radio", { name: /^ascension \d+$/ })).toHaveLength(11);
    expect(screen.getByRole("radio", { name: "ascension 7" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText("Ascension 7 · Scarcity")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("radio", { name: "ascension 2" }));
    expect(props.onAscension).toHaveBeenCalledWith(2);
    expect(props.onBlitzAscension).not.toHaveBeenCalled();
  });
});
