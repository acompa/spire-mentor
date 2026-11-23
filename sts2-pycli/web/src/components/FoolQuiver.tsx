import { assetUrl, type ArrowStatus } from "../api";
import { characterArt } from "../characters";
import { t } from "../i18n";

/** The Fool's bow and quiver (`fool_combat_ui.tscn` + `fool_arrow.tscn`),
 *  drawn bottom-left above the energy orb as the game does.
 *
 *  One arrow per `ArrowSlot`, in gain order (`NFoolCombatUi.UpdateArrows`):
 *  - `retrieved` — the full arrow, ready to shoot;
 *  - `away` (stuck in an enemy) — only its dark outline, the arrow art faded
 *    out as it swells (`NFoolArrowUi.UpdateVisuals`);
 *  - `destroyed` — not drawn (`Visible = Status != Destroyed`).
 *  Odd slots are flipped upside down and sit a little higher, the game's
 *  alternating head-up/head-down packing. */
export default function FoolQuiver({ arrows }: { arrows: ArrowStatus[] }) {
  const art = (name: string) => assetUrl(characterArt("fool", `ui/fool/${name}.png`));
  const ready = arrows.filter((a) => a === "retrieved").length;
  return (
    <div className="gs-quiver" data-testid="fool-quiver"
      title={t("arrows: {ready} ready of {total}", { ready, total: arrows.filter((a) => a !== "destroyed").length })}>
      <img className="gs-quiver-bow" alt="" aria-hidden="true" draggable={false} src={art("bow")} />
      <div className="gs-quiver-arrows">
        {arrows.map((status, i) => (
          <div key={i} className={`gs-quiver-arrow${i % 2 ? " gs-quiver-flip" : ""}`}
            data-status={status} hidden={status === "destroyed"}>
            <img className="gs-quiver-stroke" alt="" aria-hidden="true" draggable={false} src={art("arrow_stroke")} />
            <img className="gs-quiver-visual" alt="" aria-hidden="true" draggable={false} src={art("arrow")} />
          </div>
        ))}
      </div>
    </div>
  );
}
