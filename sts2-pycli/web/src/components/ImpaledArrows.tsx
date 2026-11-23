import { assetUrl } from "../api";
import { characterArt } from "../characters";

/** `impaled_arrow_vfx.tscn`: `ArrowBg`/`ArrowFg` are 501×182 sprites (centred)
 *  at (44.5, −5) and scale 0.75, each under a `BackArrow`/`FrontArrow` node at
 *  (−2, 5). */
const ART_W = 501;
const ART_H = 182;
const VISUAL_X = 44.5;
const VISUAL_Y = -5;
const VISUAL_SCALE = 0.75;
const NODE_X = -2;
const NODE_Y = 5;
/** `NImpaledArrowVfx._arrowRotationOffsets` / `_arrowPositionOffsets`, indexed
 *  by how many arrows the creature already holds (the vfx is made before
 *  `ImpaledPower.AddArrow`): later arrows fan out around the first. */
const SLOT_ROT = [0, -20, 20];
const SLOT_OFF: [number, number][] = [[0, 0], [15, 5], [-15, 5]];
/** The game draws `Rng.Chaotic.NextFloat(-15, 15)` per arrow without slots and
 *  `NextFloat(0.9, 1.1)` per slotted arrow; a fixed table keeps the board
 *  deterministic (a re-render must not reshuffle the arrows). */
const FREE_ROT = [-6, 11, -13, 4, 14, -9];
const SLOT_JITTER = [1, 0.95, 1.06, 0.92, 1.03, 0.97];

export type ArrowLayer = "back" | "front";

const art = (layer: ArrowLayer) =>
  assetUrl(characterArt("fool", `ui/fool/${layer === "back" ? "arrow_bg" : "arrow_fg"}.png`));

/** One centred arrow sprite: a zero-size box at the node origin, so a CSS
 *  transform on it is the Godot node's own rotation/scale about that origin. */
function Sprite({ src, x, y, rot, scale }: { src: string; x: number; y: number; rot: number; scale: number }) {
  return (
    <div style={{ position: "absolute", left: x, top: y, width: 0, height: 0,
      transform: `rotate(${rot}deg) scale(${scale})`, transformOrigin: "0 0" }}>
      <img alt="" draggable={false} src={src}
        style={{ position: "absolute", left: -ART_W / 2, top: -ART_H / 2, width: ART_W, height: ART_H, maxWidth: "none" }} />
    </div>
  );
}

/** The arrows in a rig's `ARROW_HIT_FRONT` / `ARROW_HIT_BACK` slot
 *  (`ImpaleArrowIntoSpineSlots`), in the slot's own frame: `SpineSprite` puts
 *  this under a box that follows the slot's bone. Each visual is reparented
 *  into the slot WITHOUT its `FrontArrow`/`BackArrow` node, and scaled by
 *  `1 / Body.Scale.X` so the bone's transform (which carries the rig's scale)
 *  draws it at its authored size. */
export function SlotArrows({ count, layer, bodyScale }: { count: number; layer: ArrowLayer; bodyScale: number }) {
  const src = art(layer);
  return (
    <>
      {Array.from({ length: count }, (_, i) => {
        const [ox, oy] = SLOT_OFF[i % SLOT_OFF.length];
        return (
          <Sprite key={i} src={src} x={VISUAL_X + ox} y={VISUAL_Y + oy} rot={SLOT_ROT[i % SLOT_ROT.length]}
            scale={(VISUAL_SCALE / bodyScale) * SLOT_JITTER[i % SLOT_JITTER.length]} />
        );
      })}
    </>
  );
}

/** The Fool's arrows stuck in a creature whose rig has no arrow slots
 *  (`ImpaleArrowIntoCreature`): at the scene's `ArrowPosition` marker, else
 *  `%CenterPos` (`anchor`, scene px from the creature's origin on the ground
 *  line), each arrow turned by its own small angle. The head (`arrow_bg`) goes
 *  behind the body and the shaft (`arrow_fg`) in front, so the arrow reads as
 *  passing THROUGH it: mount the `back` layer before the body and the `front`
 *  layer after it. */
export default function ImpaledArrows({ count, layer, pxPerScene, anchor }: {
  count: number; layer: ArrowLayer; pxPerScene: number; anchor: [number, number];
}) {
  const src = art(layer);
  return (
    <div className={`gs-impaled gs-impaled-${layer}`} aria-hidden="true"
      data-testid={layer === "front" ? "impaled-arrows" : undefined} data-count={count}
      style={{ transform: `scale(${pxPerScene}) translate(${anchor[0]}px, ${anchor[1]}px)` }}>
      {Array.from({ length: count }, (_, i) => (
        <div key={i} style={{ position: "absolute", left: 0, top: 0, width: 0, height: 0,
          transform: `rotate(${FREE_ROT[i % FREE_ROT.length]}deg)`, transformOrigin: "0 0" }}>
          <Sprite src={src} x={NODE_X + VISUAL_X} y={NODE_Y + VISUAL_Y} rot={0} scale={VISUAL_SCALE} />
        </div>
      ))}
    </div>
  );
}
