/** Fitting the merchant's tent to the stage it is given.
 *
 *  The shop is authored in design px (165px cards, a 255px merchant), like the
 *  combat scene. On a landscape tablet (1024x640 CSS px, ~470px of stage once
 *  the workbench chrome is drawn) that composition is about twice the stage's
 *  height, and the page scrolled to reach the colorless cards, the wares and
 *  Leave. Like the game, which scales its whole canvas to the window, the shop
 *  now shrinks to the stage instead: `zoom` on the content column and on the
 *  merchant (the backdrop and the light pools are percentage-sized already).
 *  `zoom` rather than a transform, so nothing opens a stacking context: the
 *  light pools and the merchant blend against the backdrop through it.
 */
import { useLayoutEffect, type RefObject } from "react";
import type { Card } from "./api";

/** Below this a 165px card is under 100px and its text stops being readable
 *  (a phone in landscape); the stage scrolls instead. */
export const SHOP_MIN_ZOOM = 0.6;
/** Quantisation, so a resize drag does not relayout every card per pixel. */
const STEP = 0.02;

/** How many class cards the merchant stocks: `MerchantInventory` puts five of
 *  the character's cards on the rack and two colorless ones beside the wares. */
const RACK_SIZE = 5;

/** The two shelves of the game's shop screen: the class cards on the top rack,
 *  the colorless ones down on the floor beside the relics and potions.
 *
 *  A card's `color` (its pool's frame tint) says which shelf it is on. A
 *  backend that sends no colour gets the merchant's own order instead: the
 *  rack's five slots first, then the colorless ones. */
export function splitShelves(cards: Card[]): { rack: Card[]; bin: Card[] } {
  if (cards.some((c) => c.color != null)) {
    const rack = cards.filter((c) => c.color !== "colorless");
    const bin = cards.filter((c) => c.color === "colorless");
    // a shop of nothing but colorless cards keeps them up on the rack
    return rack.length > 0 ? { rack, bin } : { rack: bin, bin: [] };
  }
  return { rack: cards.slice(0, RACK_SIZE), bin: cards.slice(RACK_SIZE) };
}

/** The largest zoom, a multiple of STEP in [min, 1], at which `fits` holds.
 *  `fits` is not quite monotone (a card row wraps at some widths), so this
 *  walks from a guess rather than bisecting: up while the next step still fits,
 *  or down until one does. Returns `min` when nothing fits. */
export function searchZoom(fits: (z: number) => boolean, guess: number, min = SHOP_MIN_ZOOM): number {
  const q = (z: number) => Math.round(z / STEP) * STEP;
  let z = Math.min(1, Math.max(min, q(guess)));
  if (fits(z)) {
    while (z + STEP <= 1 + 1e-9 && fits(q(z + STEP))) z = q(z + STEP);
    return z;
  }
  while (z - STEP >= min - 1e-9) {
    z = q(z - STEP);
    if (fits(z)) return z;
  }
  return min;
}

/** Zoom the shop's content column (and the merchant with it) to fit the root's
 *  box. Only in landscape: a portrait phone keeps the design size and scrolls,
 *  because fitting a shop into a 412px-wide column would leave every card a
 *  thumbnail. Re-fits whenever the box changes size. */
export function useShopFit(
  rootRef: RefObject<HTMLElement | null>,
  contentRef: RefObject<HTMLElement | null>,
  merchantRef: RefObject<HTMLElement | null>,
): void {
  useLayoutEffect(() => {
    const root = rootRef.current;
    const content = contentRef.current;
    if (!root || !content) return;
    const apply = (z: number) => {
      const v = z === 1 ? "" : String(z);
      content.style.zoom = v;
      if (merchantRef.current) merchantRef.current.style.zoom = v;
    };
    const fit = () => {
      // Out of flow while measuring: the root then falls back to its own
      // min-height (the stage), and the column reports its natural height.
      const { position, minHeight, width } = content.style;
      content.style.position = "absolute";
      content.style.minHeight = "0";
      content.style.width = "100%";
      const W = root.clientWidth;
      const H = root.clientHeight;
      let z = 1;
      if (W > 0 && H > 0 && W >= H) {
        const fits = (zz: number) => {
          apply(zz);
          return content.getBoundingClientRect().height <= H + 0.5
            && content.scrollWidth <= content.clientWidth + 1;
        };
        if (!fits(1)) z = searchZoom(fits, H / content.getBoundingClientRect().height);
      }
      apply(z);
      content.style.position = position;
      content.style.minHeight = minHeight;
      content.style.width = width;
    };
    fit();
    // The stage (the root's container) changing size is what re-fits; the
    // content's own size follows the zoom, and the fit restores exactly the
    // sizes it found, so observing the content too settles after one pass
    // (a new chip, a status line) instead of looping.
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(() => fit());
    if (root.parentElement) ro.observe(root.parentElement);
    ro.observe(content);
    return () => ro.disconnect();
  }, [rootRef, contentRef, merchantRef]);
}
