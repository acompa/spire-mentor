import type { Step, ChooseHandler } from "../api";
import { assetUrl } from "../api";
import type { ChestReward } from "../blitz";
import SpineSprite from "./SpineSprite";
import RichText from "./RichText";
import "../skins/event.css";
import { t } from "../i18n";

/** Chest height in px: a constant, for the reason TreasureReveal's is. */
const CHEST_H = 240;

/**
 * The Blitz chest (GH #1347). Unlike a standard chest, which the engine
 * opens for you (see TreasureReveal), this one is a decision: it holds two
 * relics and you take them one at a time, over two rounds. Skip closes the
 * chest for good, with whatever is still in it left behind, as the game's
 * one SKIP button does. There is no gold in it.
 *
 * Interactive in Play (`onChoose`); read-only in Replay, where the relic the
 * run took is marked.
 */
export default function ChestRewards({ step, onChoose }: { step: Step; onChoose?: ChooseHandler }) {
  const s = step.state;
  const rewards: ChestReward[] = s.rewards ?? [];
  const left = s.chest_picks_left ?? rewards.length;
  const act = Math.min(3, Math.max(1, s.act ?? 1));
  const action = step.chosen_action as { action?: string; args?: { reward_index?: number } } | null;
  const taken = onChoose == null && action?.action === "take_reward" ? action.args?.reward_index : undefined;
  const skipped = onChoose == null && (action?.action === "skip_reward" || action?.action === "proceed");
  return (
    <div className="gs-scene gs-treasure gs-treasure-pick" role="group" aria-label={t("Treasure chest")}>
      <div className="gs-treasure-banner">
        <img src={assetUrl("ui/reward_screen/reward_banner.png")} alt="" aria-hidden="true" />
        <h2 className="gs-treasure-title">{left > 1 ? t("Choose a Relic") : t("One More Relic")}</h2>
      </div>
      <div className="gs-treasure-rig" data-act={act}>
        <SpineSprite
          rigBase={assetUrl(`room_rigs/treasure_act_${act}/treasure_act_${act}`)}
          holdSetup
          actionAnim="animation"
          actionNonce={1}
          holdLast
          fallbackHeight={CHEST_H}
          fallback={assetUrl("run_history/treasure.png")}
          alt=""
        />
      </div>
      <div className="gs-treasure-haul">
        <p className="gs-treasure-rounds" data-testid="chest-rounds">
          {left > 1
            ? t("The chest holds two relics. Take one now and the other next.")
            : t("Take the last relic, or leave it.")}
        </p>
        {rewards.map((r) => (
          <button
            key={r.index} type="button"
            className={`gs-treasure-relic gs-treasure-take${taken === r.index ? " is-chosen" : ""}`}
            disabled={!onChoose}
            onClick={() => onChoose?.("take_reward", { reward_index: r.index })}
          >
            {r.image && <img src={r.image} alt="" aria-hidden="true" />}
            <span className="gs-treasure-plaque">
              <span className="gs-treasure-relicname">{r.name}</span>
              {r.description && (
                <span className="gs-treasure-relictext"><RichText text={r.description} /></span>
              )}
            </span>
          </button>
        ))}
        {taken != null && <div className="gs-treasure-gold">{t("taken here")}</div>}
        {skipped && <div className="gs-treasure-gold">{t("skipped")}</div>}
      </div>
      <button type="button" className="gs-treasure-dismiss" disabled={!onChoose}
        onClick={() => onChoose?.("skip_reward", {})}>
        {t("Skip")}
      </button>
    </div>
  );
}
