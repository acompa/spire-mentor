import type { Step, ChooseHandler } from "../api";
import CardReward from "./CardReward";
import Shop from "./Shop";
import BlitzShop from "./BlitzShop";
import ChestRewards from "./ChestRewards";
import EventScreen from "./EventScreen";
import RestSite from "./RestSite";
import CardSelect from "./CardSelect";
import BundleSelect from "./BundleSelect";
import GameOver from "./GameOver";
import ChoicePanel from "./ChoicePanel";
import { t } from "../i18n";

/**
 * Routes a non-combat decision step to its bespoke screen, in both modes.
 * ``onChoose`` is provided in Play (interactive) and omitted in Replay
 * (read-only). ``combat_play`` and ``map_select`` are handled by the caller
 * (the interactive Board, and the connected MapView/MapChoices respectively).
 */
export default function DecisionScreen({
  step,
  onChoose,
  onNewRun,
  onReviewReplay,
  onCoachReview,
}: {
  step: Step;
  onChoose?: ChooseHandler;
  onNewRun?: () => void;
  onReviewReplay?: () => void;
  onCoachReview?: () => void;
}) {
  // GH #1347: the Blitz chest, two relics over two rounds (the `rewards`
  // decision with `chest_picks_left`). Any other `rewards` screen keeps the
  // generic list at the foot of the switch.
  if (step.decision === "rewards" && step.state.chest_picks_left != null) {
    return <ChestRewards step={step} onChoose={onChoose} />;
  }
  switch (step.decision) {
    case "card_reward":
      return <CardReward step={step} onChoose={onChoose} />;
    case "shop":
      // GH #1347: the Blitz merchant sells nothing for gold; he has a
      // screen of his own.
      return step.state.blitz_choices_remaining != null
        ? <BlitzShop step={step} onChoose={onChoose} />
        : <Shop step={step} onChoose={onChoose} />;
    case "event_choice":
      return <EventScreen step={step} onChoose={onChoose} />;
    case "rest_site":
      return <RestSite step={step} onChoose={onChoose} />;
    case "card_select":
      return <CardSelect step={step} onChoose={onChoose} />;
    case "bundle_select":
      return <BundleSelect step={step} onChoose={onChoose} />;
    case "game_over":
      return <GameOver step={step} onNewRun={onNewRun} onReviewReplay={onReviewReplay} onCoachReview={onCoachReview} />;
    default:
      // Unknown decision: the generic action list keeps the run unstuck (Play),
      // or a muted note (Replay, where there are no actions to take).
      return onChoose ? (
        <ChoicePanel title={step.action_label || step.decision} options={step.state.options ?? []} onChoose={onChoose} />
      ) : (
        <div className="mono" style={{ padding: 20, color: "var(--muted)" }}>
          {step.decision} — {step.action_label || t("(no dedicated view)")}
        </div>
      );
  }
}
