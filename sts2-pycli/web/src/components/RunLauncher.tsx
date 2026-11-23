import { useEffect, useId, useState, type CSSProperties } from "react";
import { assetUrl, type Advisor } from "../api";
import { e2eAdvisable, e2eGameVersion } from "../advisor";
import { characterArt, characterInBuild, characterKey } from "../characters";
import { ASCENSION_LEVELS, ascensionLevel, ascensionModifiers } from "../ascension";
import { canonicalizeSeed, generateSeed } from "../seed";
import { LEGACY_GAME_VERSION, gameVersionLabel, pickerVersions } from "../gameVersion";
import {
  DREAD_ASCENSION, DREAD_GAME_VERSION, dreadUnavailableReason, effectiveGameMode, type GameMode,
} from "../dread";
import { BLITZ_LEVELS, blitzLevel, blitzModifiers, blitzUnavailableReason } from "../blitz";
import CombatBudget, { type CombatPreset } from "./CombatBudget";
import RichText from "./RichText";
import "../skins/runend.css";
import "../skins/launcher.css";
import { t } from "../i18n";

/** The playable characters, in the game's roster order. The Fool is
 *  v0.113.0-only: the launcher offers it only for a build that has it. */
export const CHARACTERS = ["Ironclad", "Silent", "Defect", "Regent", "Necrobinder", "Fool"] as const;

const ENGINE_LABELS: Record<string, string> = { rust: "Rust", csharp: "C#" };

export type OffCombatBrain = "heuristic" | "ppo" | "e2e";

/** The e2e student plays only the run it was trained on. */
export { E2E_CHARACTER, E2E_GAME_VERSION } from "../advisor";

/**
 * The start screen: pick a character, seed and ascension, then choose HOW the
 * run is played — by you, or by the run AI while you watch.
 *
 * Modelled on the game's own lobby rather than a settings bar: portrait tiles,
 * a game-style seed that is already rolled (with a re-roll, the way the game
 * shows a seed before you commit), the ascension ladder with each level's
 * real name, and the two launch actions each explained in a sentence. The
 * dev-only knobs (act-1 variant, engine) sit in a muted row at the foot.
 *
 * The seed is rolled here, on the client, so the run you start is the run you
 * can read off the screen — an empty field would have meant "whatever the
 * server picks", and the old form's empty default was the fixed seed `test`.
 */
