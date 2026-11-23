import { useState } from "react";
import { api, type ChoiceCheck, type ChoiceGrade } from "../api";
import { ordinal } from "../coach/coachText";
import { CHECK_CONFIG, checkSummary } from "./choiceCheck";
import { t } from "../i18n";

type CheckState =
  | { kind: "idle" }
  | { kind: "running"; done: number; total: number }
  | { kind: "done"; check: ChoiceCheck }
  | { kind: "failed"; error: string };

/** Coach chip for the run-level choice just made, graded by agreement with
 *  the AI's policy — never as a win-probability cost. "check it" runs the
 *  slow rollout comparison on demand. */
export default function ChoiceChip({ grade, onDismiss }: {
  grade: ChoiceGrade; onDismiss: () => void;
}) {
  const [check, setCheck] = useState<CheckState>({ kind: "idle" });
  const { verdict, played, top, kind } = grade;
  // Graded against the e2e advisor instead of the PPO run policy.
  const ai = grade.policy === "e2e" ? t("Instinct") : "AI";
  const flag = verdict === "flag";
  const tone = flag ? "var(--warn, #c79a3a)" : "var(--accent-strong)";

  async function runCheck() {
    setCheck({ kind: "running", done: 0, total: 0 });
    try {
      const r = await api.live.lastChoiceCheckStream(grade.node_id, CHECK_CONFIG, (ev) => {
        if (ev.type === "start") setCheck({ kind: "running", done: 0, total: ev.total });
        else setCheck({ kind: "running", done: ev.done, total: ev.total });
      });
      setCheck(r.error ? { kind: "failed", error: r.error } : { kind: "done", check: r });
    } catch (e) {
      setCheck({ kind: "failed", error: String(e) });
    }
  }

  const summary = check.kind === "done" ? checkSummary(check.check) : null;
  return (
    <div className="mono" style={{ marginBottom: 8, padding: "6px 10px", borderRadius: 4,
      color: tone, border: `1px solid ${tone}`, background: "var(--field-bg)",
      fontSize: 11, display: "flex", gap: 8, alignItems: "baseline" }}>
      <div style={{ flex: 1 }}>
        {verdict === "agree" && <>✓ {t("{ai} agrees", { ai })} — <b>{played.label}</b></>}
        {verdict === "tossup" && (
          <>
            <div>{t("✓ toss-up")} — <b>{played.label}</b> · {t("no strong {ai} preference", { ai })}</div>
            <div>{kind} · {t("{ai}'s top picks: {picks}", { ai, picks: grade.alternatives.map((a) => a.label).join(", ") || top.label })}</div>
          </>
        )}
        {flag && (
          <>
            <div>{t(kind)} · {t("you:")} <b>{played.label}</b></div>
            <div>
              {t("{ai} would take:", { ai })} <b>{top.label}</b> ({t(grade.strength)}) · {t("{label} was its {ordinal} choice", { label: played.label, ordinal: ordinal(played.rank) })}
            </div>
          </>
        )}
        {grade.checkable && check.kind === "idle" && (
          <button type="button" onClick={runCheck}
            title={t("Play the rest of the act out from this position both ways — moves wait until it finishes")}
            style={{ font: "inherit", marginTop: 4, background: "none", color: "inherit",
              border: "1px solid currentColor", borderRadius: 3, cursor: "pointer", padding: "0 6px" }}>
            {t("check it")}
          </button>
        )}
        {check.kind === "running" && (
          <div style={{ color: "var(--muted)" }}>{t("checking… {done}/{total}", { done: check.done, total: check.total })}</div>
        )}
        {summary && <div style={{ color: "var(--text)" }}>{summary}</div>}
        {check.kind === "failed" && <div style={{ color: "var(--muted)" }}>{t("check failed: {error}", { error: check.error })}</div>}
      </div>
      <button type="button" onClick={onDismiss} aria-label={t("Dismiss")}
        style={{ font: "inherit", background: "none", border: "none", color: "var(--muted)",
          cursor: "pointer" }}>✕</button>
    </div>
  );
}
