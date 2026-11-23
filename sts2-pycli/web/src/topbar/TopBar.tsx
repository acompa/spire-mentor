import { gameVersionBody } from "../gameVersion";
import { useMemo, type ReactNode } from "react";
import Menu, { type MenuItemSpec } from "./Menu";
import { usePhone } from "../phone";
import { t } from "../i18n";
import { languageItems } from "./languageItems";
import CommandPalette from "./CommandPalette";
import type { PaletteEntry } from "./paletteEntries";
import type { Command } from "./commands";
import MentorBar, { MODES, type MentorBarProps, type MentorMode } from "./MentorBar";

/**
 * The workbench's top bar, rebuilt around three questions a user actually
 * asks — "what am I looking at", "what is the AI doing for me", "what can I
 * do to this run" — instead of one row of every button.
 *
 *   [Live | Replay | MCTS]  Mode [Off · Coach · Hints · AI plays] [Search · Instinct]
 *   Peek · Let AI pick · Autobattle  ● status  …context…  [Run ▾] [Analyze ▾] [Tools ▾]  act · floor  ▤
 *
 *  - The VIEW switch and the AI's controls (MentorBar: the mod's badge
 *    popover laid flat) are always on the bar: their state matters at a
 *    glance. While the AI plays, the bar turns amber and leads with Stop.
 *  - CONTEXT controls are the one or two things the current view is about:
 *    replay's run picker, follow and "Play from here".
 *  - Everything else is a one-shot verb and lives in a menu, grouped by
 *    job. Items that do not apply right now are disabled with the reason,
 *    not removed, so each menu keeps its shape as the run moves.
 *  - Ctrl+K lists all of it.
 *  - A phone has no room for the row: held upright the bar is the Mode
 *    switch and one Menu button whose panel holds everything else; held
 *    sideways the whole bar folds into the Menu.
 *
 * This component owns layout only. What is offered, and why an item is
 * disabled, is decided by `buildCommands` (commands.ts) in App, so the tests
 * there are the tests for the bar's behaviour.
 */

export interface TopBarProps {
  mode: "play" | "replay" | "mcts";
  setMode: (m: "play" | "replay" | "mcts") => void;
  showViewToggle: boolean;
  replayCount: number;

  commands: Command[];

  /** The AI's controls (Mode, Brain, this decision's verbs, status);
   *  `undefined` ⇒ not offered in this mode / build. */
  mentor?: MentorBarProps;

  /** Replay's context controls; only rendered in replay mode. */
  replay?: {
    runs: string[];
    loadRun: (path: string) => void;
    loadedPath: string | null;
    follow: boolean;
    setFollow: (v: boolean) => void;
    followIsLive: boolean;
    /** Follow was switched on by the mod's "Open Workbench" link, which
     *  PROMISES the live run; warn when it is tailing something else. */
    followFromUrl: boolean;
    bridgeError: string | null;
  };

  tools: {
    simlabLink: boolean;
    /** The shown run's game build (GH #877): the synergy catalog opens with
     *  that build's card text. Absent / the default = the default build. */
    gameVersion?: string | null;
    debug: { on: boolean; toggle: () => void };
    /** The engine's own numbers (win %, Q, shares) beside the marks and rows. */
    engineNumbers: { on: boolean; toggle: () => void };
    onHelp: () => void;
    /** Browser build only: the "Live game…" connection dialog (GH #709). */
    onBridge?: () => void;
    bridgeState?: "off" | "connected" | "unreachable";
    /** Self-contained controls hosted as rows: bug report, bench, offline,
     *  fx, sound. Each renders `null` when it does not apply. */
    controls: ReactNode[];
  };

  /** "Act 2 · floor 21", or null. */
  where: string | null;
  whereTitle?: string;

  palette: { open: boolean; setOpen: (v: boolean) => void };

  /** A phone's own bar buttons: take the last move back, and open the map
   *  and history sheet. On the bar itself when held upright, at the top of
   *  the Menu's panel when held sideways (the bar is folded away there). */
  phoneActions?: {
    undo?: { run: () => void; disabledReason?: string };
    log: { open: boolean; toggle: () => void };
    /** The AI makes just this one move ("Let AI pick", G). Upright it sits
     *  on the bar; sideways, pinned beside the Menu button. */
    aiStep?: { run: () => void; disabledReason?: string };
  };