export default function RunLauncher({
  character, onCharacter,
  seed, onSeed,
  ascension, onAscension,
  act1, onAct1,
  backend, onBackend, backends,
  gameVersion = LEGACY_GAME_VERSION, onGameVersion, gameVersions = [],
  gameMode = "standard", onGameMode,
  blitzAscension = 0, onBlitzAscension,
  onPlay, onWatch, busy, error,
  offCombatBrain, onOffCombatBrain, e2eAvailable = false,
  advisor = "search", onAdvisor,
  combat,
}: {
  character: string;
  onCharacter: (c: string) => void;
  seed: string;
  onSeed: (s: string) => void;
  ascension: number;
  onAscension: (a: number) => void;
  act1: string;
  onAct1: (a: string) => void;
  backend: string;
  onBackend: (b: string) => void;
  backends: string[];
  /** GH #801: the game build the run plays, from the server's
   *  `game_versions`. The select only shows when there are two or more to
   *  choose between. */
  gameVersion?: string;
  onGameVersion?: (v: string) => void;
  gameVersions?: string[];
  /** GH #1346, #1347: the remembered mode pick. Dread (which locks the
   *  ascension at 10) and Blitz apply only on the Rust engine at v0.113.0;
   *  the control shows when the server has that build. */
  gameMode?: GameMode;
  onGameMode?: (m: GameMode) => void;
  /** GH #1347: the Blitz level 0..5, kept apart from `ascension` so the
   *  standard pick is still there when the mode goes back. */
  blitzAscension?: number;
  onBlitzAscension?: (a: number) => void;
  /** "Play yourself": start an interactive run. */
  onPlay: () => void;
  /** "Watch the AI play": start a run and let the run agent play it out.
   *  Omitted when the server does not advertise run mode. */
  onWatch?: () => void;
  /** A start / autoplay is in flight — both launchers are disabled. */
  busy: boolean;
  error: string | null;
  offCombatBrain: OffCombatBrain;
  onOffCombatBrain: (b: OffCombatBrain) => void;
  /** The engine has the e2e student brain (`features.e2e`). */
  e2eAvailable?: boolean;
  /** Who advises the hand-played run ("Play yourself"). The picker shows
   *  only where the e2e student can advise (its characters, v0.111.0, Rust). */
  advisor?: Advisor;
  onAdvisor?: (a: Advisor) => void;
  combat: {
    preset: CombatPreset;
    monsterSims: number;
    bossSims: number;
    threads: number;
    onPreset: (p: CombatPreset) => void;
    onMonsterSims: (n: number) => void;
    onBossSims: (n: number) => void;
  };
}) {
  const ids = useId();
  // Roll a seed the moment the screen appears with none, like the game's lobby.
  useEffect(() => {
    if (!seed) onSeed(generateSeed());
    // eslint-disable-next-line react-hooks/exhaustive-deps -- mount only
  }, []);

  const canonical = canonicalizeSeed(seed);
  const slug = characterKey(character);
  // The build this run would play (the version select shows the same value):
  // a character it lacks is not offered, and a pick it lacks falls back.
  const runVersion = backend === "rust" ? gameVersion : LEGACY_GAME_VERSION;
  useEffect(() => {
    if (!characterInBuild(slug, runVersion)) onCharacter(CHARACTERS[0]);
  }, [slug, runVersion, onCharacter]);
  // Dread (GH #1346): offered when the server has its build, enabled when
  // the engine and build picked can run it. In Dread the ladder shows the
  // 10 the game embarks at and cannot be moved; the page's own ascension
  // pick is untouched, so Standard brings it back.
  // (`dreadOffered` gates the whole Mode control: Blitz shares the build.)
  const dreadOffered = !!onGameMode && gameVersions.includes(DREAD_GAME_VERSION);
  const dreadBlocked = dreadUnavailableReason(backend, runVersion);
  const mode = dreadOffered ? effectiveGameMode(gameMode, backend, runVersion) : "standard";
  const dread = mode === "dread";
  // Blitz (GH #1347) arrived in the same build, so it is offered with
  // Dread. It has a ladder of its own (0..5, a different modifier per
  // level), picked apart from the standard one.
  const blitzBlocked = blitzUnavailableReason(backend, runVersion);
  const blitz = mode === "blitz";
  const ladder = blitz ? BLITZ_LEVELS : ASCENSION_LEVELS;
  const shownAscension = dread ? DREAD_ASCENSION : blitz ? blitzAscension : ascension;
  const level = blitz ? blitzLevel(shownAscension) : ascensionLevel(shownAscension);
  const stacked = blitz ? blitzModifiers(shownAscension) : ascensionModifiers(shownAscension);
  const pickAscension = blitz ? (onBlitzAscension ?? (() => {})) : onAscension;
  const [heroOk, setHeroOk] = useState(true);
  useEffect(() => setHeroOk(true), [slug]);

  return (
    <div className="gs-scene gs-scene--plain gs-launch" data-testid="run-launcher">
      <div className="gs-launch-frame">
        <header className="gs-launch-head">
          <h1 className="gs-launch-title">{t("New run")}</h1>
          <p className="gs-launch-sub">{t("Pick a character, a seed and an ascension, then decide who plays.")}</p>
        </header>

        <div className="gs-launch-grid">
          <div className="gs-launch-setup">
            {/* ---- character ---- */}
            <section className="gs-launch-field">
              <h2 className="gs-launch-label" id={`${ids}-char`}>{t("Character")}</h2>
              <div className="gs-launch-chars" role="radiogroup" aria-labelledby={`${ids}-char`}>
                {CHARACTERS.filter((ch) => characterInBuild(characterKey(ch), runVersion)).map((ch) => {
                  const k = characterKey(ch);
                  const on = ch === character;
                  return (
                    <button
                      key={ch} type="button" role="radio" aria-checked={on}
                      className={`gs-launch-char${on ? " is-on" : ""}`}
                      onClick={() => onCharacter(ch)} disabled={busy}
                    >
                      <span className="gs-launch-charface">
                        {k
                          ? <img src={assetUrl(characterArt(k, `ui/top_panel/character_icon_${k}.png`))} alt="" draggable={false} />
                          : <b>{ch[0]}</b>}
                      </span>
                      <span className="gs-launch-charname">{t(ch)}</span>
                    </button>
                  );
                })}
              </div>
            </section>

            {/* ---- seed ---- */}
            <section className="gs-launch-field">
              <label className="gs-launch-label" htmlFor={`${ids}-seed`}>{t("Seed")}</label>
              <div className="gs-launch-seedrow">
                <input
                  id={`${ids}-seed`} className="gs-launch-seed mono" aria-label={t("seed")}
                  value={seed} onChange={(e) => onSeed(e.target.value)}
                  placeholder={t("random")} spellCheck={false} autoComplete="off"
                  maxLength={64} disabled={busy}
                />
                <button type="button" className="gs-launch-reroll" onClick={() => onSeed(generateSeed())}
                  disabled={busy} title={t("roll a new seed")} aria-label={t("new seed")}>
                  <span aria-hidden="true">⟳</span> {t("New seed")}
                </button>
              </div>
              <p className="gs-launch-hint">
                {seed.trim() === ""
                  ? t("Empty rolls a fresh seed when the run starts.")
                  : canonical !== seed
                    ? (() => {
                        const [pre, post] = t("Runs as {seed} — the game upper-cases seeds and reads O as 0 and I as 1.").split("{seed}");
                        return <>{pre}<b className="mono">{canonical}</b>{post}</>;
                      })()
                    : t("The same seed replays the same map, rewards and fights.")}
              </p>
            </section>

            {/* ---- mode ---- */}
            {dreadOffered && (
              <section className="gs-launch-field">
                <h2 className="gs-launch-label" id={`${ids}-mode`}>{t("Mode")}</h2>
                <div className="gs-launch-modes" role="radiogroup" aria-labelledby={`${ids}-mode`}>
                  <button type="button" role="radio" aria-checked={mode === "standard"}
                    className={`gs-launch-rung gs-launch-mode${mode === "standard" ? " is-on" : ""}`}
                    onClick={() => onGameMode?.("standard")} disabled={busy}>
                    {t("Standard")}
                  </button>
                  <button type="button" role="radio" aria-checked={dread}
                    className={`gs-launch-rung gs-launch-mode${dread ? " is-on" : ""}`}
                    title={dreadBlocked ?? undefined} aria-describedby={`${ids}-dread`}
                    onClick={() => onGameMode?.("dread")} disabled={busy || dreadBlocked !== null}>
                    {t("Dread")}
                  </button>
                  <button type="button" role="radio" aria-checked={blitz}
                    className={`gs-launch-rung gs-launch-mode${blitz ? " is-on" : ""}`}
                    title={blitzBlocked ?? undefined} aria-describedby={`${ids}-blitz`}
                    onClick={() => onGameMode?.("blitz")} disabled={busy || blitzBlocked !== null}>
                    {t("Blitz")}
                  </button>
                </div>
                {/* One line per mode; the picked mode's line stands alone. */}
                {!blitz && (
                  <p className="gs-launch-hint" id={`${ids}-dread`} data-testid="dread-note">
                    {dreadBlocked
                      ?? (dread
                        ? t("Dread is always ascension 10. Each act's fights carry its boss's dominion, and bosses are stronger.")
                        : t("Dread: ascension 10, with each act's fights under its boss's dominion."))}
                  </p>
                )}
                {!dread && (
                  <p className="gs-launch-hint" id={`${ids}-blitz`} data-testid="blitz-note">
                    {blitzBlocked
                      ?? (blitz
                        ? t("Blitz has its own ascension ladder, 0 to 5. Each act is a fixed line of eight rooms with no map: Neow gives 3 of 10 cards, rooms pay no gold, the merchant gives 3 free wares and the chest holds 2 relics.")
                        : t("Blitz: three short acts with no map, and an ascension ladder of its own."))}
                  </p>
                )}
              </section>
            )}

            {/* ---- ascension ---- */}
            <section className="gs-launch-field">
              <h2 className="gs-launch-label" id={`${ids}-asc`}>{t("Ascension")}</h2>
              <div className="gs-launch-ladder" role="radiogroup" aria-labelledby={`${ids}-asc`}>
                {ladder.map((l) => {
                  const on = l.level === level.level;
                  const lit = l.level <= level.level;
                  return (
                    <button
                      key={l.level} type="button" role="radio" aria-checked={on}
                      aria-label={t("ascension {n}", { n: l.level })} title={l.level === 0 ? t(l.title) : `A${l.level} · ${t(l.title)}`}
                      className={`gs-launch-rung${on ? " is-on" : ""}${lit ? " is-lit" : ""}`}
                      onClick={() => pickAscension(l.level)} disabled={busy || dread}
                    >
                      {l.level}
                    </button>
                  );
                })}
              </div>
              <div className="gs-launch-asc">
                <span className="gs-launch-ascbadge" aria-hidden="true">
                  <FlameIcon dim={level.level === 0} /><b>{level.level}</b>
                </span>
                <div className="gs-launch-asctext">
                  <div className="gs-launch-asctitle">
                    {level.level === 0
                      ? t(level.title)
                      : `${blitz ? t("Blitz ascension {n}", { n: level.level }) : t("Ascension {n}", { n: level.level })} · ${t(level.title)}`}
                  </div>
                  <ul className="gs-launch-mods">
                    {level.level === 0
                      ? <li><RichText text={t(level.description)} /></li>
                      : stacked.map((m) => (
                        <li key={m.level} className={m.level === level.level ? "is-top" : undefined}>
                          <RichText text={t(m.description)} />
                        </li>
                      ))}
                  </ul>
                </div>
              </div>
            </section>
          </div>

          {/* ---- hero ---- */}
          <div className="gs-launch-hero" aria-hidden="true">
            {slug && heroOk && (
              <img
                key={slug}
                src={assetUrl(characterArt(slug, `character_sprites/${slug}.png`))}
                alt="" draggable={false}
                onError={() => setHeroOk(false)}
              />
            )}
            <div className="gs-launch-heroname">{t(character)}</div>
          </div>
        </div>

        {/* ---- the two ways to play ---- */}
        <div className={`gs-launch-actions${onWatch ? "" : " gs-launch-actions--solo"}`}>
          <section className="gs-launch-action gs-launch-action--play">
            <h2 className="gs-launch-actiontitle">{t("Play yourself")}</h2>
            <p className="gs-launch-actiondesc" id={`${ids}-play`}>
              {t("You make every decision — map, cards, fights, shops. AI hints stay off unless you switch them on.")}
            </p>
            {onAdvisor && e2eAdvisable(e2eAvailable, character, gameVersion, backend) && (
              <div className="gs-launch-knobs">
                <label title={t("Who the AI hints, the policy panel, the choice coach and \"play best\" ask. Search: the combat MCTS in fights, the PPO run policy elsewhere. Instinct: one transformer, no search — for every decision (its ~21 MB net downloads the first time you pick it).")}>
                  {t("advisor")}{" "}
                  <select aria-label={t("advisor")} value={advisor} disabled={busy}
                    onChange={(e) => onAdvisor(e.target.value as Advisor)}>
                    <option value="search">{t("Search (MCTS + PPO)")}</option>
                    <option value="e2e">{t("Instinct")}</option>
                  </select>
                </label>
              </div>
            )}
            <button type="button" className="gs-runend-btn gs-runend-btn--primary gs-launch-go"
              onClick={onPlay} disabled={busy} aria-describedby={`${ids}-play`}>
              <span className="gs-runend-btnface">{t("Play yourself")}</span>
            </button>
          </section>

          {onWatch && (
            <section className="gs-launch-action gs-launch-action--watch">
              <h2 className="gs-launch-actiontitle">{t("Watch the AI play")}</h2>
              <p className="gs-launch-actiondesc" id={`${ids}-watch`}>
                {t("The run AI plays the whole run out while you watch. Stop it any time and take over from there.")}
              </p>
              <div className="gs-launch-knobs">
                <label title={e2eAvailable
                  ? t("Brain: Heuristic drives card rewards / shops / events / map / rest while the combat MCTS agent searches every fight. Instinct is one transformer that makes EVERY decision, fights included, with no search (Ironclad, Silent, Defect, Regent and Necrobinder on v0.111.0, the Fool on v0.113.0; its ~21 MB net downloads the first time you pick it).")
                  : t("Off-combat brain: the heuristic drives card rewards / shops / events / map / rest. Combat itself is always searched by the combat MCTS agent.")}>
                  {t("brain")}{" "}
                  <select
                    aria-label={t("brain")}
                    value={offCombatBrain}
                    disabled={busy}
                    onChange={(e) => onOffCombatBrain(e.target.value as OffCombatBrain)}
                  >
                    <option value="heuristic">{t("Heuristic")}</option>
                    {e2eAvailable && <option value="e2e">{t("Instinct")}</option>}
                  </select>
                </label>
                {offCombatBrain === "e2e" ? (
                  <span className="gs-launch-hint" data-testid="e2e-note">
                    {t(character)} · {e2eGameVersion(character)} · {t("act 1 rolled")} · {t("no combat search")}
                  </span>
                ) : (
                  <CombatBudget
                    preset={combat.preset} monsterSims={combat.monsterSims} bossSims={combat.bossSims}
                    threads={combat.threads} disabled={busy}
                    onPreset={combat.onPreset} onMonsterSims={combat.onMonsterSims} onBossSims={combat.onBossSims}
                  />
                )}
              </div>
              <button type="button" className="gs-runend-btn gs-runend-btn--secondary gs-launch-go"
                onClick={onWatch} disabled={busy || dread || blitz} aria-describedby={`${ids}-watch`}>
                <span className="gs-runend-btnface">{busy ? t("Running…") : t("Watch the AI play")}</span>
              </button>
              <p className="gs-launch-footnote">
                {dread || blitz
                  ? t("AI runs are standard runs. Switch the mode to Standard to watch one.")
                  : offCombatBrain === "e2e"
                    ? t("Instinct plays the run it was trained on: Rust engine, {version}, the rolled act 1.", { version: e2eGameVersion(character) })
                    : t("AI runs always use the Rust engine and the Overgrowth act 1.")}
              </p>
            </section>
          )}
        </div>

        {/* The start screen carries its own copy of the error. The shared
            banner lives in the right-hand log panel, which is the last place
            anyone looks while staring at a button that appears to do nothing. */}
        {error && <p className="gs-launch-error" role="alert">⚠ {error}</p>}

        {/* ---- dev knobs ---- */}
        <div className="gs-launch-advanced">
          <span className="gs-launch-advlabel">{t("Advanced")}</span>
          <label title={t("Which Act 1 to walk. STS2 ships two: Overgrowth and Underdocks. Roll = the real game's seed-driven pick (50/50), which is what makes a seed reproduce your actual run.")}>
            {t("act 1")}{" "}
            <select value={act1} onChange={(e) => onAct1(e.target.value)} disabled={busy}>
              <option value="roll">{t("Roll (as the game does)")}</option>
              <option value="overgrowth">{t("Overgrowth")}</option>
              <option value="underdocks">{t("Underdocks")}</option>
            </select>
          </label>
          <label title={t("Game engine: C# headless or the Rust port")}>
            {t("engine")}{" "}
            <select value={backend} onChange={(e) => onBackend(e.target.value)} disabled={busy}>
              {backends.map((b) => (
                <option key={b} value={b}>{ENGINE_LABELS[b] ?? b}</option>
              ))}
            </select>
          </label>
          {pickerVersions(gameVersions, runVersion).length > 1 && onGameVersion && (
            <label title={t("Which game build the engine plays. Builds differ in content and RNG seeding, so the same seed is a different run.")}>
              {t("version")}{" "}
              {/* Only the Rust engine plays a non-default build: with C#
                  the picker shows (and the run starts on) the default. */}
              <select aria-label={t("version")}
                value={runVersion}
                onChange={(e) => onGameVersion(e.target.value)}
                disabled={busy || backend !== "rust"}>
                {pickerVersions(gameVersions, runVersion).map((v) => (
                  <option key={v} value={v}>{gameVersionLabel(v)}</option>
                ))}
              </select>
            </label>
          )}
        </div>
      </div>
    </div>
  );
}

/** The HUD's ascension flame, dimmed to embers at A0. */
function FlameIcon({ dim }: { dim: boolean }) {
  const style: CSSProperties = dim ? { filter: "grayscale(0.7) brightness(0.55)" } : {};
  return (
    <svg className="gs-launch-flame" viewBox="0 0 24 28" aria-hidden="true" focusable="false" style={style}>
      <defs>
        <linearGradient id="gsLaunchFlame" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="#ffd24a" />
          <stop offset="46%" stopColor="#fb7a1c" />
          <stop offset="100%" stopColor="#c8380d" />
        </linearGradient>
      </defs>
      <path
        d="M12.4.7c.4 5.2-3.5 6.7-6 10.4C4.5 13.9 3.3 16.3 3.3 18.9c0 5.1 3.9 8.4 8.7 8.4s8.7-3.3 8.7-8.4c0-4.1-1.7-7.2-4.4-10.3-1.1 2.2-2.2 2.6-3.1 2-1 -3.4-1.3-6.6-.8-9.9Z"
        fill="url(#gsLaunchFlame)" stroke="#63200a" strokeWidth="1.8" strokeLinejoin="round" />
    </svg>
  );
}
