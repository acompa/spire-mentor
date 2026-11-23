import { useCallback, useEffect, useState } from "react";
import { fetchMctsRuns, fetchMctsAggregate, type MctsRunSummary, type MctsAggregate } from "../api";
import { t } from "../i18n";
import { servedByControlPlane } from "../bridge";
import { importAdvisorLogs } from "../liveLog";

/** Run history: a loss-pattern summary + table of every recorded run (any
 *  run log with a summary row, however it was played). Clicking a row opens
 *  that run in the timeline replay via `onOpen(path)`. The mode key and the
 *  `/api/mcts-runs` routes keep their old name; only the page was renamed. */
export function MctsRuns({ onOpen }: { onOpen: (path: string) => void }) {
  const [runs, setRuns] = useState<MctsRunSummary[]>([]);
  const [agg, setAgg] = useState<MctsAggregate | null>(null);
  const [loading, setLoading] = useState(false);
  // Re-fetch the run list + aggregate. The list is only otherwise loaded on
  // mount, so runs recorded after this page opened won't appear until the user
  // hits Refresh (or remounts the component).
  const refresh = useCallback(() => {
    setLoading(true);
    // In the bundled workbench, the advisor's logs this page has not stored
    // yet come in first (src/liveLog.ts).
    const imported = servedByControlPlane() ? importAdvisorLogs() : Promise.resolve(0);
    imported
      .then(() => Promise.all([
        fetchMctsRuns().then((d) => setRuns(d.runs)),
        fetchMctsAggregate().then(setAgg),
      ]))
      .finally(() => setLoading(false));
  }, []);
  useEffect(() => {
    refresh();
  }, [refresh]);
  return (
    <div className="mono" style={{ padding: 12 }}>
      <h2 style={{ display: "flex", alignItems: "center", gap: 12 }}>
        {t("Run history")}
        <button onClick={refresh} disabled={loading} title={t("look for newly recorded runs")}>
          {loading ? "…" : t("↻ Refresh")}
        </button>
      </h2>
      {agg && (
        <div style={{ marginBottom: 12 }}>
          <div>{t("runs {runs} · losses {losses}", { runs: agg.n_runs, losses: agg.n_losses })}</div>
          <div>{t("turn-1 Q: min {min} · max {max} · mean {mean}", { min: agg.turn1_q.min ?? "", max: agg.turn1_q.max ?? "", mean: agg.turn1_q.mean ?? "" })}</div>
          <div>{t("tags:")} {Object.entries(agg.pattern_tags).map(([k, v]) => `${k}×${v}`).join("  ")}</div>
        </div>
      )}
      <table>
        <thead><tr><th>{t("run")}</th><th>{t("outcome")}</th><th>{t("floor")}</th><th>{t("turn")}</th><th>{t("turn-1 Q")}</th><th>{t("enemy min%")}</th></tr></thead>
        <tbody>
          {runs.map((r) => (
            <tr key={r.path} onClick={() => onOpen(r.path)} style={{ cursor: "pointer" }}>
              <td>{r.stem}</td>
              <td style={{ color: r.won ? "var(--win, #6c6)" : "var(--loss, #c66)" }}>{r.won ? t("WON") : t("LOST")}</td>
              <td>{r.final_floor ?? ""}</td>
              <td>{r.final_turn ?? ""}</td>
              <td>{r.turn1_q?.toFixed(2)}</td>
              <td>{r.enemy_min_pct != null ? `${r.enemy_min_pct.toFixed(0)}%` : ""}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {!loading && runs.length === 0 && (
        // Say WHY it is empty. "runs 0" alone reads as "your runs were not
        // recorded" when the truth is usually "nothing has been recorded here
        // yet" — and on the Cloud Run deploy the logs dir starts empty on
        // every cold start.
        <div style={{ color: "var(--muted)", marginTop: 8 }}>
          {t("No finished runs recorded yet. A run lands here once it is over: when you end it, or press “Review replay” on its game-over screen.")}
        </div>
      )}
    </div>
  );
}