  /** The side-panel toggle sits OUTSIDE the scroller so it is always on
   *  screen; rendered by the caller and passed through. */
  trailing?: ReactNode;
}

const GROUP_TITLES: Record<string, string> = { run: t("Run"), analyze: t("Analyze") };

function toMenuItem(c: Command): MenuItemSpec {
  return { id: c.id, label: c.label, title: c.title, disabledReason: c.disabledReason,
    danger: c.danger, busy: c.busy, onSelect: c.run };
}

/** The Run menu: everything but the destructive item, a rule, then it. */
function runMenuItems(cmds: Command[]): (MenuItemSpec | null)[] {
  const run = cmds.filter((c) => c.group === "run" && c.id !== "play-from-here");
  const safe = run.filter((c) => !c.danger).map(toMenuItem);
  const danger = run.filter((c) => c.danger).map(toMenuItem);
  return danger.length ? [...safe, null, ...danger] : safe;
}

/** The synergy catalog's URL for game build `v` (`?game_version=` only for a
 *  non-default build, read by `catalog/main.ts`). */
function catalogHref(v: string | null | undefined): string {
  const q = gameVersionBody(v).game_version;
  return q ? `/catalog.html?game_version=${encodeURIComponent(q)}` : "/catalog.html";
}

export default function TopBar(p: TopBarProps) {
  const runItems = useMemo(() => runMenuItems(p.commands), [p.commands]);
  const analyzeItems = useMemo(
    () => p.commands.filter((c) => c.group === "analyze").map(toMenuItem),
    [p.commands],
  );
  const playFromHere = p.commands.find((c) => c.id === "play-from-here");

  const toolItems: (MenuItemSpec | null)[] = [
    ...(p.tools.simlabLink
      ? [{ id: "simlab", label: t("🧪 SimLab ↗"), href: "/simlab.html",
          title: t("SimLab — batch combat statistics (opens in a new tab so your run isn't disturbed)") },
        { id: "seedcheck", label: t("🎲 Seed check ↗"), href: "/seedcheck.html",
          title: t("Seed check — can Instinct win a posted seed? (Ironclad A10, v0.111.0; opens in a new tab)") }]
      : []),
    { id: "synergies", label: t("⚡ Synergies ↗"), href: catalogHref(p.tools.gameVersion),
      title: t("Card Synergy Catalog — pair synergies per character (opens in a new tab so your run isn't disturbed)") },
    ...(p.tools.onBridge
      ? [{ id: "bridge", onSelect: p.tools.onBridge,
          label: p.tools.bridgeState === "connected" ? t("🎮 Live game · connected")
            : p.tools.bridgeState === "unreachable" ? t("🎮 Live game · unreachable") : t("🎮 Live game…"),
          title: t("connect this page to the running game's advisor (game tree, live search, rewind)") }]
      : []),
    null,
    { id: "engine-numbers", label: t("🔢 engine numbers"), checked: p.tools.engineNumbers.on,
      onSelect: p.tools.engineNumbers.toggle,
      title: t("show the engine's own numbers beside the advice: win %, Q, search shares, W/L/T (off: the advice in words)") },
    { id: "debug", label: t("🐞 debug log"), checked: p.tools.debug.on, onSelect: p.tools.debug.toggle,
      title: t("the debug log drawer — records run settings, per-decision MCTS sims, and the play-to-end trace (can be turned on any time)") },
    ...p.tools.controls.filter(Boolean).map((node, i) => ({ id: `control-${i}`, label: "", render: node })),
    null,
    { id: "palette", label: t("command palette"), title: t("every command, filterable"), onSelect: () => p.palette.setOpen(true) },
    { id: "help", label: t("keyboard shortcuts"), title: t("what the keys do"), onSelect: p.tools.onHelp },
    null,
    ...languageItems(),
  ];

  // The palette is a view of the same entries the bar renders, plus the
  // chips and the view switch, so nothing is reachable from one and not
  // the other.
  const paletteEntries: PaletteEntry[] = useMemo(() => {
    const out: PaletteEntry[] = [];
    if (p.showViewToggle) {
      out.push({ id: "view-live", group: t("View"), label: t("Live"), disabledReason: p.mode === "play" ? t("already here") : undefined, run: () => p.setMode("play") });
      out.push({ id: "view-replay", group: t("View"), label: t("Replay"),
        disabledReason: p.mode === "replay" ? t("already here") : p.replayCount === 0 ? t("no run loaded to replay") : undefined,
        run: () => p.setMode("replay") });
    }
    out.push({ id: "view-mcts", group: t("View"), label: t("📊 Run history"), disabledReason: p.mode === "mcts" ? t("already here") : undefined, run: () => p.setMode("mcts") });
    const m = p.mentor;
    if (m) {
      for (const [key, label] of [["off", t("Mode: Off")], ["coach", t("Mode: Coach")], ["hints", t("Mode: Hints")],
        ["aiplays", t("Mode: AI plays")]] as [MentorMode, string][]) {
        out.push({ id: `mode-${key}`, group: "AI", label, checked: m.mode === key,
          disabledReason: key === "aiplays" && m.mode !== "aiplays" ? m.aiPlaysDisabled : undefined,
          run: () => m.setMode(key) });
      }
      if (m.brain) {
        const b = m.brain;
        out.push({ id: "brain-search", group: "AI", label: t("Brain: Search"), checked: b.value === "search",
          run: () => b.set("search") });
        out.push({ id: "brain-instinct", group: "AI", label: t("Brain: Instinct"), checked: b.value === "instinct",
          run: () => b.set("instinct") });
      }
      if (m.onPeek) out.push({ id: "peek", group: "AI", label: t("Peek at the AI's pick"), shortcut: "P", run: m.onPeek });
      if (m.letAiPick) {
        out.push({ id: "let-ai-pick", group: "AI", label: t("Let AI pick"), shortcut: "G",
          disabledReason: m.letAiPick.disabledReason, run: m.letAiPick.run });
      }
      if (m.autobattle) {
        out.push({ id: "autobattle", group: "AI", label: t("⚔️ autobattle"), checked: m.autobattle.on,
          run: m.autobattle.toggle });
      }
      if (m.aiPlaying) out.push({ id: "stop-ai", group: "AI", label: t("■ Stop AI"), shortcut: "Esc", run: m.onStop });
    }
    for (const c of p.commands) {
      out.push({ id: c.id, group: GROUP_TITLES[c.group] ?? c.group, label: c.label, title: c.title,
        shortcut: c.id === "recommend" ? "H" : undefined,
        disabledReason: c.busy ? t("in progress") : c.disabledReason, run: c.run });
    }
    if (p.tools.simlabLink) out.push({ id: "simlab", group: t("Tools"), label: t("🧪 SimLab ↗"), run: () => window.open("/simlab.html", "_blank", "noopener") });
    if (p.tools.simlabLink) out.push({ id: "seedcheck", group: t("Tools"), label: t("🎲 Seed check ↗"), run: () => window.open("/seedcheck.html", "_blank", "noopener") });
    out.push({ id: "synergies", group: t("Tools"), label: t("⚡ Synergies ↗"), run: () => window.open(catalogHref(p.tools.gameVersion), "_blank", "noopener") });
    out.push({ id: "engine-numbers", group: t("Tools"), label: t("🔢 engine numbers"), checked: p.tools.engineNumbers.on,
      run: p.tools.engineNumbers.toggle });
    out.push({ id: "debug", group: t("Tools"), label: t("🐞 debug log"), checked: p.tools.debug.on, run: p.tools.debug.toggle });
    out.push({ id: "help", group: t("Tools"), label: t("keyboard shortcuts"), shortcut: "?", run: p.tools.onHelp });
    return out;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [p.mode, p.showViewToggle, p.replayCount, p.mentor, p.commands, p.tools.simlabLink, p.tools.debug.on,
    p.tools.engineNumbers.on, p.tools.gameVersion]);

  const phone = usePhone();
  // The bar's contents. `inPanel`: laid out inside the phone's Menu panel
  // (phone landscape, where the bar itself folds away), whose own sections
  // already list the Run / Analyze / Tools entries.
  const barContent = (inPanel: boolean, mentorParts: "all" | "rest" = "all") => (
    <>
        {/* View switch */}
        <span className="wb-segmented" role="group" aria-label={t("view")}>
          {p.showViewToggle && (
            <>
              <button onClick={() => p.setMode("play")} disabled={p.mode === "play"} aria-pressed={p.mode === "play"}
                title={t("the live run / start screen")}>{t("Live")}</button>
              <button onClick={() => p.setMode("replay")} disabled={p.mode === "replay" || p.replayCount === 0}
                aria-pressed={p.mode === "replay"}
                title={p.replayCount === 0 ? t("no run loaded to replay") : t("scrub a loaded run")}>{t("Replay")}</button>
            </>
          )}
          <button onClick={() => p.setMode("mcts")} disabled={p.mode === "mcts"} aria-pressed={p.mode === "mcts"}
            title={t("your recorded runs: open one to replay it")}>{t("📊 Run history")}</button>
        </span>

        {/* The AI's controls: Mode, Brain, this decision, status */}
        {p.mentor && <MentorBar {...p.mentor} parts={mentorParts} />}

        {/* Context controls */}
        {p.mode === "replay" && p.replay && (
          <span className="wb-context">
            <select onChange={(e) => e.target.value && p.replay!.loadRun(e.target.value)} defaultValue=""
              aria-label={t("load a run")}>
              <option value="" disabled>{t("load a run…")}</option>
              {p.replay.runs.map((r) => <option key={r} value={r}>{r.split("/").pop()}</option>)}
            </select>
            {p.replay.loadedPath && (
              <label title={p.replay.followIsLive
                ? t("following the file the live advisor says it is writing")
                : t("poll the loaded run file and auto-advance to new steps as a live game writes them")}>
                <input type="checkbox" checked={p.replay.follow} onChange={(e) => p.replay!.setFollow(e.target.checked)} />
                {" "}{t("follow")}{p.replay.follow ? (p.replay.followIsLive ? t(" · live") : t(" · not live")) : ""}
              </label>
            )}
            {playFromHere && (
              <button onClick={playFromHere.run} disabled={!!playFromHere.disabledReason || playFromHere.busy}
                title={playFromHere.disabledReason ?? playFromHere.title}>
                {playFromHere.busy ? t("🎮 rebuilding…") : playFromHere.label}
              </button>
            )}
            {p.replay.loadedPath && p.replay.follow && !p.replay.followIsLive && p.replay.followFromUrl && (
              <span style={{ color: "var(--accent-strong)" }} title={t(
                "No advisor is answering, so this is the newest file in the logs directory — not necessarily this game's run. Turn the AI coach on in the mod and this switches to the live run by itself.",
              )}>
                {t("⚠ advisor offline — newest file on disk")}
              </span>
            )}
            {p.replay.bridgeError && <span style={{ color: "var(--hp)" }}>{p.replay.bridgeError}</span>}
          </span>
        )}

        {!inPanel && (<>
        {/* Menus */}
        {p.mode !== "mcts" && (
          <span className="wb-menus">
            {runItems.length > 0 && <Menu label={t("Run")} title={t("save, load, import, end")} items={runItems} />}
            {analyzeItems.length > 0 && <Menu label={t("Analyze")} title={t("reviews, search tree, sandbox")} items={analyzeItems} />}
            <Menu label={t("Tools")} title={t("links, debug, effects, sound, shortcuts")} items={toolItems}
              active={p.tools.debug.on} />
          </span>
        )}
        {p.mode === "mcts" && (
          <span className="wb-menus">
            <Menu label={t("Tools")} title={t("links, debug, effects, sound, shortcuts")} items={toolItems}
              active={p.tools.debug.on} />
          </span>
        )}

        </>)}

        {p.where && (
          <span className="wb-where" title={p.whereTitle}>{p.where}</span>
        )}
    </>
  );

  // Phone, both orientations: one labelled "Menu" button pinned at the bar's
  // right edge (outside the scroller), opening every entry as one panel. In
  // landscape the bar is folded into it as well.
  const pa = p.phoneActions;
  // Labelled, tap-size, on the main screen: the owner could find neither
  // undo nor the log behind an unlabelled icon inside the Menu.
  const phoneButtons = pa && (
    <>
      {pa.undo && (
        <button type="button" className="wb-phonebtn" onClick={pa.undo.run} disabled={!!pa.undo.disabledReason}
          title={pa.undo.disabledReason ?? t("take the last move back")}>
          <span aria-hidden="true">↶</span> {t("Undo")}
        </button>
      )}
      <button type="button" className="wb-phonebtn" onClick={pa.log.toggle} aria-pressed={pa.log.open}
        title={t("the map, the history (tap a step to go back to it) and the AI's reasoning")}>
        <span aria-hidden="true">▤</span> {t("Log")}
      </button>
    </>
  );
  // One tap = one AI move, so it can be pressed again and again without
  // opening the Menu (where "Let AI pick" also stays, with the desktop bar's
  // verbs).
  const aiStepButton = pa?.aiStep && (
    <button type="button" className="wb-phonebtn wb-phonebtn--ai" onClick={pa.aiStep.run}
      disabled={!!pa.aiStep.disabledReason} data-testid="phone-ai-step"
      title={pa.aiStep.disabledReason ?? t("the AI makes this one move")} aria-label={t("AI: make one move")}>
      <span aria-hidden="true">▶</span> AI
    </button>
  );
  const m = p.mentor;
  const modeItems: MenuItemSpec[] = m ? MODES.map((x) => ({
    id: `mode-${x.key}`, label: x.label, title: x.title, checked: m.mode === x.key,
    disabledReason: x.key === "aiplays" && m.mode !== "aiplays" ? m.aiPlaysDisabled : undefined,
    onSelect: () => { if (m.mode !== x.key) m.setMode(x.key); },
  })) : [];
  const heading = (h: string): MenuItemSpec =>
    ({ id: `heading-${h}`, label: h, render: <span className="wb-menu-heading">{h}</span> });
  const phoneItems: (MenuItemSpec | null)[] = [
    ...(phone
      ? [{ id: "bar", label: "", render: <div className="wb-phonemenu-bar">
        {barContent(true, phone === "portrait" && p.mentor ? "rest" : "all")}
        {phone === "landscape" && phoneButtons}{!p.phoneActions && p.trailing}</div> }]
      : []),
    ...(p.mode !== "mcts" && runItems.length > 0 ? [heading(t("Run")), ...runItems] : []),
    ...(p.mode !== "mcts" && analyzeItems.length > 0 ? [heading(t("Analyze")), ...analyzeItems] : []),
    heading(t("Tools")), ...toolItems,
  ];

  return (
    <>
      {phone === null && <div className="wb-topbar-scroll">{barContent(false)}</div>}
      {phone === null && p.trailing}
      {/* A phone held upright: no scroller. The bar is the Mode switch at tap
          size (Stop alone while the AI plays) and the Menu button; the view
          switch, Brain, verbs and status are in the Menu's panel. Without AI
          controls (replay, MCTS) it says where the user is instead. */}
      {phone === "portrait" && (
        <div className="wb-phonebar">
          {/* The AI's mode as one button that opens the four choices (Stop
              alone while the AI plays), then Undo and Log. */}
          {m?.aiPlaying ? <MentorBar {...m} parts="mode" />
            : m ? (
              <Menu className={`wb-phonemode wb-phonemode--${m.mode}`} title={t("what the AI does for you")} caret={false}
                label={<><span className="wb-phonemode-cap">AI</span> {MODES.find((x) => x.key === m.mode)?.label}</>}
                items={modeItems} />
            ) : <span className="wb-phonebar-where">{p.mode === "replay" ? t("Replay") : p.mode === "mcts" ? t("Run history") : p.where ?? ""}</span>}
          {aiStepButton}
          {phoneButtons}
        </div>
      )}
      {phone === "landscape" && aiStepButton}
      {phone && (
        <Menu className="wb-phonemenu" caret={false} title={t("every workbench control: view, AI, run, analyze, tools")}
          label={<><span aria-hidden="true" className="wb-phonemenu-icon">☰</span> {t("Menu")}</>}
          items={phoneItems} />
      )}
      <CommandPalette open={p.palette.open} entries={paletteEntries} onClose={() => p.palette.setOpen(false)} />
    </>
  );
}
