import { Fragment, type ReactNode } from "react";
import type { CombatLogRow } from "../api";
import { getLang, t } from "../i18n";

/**
 * The one place a combat-log display row becomes a coloured sentence.
 *
 * Shared by both history logs (LiveLog / EventLog), so a hit reads
 * identically everywhere: actors coloured by side, amounts and sources in
 * gold.
 */

/** Colour for a creature name: the player's side reads cool, the enemy's hot. */
function sideColor(side: string | null): string {
  return side === "enemy" ? "var(--hp)" : "var(--block)";
}

/** A translated template with `{name}` slots filled by React nodes (the
 *  sentence is translated whole, so the slots may move in Chinese). */
function fill(template: string, nodes: Record<string, ReactNode>): ReactNode[] {
  return template.split(/(\{\w+\})/g).map((part, i) => {
    const m = /^\{(\w+)\}$/.exec(part);
    return <Fragment key={i}>{m && m[1] in nodes ? nodes[m[1]] : part}</Fragment>;
  });
}

/** The coloured spans for one row, falling back to its plain `text`. */
export function LogLine({ row }: { row: CombatLogRow }) {
  const actor = row.actor ? (
    <span style={{ color: sideColor(row.actor_side), fontWeight: 600 }}>{row.actor}</span>
  ) : null;
  const target = row.target ? (
    <span style={{ color: sideColor(row.target_side), fontWeight: 600 }}>{row.target}</span>
  ) : null;
  const amount = (
    <span style={{ color: "var(--gold)", fontWeight: 700 }}>{row.amount}</span>
  );
  const src = row.source ? <span style={{ color: "var(--gold)" }}>{row.source}</span> : null;
  const noun = <span style={{ color: "var(--gold)" }}>{row.noun_display ?? row.noun}</span>;

  switch (row.kind) {
    case "damage":
      return actor ? (
        src
          ? <>{fill(t("{actor} dealt {amount} damage to {target} using {source}."), { actor, amount, target, source: src })}</>
          : <>{fill(t("{actor} dealt {amount} damage to {target}."), { actor, amount, target })}</>
      ) : (
        <>{fill(t("{target} took {amount} damage."), { target, amount })}</>
      );
    case "block":
      return <>{fill(t("{actor} gained {amount} {noun}."), { actor, amount, noun })}</>;
    case "power":
      return row.verb === "applied" ? (
        <>{fill(t("{actor} applied {amount} {noun} to {target}."), { actor, amount, noun, target })}</>
      ) : (
        <>{fill(t("{target} gained {amount} {noun}."), { target, amount, noun })}</>
      );
    case "monster_move":
      return <>{fill(t("{actor} used {noun}."), { actor, noun })}</>;
    case "potion":
      return <>{fill(t("{actor} drank {noun}."), { actor, noun })}</>;
    case "orb_channeled":
      return <>{fill(t("{actor} channeled {noun}."), { actor, noun })}</>;
    case "orb_evoked":
      return <>{fill(t("{actor} evoked {noun}."), { actor, noun })}</>;
    case "card_auto_played":
      return src
        ? <>{fill(t("{actor} auto-played {noun} via {source}."), { actor, noun, source: src })}</>
        : <>{fill(t("{actor} auto-played {noun}."), { actor, noun })}</>;
    case "card_exhausted":
    case "card_discarded":
    case "card_generated":
      // `verb` is an English value; in Chinese the engine's own sentence reads right.
      return row.actor && row.verb && row.noun && getLang() !== "zh" ? (
        <>{actor} {row.verb} {noun}.</>
      ) : (
        <>{row.text}</>
      );
    // Every other kind (including anything a newer engine adds) renders the
    // server's own sentence rather than being dropped.
    default:
      return <>{row.text}</>;
  }
}
