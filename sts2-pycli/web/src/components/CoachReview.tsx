import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { CoachChoiceRecord, CoachRecapRecord, CoachRecord, CoachTurnRecord } from "../api";
import { choiceGap, turnCost, type AutopsyItem } from "../coach/coachRank";
import { choiceChipText, turnBlockLines } from "../coach/coachText";
import { t, tn } from "../i18n";

/**
 * Coach review — every graded decision of a run (follow / replay: what the
 * real-game advisor logged) or of this live session, as a bottom dock shaped
 * like the regret review (fixed, Escape closes, reports its height).
 *
 * Three tabs:
 *   - Autopsy: the run's costliest decisions, when the run ended in a death.
 *   - Fights: one recap per won fight (logged by the advisor).
 *   - All decisions: costly and tied turns by HP (else win-chance points),
 *     choices by how far the AI's pick out-weighed yours — two groups, never
 *     merged onto one scale.
 *
 * Every row seeks to its decision (`onSeek`) without closing the dock.
 */

export type CoachSeekTarget = CoachTurnRecord | CoachChoiceRecord | CoachRecapRecord;
export type CoachTab = "autopsy" | "fights" | "all";

export interface CoachAutopsyView {
  items: AutopsyItem[];
  floor?: number | null;
  turns_ungraded?: number;
  decisions_graded?: number;
}

function Row({ heading, lines, tone, onSeek }: {
  heading: string; lines: string[]; tone: string; onSeek?: () => void;
}) {
  return (
    <button type="button" onClick={onSeek} title={onSeek ? t("jump to this decision") : undefined}
      style={{ display: "block", width: "100%", textAlign: "left", cursor: onSeek ? "pointer" : "default",
        background: "var(--field-bg)", border: "1px solid var(--border)", borderRadius: 8,
        padding: "8px 12px", marginBottom: 8, color: "var(--text)", font: "inherit" }}>
      <div style={{ color: tone, fontWeight: 700 }}>{heading}</div>
      {lines.map((l, i) => <div key={i} style={{ fontSize: 12, marginTop: 2 }}>{l}</div>)}
    </button>
  );
}

const TONE: Record<string, string> = {
  blunder: "var(--hp)",
  slip: "var(--warn, #c79a3a)",
  tie: "var(--muted)",
  flag: "var(--warn, #c79a3a)",
  agree: "var(--accent-strong)",
  tossup: "var(--muted)",
};

function choiceLines(c: CoachChoiceRecord): string[] {
  const text = choiceChipText(c);
  if (text) return text.lines;
  const agree = c.verdict === "agree" ? t("AI agrees") : t("toss-up");
  return [`${agree} · ${t("you: {label}", { label: c.played?.label ?? "?" })}${c.peeked ? ` ${t("(peeked)")}` : ""}`];
}

