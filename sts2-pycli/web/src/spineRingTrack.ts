// Aeonglass's rings live on a second Spine track. The game's NAeonGlassVfx
// (`_Ready`) loops `_track1/rings_normal` on track 1 next to the body's
// `idle_loop`; an attack swaps in `_track1/rings_attack_heavy` /
// `_track1/rings_attack_double` and queues `rings_normal` back, and death plays
// `_track1/rings_die`. Without that track the second ring rests at its setup
// pose, a detached ring beside the hourglass.

/** The track the game plays a rig's ring clips on. */
export const RING_TRACK = 1;
/** The ring clip that loops while the body idles. */
export const RINGS_IDLE = "_track1/rings_normal";

/** The ring clip the game pairs with body clip `clip` (`attack_heavy` →
 *  `_track1/rings_attack_heavy`), or the idle ring clip for the idle, or
 *  undefined when the rig has no such clip (every rig but Aeonglass). */
export function ringClipFor(clip: string, names: readonly string[], idle?: string): string | undefined {
  if (!names.includes(RINGS_IDLE)) return undefined;
  if (clip === idle) return RINGS_IDLE;
  const paired = `_track1/rings_${clip}`;
  return names.includes(paired) ? paired : undefined;
}
