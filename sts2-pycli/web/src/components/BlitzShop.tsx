import type { CSSProperties } from "react";
import type { Step, ChooseHandler } from "../api";
import { assetUrl } from "../api";
import { blitzPicksLine, type BlitzWare } from "../blitz";
import RichText from "./RichText";
import TermTip from "./TermTip";
import SpineSprite from "./SpineSprite";
import { MERCHANT_H, MOTES_FAR, MOTES_NEAR } from "./Shop";
import "../skins/shop.css";
import { t } from "../i18n";

/** What a ware's tag says about its state. */
function wareState(w: BlitzWare): string {
  if (w.status === "Used") return t("Taken");
  if (w.status === "Disabled") return t("No picks left");
  return t("Free");
}

/** Ware slot centres on the rug (`merchant_inventory.tscn`
 *  `MerchantBlitzEntry*`), in the rug's own pixels. */
const BLITZ_SLOTS: readonly [number, number][] = [
  [415, 320], [896, 293], [1291, 355], [437, 679], [831, 637], [1247, 689],
];
const RUG_W = 1747;
const RUG_H = 978;

/** `RandomUpgrades` -> `random_upgrades` (the game's `StringHelper.SnakeCase`
 *  on the entry type, which names its icon). */
function snake(kind: string): string {
  return kind.replace(/(?<=[a-z0-9])(?=[A-Z])/g, "_").toLowerCase();
}

function stripTags(s: string): string {
  return s.replace(/\[\/?[a-z]+\]/g, "");
}

/**
 * The Blitz merchant (GH #1347): the same tent as the standard shop, but
 * nothing here costs gold. He lays out six wares, five regular and one
 * cursed (it also hands out a Curse), and lets you take three. Each ware is
 * a service or a reward, drawn as the game draws it: an icon on the rug,
 * named and described in its hover tip (GH #1597).
 *
 * A ware that needs a card from your deck (upgrade, transform, removal,
 * enchant) opens the usual card pick, which can be cancelled; a ware that
 * hands out card rewards opens them one after another. Either way the run
 * comes back here. Interactive in Play (`onChoose`); read-only in Replay,
 * where the ware the run took is marked.
 */
export default function BlitzShop({ step, onChoose }: { step: Step; onChoose?: ChooseHandler }) {
  const s = step.state;
  const wares: BlitzWare[] = s.blitz_wares ?? [];
  const remaining = s.blitz_choices_remaining ?? 0;
  const action = step.chosen_action as { action?: string; args?: { index?: number } } | null;
  const taken = onChoose == null && action?.action === "blitz_choice" ? action.args?.index : undefined;
  return (
    <div
      className="gs-shop gs-scene gs-blitzshop"
      style={
        {
          "--gs-scene-bg": `url(${assetUrl("scene_bgs/merchant_room/bottom/shop_merchant_bottom.png")})`,
          "--gs-shop-panel": `url(${assetUrl("ui/common_ui/submenu_panel_short.png")})`,
        } as CSSProperties
      }
    >
      {/* The standard shop's room: see Shop.tsx for what each layer is. */}
      <div className="gs-shop-scenery" aria-hidden="true">
        <div className="gs-shop-drape is-left" />
        <div className="gs-shop-drape is-right" />
        <div className="gs-shop-haze" />
        <div className="gs-shop-lightpool" />
        <div className="gs-shop-lanternglow" />
        <div className="gs-shop-merchant">
          <SpineSprite
            rigBase={assetUrl("room_rigs/shop_merchant_top/shop_merchant_top")}
            fallbackHeight={MERCHANT_H}
            alt=""
          />
        </div>
      </div>

      <div className="gs-shop-content">
        <header className="gs-shop-header">
          <div className="gs-shop-sign">
            <span className="gs-shop-signtitle">{t("Merchant")}</span>
          </div>
          <p className="gs-blitz-picks" data-testid="blitz-picks">
            <RichText text={blitzPicksLine(remaining)} />
          </p>
        </header>

        {/* The game's Blitz inventory (`merchant_inventory.tscn` BlitzSlots):
            the merchant's rug, six ware icons laid on it, each 240 px on a
            1747 x 978 rug. The name and text live in the hover tip, as in the
            game. A taken ware's icon is gone; once the picks run out the rest
            turn dark gray. */}
        <div className="gs-blitz-rug" role="group" aria-label={t("The merchant's wares")}
          style={{ backgroundImage: `url(${assetUrl("rooms/merchant_room/shop_rug.png")})` }}>
          {wares.map((w, i) => {
            const [x, y] = BLITZ_SLOTS[i % BLITZ_SLOTS.length];
            const cls = "gs-blitz-slot"
              + (w.cursed ? " is-cursed" : "")
              + (w.rare ? " is-rare" : "")
              + (w.status === "Used" ? " is-used" : "")
              + (w.status === "Disabled" ? " is-disabled" : "")
              + (taken === w.index ? " is-chosen" : "");
            return (
              <TermTip key={w.index} tip={{ title: w.title, description: w.description }}
                className="gs-blitz-slotwrap"
                style={{ left: `${(x / RUG_W) * 100}%`, top: `${(y / RUG_H) * 100}%` }}>
                <button type="button" className={cls}
                  data-kind={w.kind} data-status={w.status}
                  aria-label={`${w.title}${w.cursed ? ` ${t("(cursed)")}` : ""}${w.rare ? ` ${t("(rare)")}` : ""}: ${stripTags(w.description)} ${taken === w.index ? t("Taken here") : wareState(w)}`}
                  disabled={!onChoose || !w.enabled}
                  onClick={() => onChoose?.("blitz_choice", { index: w.index })}>
                  <img className="gs-blitz-icon" alt="" aria-hidden="true"
                    src={assetUrl(`v0.113.0/blitz/entry_${snake(w.kind)}.png`)} />
                  {taken === w.index && <span className="gs-blitz-chosen">{t("Taken here")}</span>}
                </button>
              </TermTip>
            );
          })}
          {wares.length === 0 && <span className="gs-shop-empty">{t("Nothing on the rug")}</span>}
        </div>

        <div className="gs-shop-service gs-blitz-service">
          <button className="gs-shop-hexbtn" disabled={!onChoose}
            onClick={() => onChoose?.("leave_room", {})}>
            <span className="gs-shop-hexface">{t("Leave shop")}</span>
          </button>
        </div>
      </div>

      <div className="gs-shop-motes" aria-hidden="true">
        <i className="is-far" style={{ backgroundImage: MOTES_FAR }} />
        <i className="is-near" style={{ backgroundImage: MOTES_NEAR }} />
      </div>
      <div className="gs-shop-grade" aria-hidden="true" />
      <div className="gs-shop-warm" aria-hidden="true" />
      <div className="gs-shop-grain" aria-hidden="true" />
      <div className="gs-shop-fg" aria-hidden="true" />
    </div>
  );
}
