import { useState } from "react";
import type { SweepListEntry } from "./api";
import { card, shortTime } from "./styles";
import { ProgressBar, StatusInline } from "./widgets";
import { t, tn } from "../i18n";

// ---------------------------------------------------------------------------
// SweepList
// ---------------------------------------------------------------------------
/** Strip a known suffix off the encounter name for a compact label. */
function shortEnc(enc: string): string {
  return enc.replace(/_(NORMAL|ELITE|BOSS|WEAK)$/, "");
}

function SweepRow({
  s,
  selected,
  onSelect,
}: {
  s: SweepListEntry;
  selected: boolean;
  onSelect: (id: string) => void;
}) {
  const positions = s.spec.positions?.length ?? s.spec.start_states?.length ?? 0;
  const encLabel = positions
    ? tn(positions, "{n} position", "{n} positions")
    : s.spec.encounters.length === 1
      ? t("{enc} enc", { enc: `${shortEnc(s.spec.encounters[0])}${
          s.spec.ascensions.length === 1 ? `+${s.spec.ascensions[0]}` : ""
        }` })
      : t("{n} enc", { n: s.spec.encounters.length });
  // New specs carry `decks`; legacy ones only `deck_preset` (count as 1).
  const deckCount = s.spec.decks?.length ?? 1;
  const deckLabel = positions ? "" : ` · ${tn(deckCount, "{n} deck", "{n} decks")}`;
  const summary = `${s.spec.name} · ${encLabel}${deckLabel} · ${t("{n} strats", { n: s.spec.strategies.length })} · ${positions ? t("{n} trials", { n: s.spec.n_seeds }) : t("{n} seeds", { n: s.spec.n_seeds })}`;
  return (
    <div
      onClick={() => onSelect(s.sweep_id)}
      title={`${s.spec.encounters.join(", ")} · A${s.spec.ascensions.join(",")} · ${s.spec.strategies.join("/")}`}
      style={{
        display: "flex",
        alignItems: "center",
        gap: 8,
        padding: "3px 8px",
        cursor: "pointer",
        background: selected ? "var(--accent-weak)" : undefined,
        borderRadius: 4,
        fontSize: 13,
        whiteSpace: "nowrap",
      }}
    >
      <span style={{ color: "var(--muted)" }}>{selected ? "▾" : "▸"}</span>
      <span
        style={{
          flex: 1,
          overflow: "hidden",
          textOverflow: "ellipsis",
        }}
      >
        {summary}
      </span>
      {s.status === "running" && <ProgressBar done={s.done} total={s.total} width={60} />}
      <StatusInline s={s} />
      <span style={{ color: "var(--muted)", fontSize: 12, minWidth: 44, textAlign: "right" }}>
        {shortTime(s.mtime)}
      </span>
    </div>
  );
}

export default function SweepHistory({
  sweeps,
  selected,
  onSelect,
}: {
  sweeps: SweepListEntry[];
  selected: string | null;
  onSelect: (id: string) => void;
}) {
  const [showAll, setShowAll] = useState(false);
  const RECENT = 8;
  // Server already sorts newest-first by mtime; trust that order.
  const shown = showAll ? sweeps : sweeps.slice(0, RECENT);
  return (
    <div style={{ ...card, padding: 8 }}>
      <div
        style={{
          fontSize: 12,
          color: "var(--muted)",
          fontWeight: 600,
          padding: "2px 8px 6px",
        }}
      >
        {t("History")}
      </div>
      {sweeps.length === 0 && (
        <p style={{ color: "var(--muted)", padding: "0 8px" }}>{t("No sweeps yet.")}</p>
      )}
      {shown.map((s) => (
        <SweepRow
          key={s.sweep_id}
          s={s}
          selected={s.sweep_id === selected}
          onSelect={onSelect}
        />
      ))}
      {sweeps.length > RECENT && (
        <button
          onClick={() => setShowAll((v) => !v)}
          style={{
            margin: "6px 8px 2px",
            fontSize: 12,
            background: "none",
            border: "none",
            color: "var(--accent)",
            cursor: "pointer",
            padding: 0,
          }}
        >
          {showAll ? t("show less") : t("show all ({n})", { n: sweeps.length })}
        </button>
      )}
    </div>
  );
}