export default function CoachReview({
  records, autopsy, initialTab, live = false, onSeek, onClose, onHeight,
}: {
  records: CoachRecord[];
  autopsy: CoachAutopsyView | null;
  initialTab?: CoachTab;
  live?: boolean;
  onSeek?: (r: CoachSeekTarget) => void;
  onClose: () => void;
  /** The dock's rendered height (px), so the parent can pad scroll regions
   *  out from under it; called with 0 on unmount. */
  onHeight?: (px: number) => void;
}) {
  const [tab, setTab] = useState<CoachTab>(initialTab ?? (autopsy ? "autopsy" : "all"));
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") { e.preventDefault(); onClose(); }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const rootRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el || !onHeight) return;
    const report = () => onHeight(el.offsetHeight);
    report();
    const ro = new ResizeObserver(report);
    ro.observe(el);
    return () => { ro.disconnect(); onHeight(0); };
  }, [onHeight]);

  const turns = records.filter((r): r is CoachTurnRecord => r.kind === "turn");
  const choices = records.filter((r): r is CoachChoiceRecord => r.kind === "choice");
  const recaps = records.filter((r): r is CoachRecapRecord => r.kind === "recap");
  const shownTurns = turns
    .filter((t) => t.verdict === "slip" || t.verdict === "blunder" || t.verdict === "tie")
    .sort((a, b) => turnCost(b) - turnCost(a));
  const shownChoices = choices.filter((c) => c.verdict !== "ungraded").sort((a, b) => choiceGap(b) - choiceGap(a));
  const ungraded = turns.filter((t) => t.verdict === "ungraded").length;
  const seek = (r: CoachSeekTarget) => (onSeek ? () => onSeek(r) : undefined);

  const tabs: [CoachTab, string][] = [
    ...(autopsy ? [["autopsy", t("Autopsy")] as [CoachTab, string]] : []),
    ["fights", t("Fights ({n})", { n: recaps.length })],
    ["all", t("All decisions ({n})", { n: shownTurns.length + shownChoices.length })],
  ];

  return (
    <div ref={rootRef} className="mono" role="region" aria-label={t("Coach Review")}
      style={{ position: "fixed", left: 0, right: 0, bottom: 0, zIndex: 50,
        display: "flex", flexDirection: "column", maxHeight: "46vh",
        background: "var(--bg)", borderTop: "1px solid var(--border)",
        boxShadow: "0 -8px 28px oklch(30% 0.04 255 / .35)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 16px",
        borderBottom: "1px solid var(--border)" }}>
        <b>{t("Coach review")}{live ? ` · ${t("live run")}` : ""}</b>
        <span role="tablist" style={{ display: "flex", gap: 4 }}>
          {tabs.map(([id, label]) => (
            <button key={id} role="tab" aria-selected={tab === id} onClick={() => setTab(id)}
              style={{ font: "inherit", fontWeight: tab === id ? 700 : 400 }}>{label}</button>
          ))}
        </span>
        <span style={{ marginLeft: "auto" }} />
        <button onClick={onClose} aria-label={t("Close")}>✕</button>
      </div>
      <div style={{ padding: 16, overflow: "auto" }}>
        {tab === "autopsy" && autopsy && (
          <>
            <div style={{ marginBottom: 8 }}>
              <b>{t("Run autopsy")}{autopsy.floor != null ? ` · ${t("died on floor {floor}", { floor: autopsy.floor })}` : ""}</b>
            </div>
            {autopsy.items.length === 0 && <div>{t("No costly decisions found.")}</div>}
            {autopsy.items.map((item, i) => item.kind === "turn"
              ? <Row key={item.record.id} tone={TONE[item.record.verdict] ?? "var(--text)"}
                  heading={`${i + 1}. ${t("Combat turn")}${item.record.floor != null ? ` · ${t("floor {floor}", { floor: item.record.floor })}` : ""}`}
                  lines={turnBlockLines(item.record)} onSeek={seek(item.record)} />
              : <Row key={item.record.id} tone={TONE.flag}
                  heading={`${i + 1}. ${t(item.record.label_kind)} · ${t("you: {label}", { label: item.record.played?.label ?? "?" })}`}
                  lines={choiceLines(item.record)} onSeek={seek(item.record)} />)}
            {(autopsy.decisions_graded != null || autopsy.turns_ungraded != null) && (
              <div style={{ color: "var(--muted)", fontSize: 11 }}>
                {tn(autopsy.decisions_graded ?? 0, "{n} decision graded", "{n} decisions graded")}
                {" · "}{tn(autopsy.turns_ungraded ?? 0, "{n} turn ungraded", "{n} turns ungraded")}
              </div>
            )}
          </>
        )}
        {tab === "fights" && (
          <>
            {recaps.length === 0 && (
              <div style={{ color: "var(--muted)" }}>
                {live ? t("Fight recaps come from the real-game advisor (follow mode).") : t("No fight recaps in this run.")}
              </div>
            )}
            {recaps.map((r) => (
              <div key={r.id} style={{ marginBottom: 12 }}>
                <Row tone="var(--text)" heading={t("Fight recap · floor {floor}", { floor: r.floor ?? "?" })}
                  lines={[`${t("−{n} HP", { n: r.hp_lost })}${r.turns_ungraded > 0 ? ` · ${tn(r.turns_ungraded, "{n} turn ungraded", "{n} turns ungraded")}` : ""}`]}
                  onSeek={seek(r)} />
                {r.costliest.map((tr) => (
                  <div key={tr.id} style={{ marginLeft: 16 }}>
                    <Row tone={TONE[tr.verdict] ?? "var(--text)"} heading={tr.verdict === "blunder" ? t("⚠ blunder") : t("▽ suboptimal")}
                      lines={turnBlockLines(tr)} onSeek={seek(tr)} />
                  </div>
                ))}
              </div>
            ))}
          </>
        )}
        {tab === "all" && (
          <>
            <div style={{ fontWeight: 700, margin: "0 0 6px" }}>{t("Combat turns · by HP lost")}</div>
            {shownTurns.length === 0 && <div style={{ color: "var(--muted)", marginBottom: 8 }}>{t("No costly turns.")}</div>}
            {shownTurns.map((tr) => (
              <Row key={tr.id} tone={TONE[tr.verdict] ?? "var(--text)"}
                heading={tr.verdict === "blunder" ? t("⚠ blunder") : tr.verdict === "slip" ? t("▽ suboptimal") : t("≈ toss-up")}
                lines={turnBlockLines(tr)} onSeek={seek(tr)} />
            ))}
            {ungraded > 0 && (
              <div style={{ color: "var(--muted)", fontSize: 11, marginBottom: 8 }}>
                {tn(ungraded, "{n} turn ungraded (too close to call, or not gradable)", "{n} turns ungraded (too close to call, or not gradable)")}
              </div>
            )}
            <div style={{ fontWeight: 700, margin: "8px 0 6px" }}>{t("Run choices · by how strongly the AI disagreed")}</div>
            {shownChoices.length === 0 && <div style={{ color: "var(--muted)" }}>{t("No graded choices.")}</div>}
            {shownChoices.map((c) => (
              <Row key={c.id} tone={TONE[c.verdict] ?? "var(--text)"}
                heading={`${t(c.label_kind)} · ${t("you: {label}", { label: c.played?.label ?? "?" })}`}
                lines={choiceLines(c)} onSeek={seek(c)} />
            ))}
          </>
        )}
      </div>
    </div>
  );
}
