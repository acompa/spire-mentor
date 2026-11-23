import type { MctsConfig } from "../api";
import NumberInput from "./NumberInput";
import { t } from "../i18n";

const SIM_CHOICES = [300, 1000, 2000, 3000, 5000, 10000, 50000, 150000];
const THREAD_CHOICES = [1, 2, 4, 8, 16];

/**
 * Compact inline editor for the MCTS knobs (sims / explore / depth / rollout /
 * seed / prune / threads). Unlike the search-tree modal's draft+run panel, this
 * applies each change immediately via `onChange` — intended for the hint
 * config, where the next hint (H / play-best / autoplay) picks up the new
 * values.
 */
export default function MctsControls({
  config,
  onChange,
  disabled,
  label = t("hint"),
}: {
  config: MctsConfig;
  onChange: (config: MctsConfig) => void;
  disabled?: boolean;
  label?: string;
}) {
  const setK = <K extends keyof MctsConfig>(k: K, v: MctsConfig[K]) =>
    onChange({ ...config, [k]: v });
  return (
    <span className="mono" style={{ display: "inline-flex", alignItems: "center", gap: 8, flexWrap: "wrap", fontSize: 12, color: "var(--muted)" }}>
      <span>{label}:</span>
      <label title={t("number of MCTS playouts — more = stronger but slower")}>{t("sims")}{" "}
        <select value={config.sims} disabled={disabled} onChange={(e) => setK("sims", Number(e.target.value))}>
          {SIM_CHOICES.map((s) => <option key={s} value={s}>{s}</option>)}
        </select>
      </label>
      <label title={t("UCT exploration constant — higher explores more, lower exploits the best line")}>{t("explore")}{" "}
        <NumberInput min={0} step={0.1} value={config.c_puct} disabled={disabled} width={64}
          onChange={(v) => setK("c_puct", v)} />
      </label>
      <label title={t("rollout simulation depth in plies — how far each random playout looks ahead")}>{t("depth")}{" "}
        <NumberInput min={1} max={500} value={config.depth} disabled={disabled} width={80}
          onChange={(v) => setK("depth", v)} />
      </label>
      <label title={t("rollout policy — heuristic (expert priorities) or uniform (unbiased random)")}>{t("rollout")}{" "}
        <select value={config.rollout_policy} disabled={disabled}
          onChange={(e) => setK("rollout_policy", e.target.value as MctsConfig["rollout_policy"])}>
          <option value="heuristic">{t("heuristic")}</option>
          <option value="uniform">{t("uniform")}</option>
        </select>
      </label>
      <label title={t("RNG seed — fixes the determinization draw; change to re-roll")}>{t("seed")}{" "}
        <NumberInput min={0} value={config.seed} disabled={disabled} width={80}
          onChange={(v) => setK("seed", v)} />
      </label>
      <label title={t("prune move orders the engine proves dominated — e.g. never search Strike-then-Beam-Cell when Beam-Cell-then-Strike is the same turn with more damage")}>
        <input type="checkbox" checked={config.dominance_pruning ?? false} disabled={disabled}
          onChange={(e) => setK("dominance_pruning", e.target.checked)} />{" "}{t("prune")}
      </label>
      <label title={t("root-parallel workers — splits the sim budget across independent trees for a ~Nx wall-clock speedup at the same total sims (needs free cores), at the cost of N shallower trees and a flatter visit distribution. Defaults to 1: one deterministic tree, directly comparable to the search-tree inspector.")}>{t("threads")}{" "}
        <select value={config.threads ?? 1} disabled={disabled}
          onChange={(e) => setK("threads", Number(e.target.value))}>
          {/* Merge in the current value so an off-preset core count (e.g. 12) renders. */}
          {Array.from(new Set([...THREAD_CHOICES, config.threads ?? 1]))
            .sort((a, b) => a - b)
            .map((t) => <option key={t} value={t}>{t}</option>)}
        </select>
      </label>
    </span>
  );
}
