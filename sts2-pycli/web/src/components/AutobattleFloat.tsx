import { useEffect, useLayoutEffect, useRef, useState, type ComponentProps,
  type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
import AutobattlePanel from "./AutobattlePanel";
import { clamp, usePanelFlag } from "../panelPrefs";
import { t } from "../i18n";

/**
 * The autobattle panel floating over the board (the side panel is hidden,
 * its default at <=1200px, so on every tablet).
 *
 * Expanded, the card is ~300x500 px over the board's top-left: on a map it
 * hides a whole column of nodes. Two things keep it out of the way:
 *
 * - **It folds itself.** A tap anywhere outside the card folds it to one row
 *   (grip, caret, the action button, a one-line verdict); the tap still lands
 *   on whatever it hit. New results (a verdict, the next stop's change-diff)
 *   unfold it again, so you read them once and the next tap on the board
 *   clears them away. Folding it with its caret pins it folded: news then
 *   updates the one-line verdict only, until the caret unfolds it.
 * - **It moves.** The grip (⠿) drags it, with a finger too. The spot is
 *   remembered as an anchor: the card is drawn there, shifted only as far as
 *   it takes to keep the whole card on the board (an unfolded card anchored
 *   near the bottom edge rises; folded again, it drops back). Double-tap the
 *   grip to send it back to its default spot, just under the HUD band.
 */

const POS_KEY = "autobattleFloatPos";
const PINNED_KEY = "autobattleFloatPinned";
/** Below the HUD band: clears its torn-paper bottom edge (a `::after`). */
const HUD_GAP = 14;
/** The default spot's left edge, and the fallback top with no HUD on screen. */
const DEFAULT = { x: 8, y: 64 };

type Pos = { x: number; y: number };

function readPos(): Pos | null {
  try {
    const raw = globalThis.localStorage?.getItem(POS_KEY);
    if (!raw) return null;
    const p = JSON.parse(raw) as Partial<Pos>;
    return Number.isFinite(p.x) && Number.isFinite(p.y) ? { x: p.x as number, y: p.y as number } : null;
  } catch {
    return null;
  }
}

function writePos(p: Pos | null): void {
  try {
    if (p) globalThis.localStorage?.setItem(POS_KEY, JSON.stringify(p));
    else globalThis.localStorage?.removeItem(POS_KEY);
  } catch {
    // Not persisted; it still moves this session.
  }
}

/** The anchor, moved only as far as keeps the whole card inside the board. */
function fitInside(p: Pos, card: HTMLElement, box: HTMLElement): Pos {
  const maxX = Math.max(0, box.clientWidth - card.offsetWidth);
  const maxY = Math.max(0, box.clientHeight - card.offsetHeight);
  return { x: Math.round(clamp(p.x, 0, maxX)), y: Math.round(clamp(p.y, 0, maxY)) };
}

type PanelProps = Omit<ComponentProps<typeof AutobattlePanel>, "grip" | "compact" | "onToggleCompact">;

export default function AutobattleFloat(props: PanelProps) {
  const ref = useRef<HTMLDivElement>(null);
  /** Where the user put it; null = the default spot under the HUD. */
  const [anchor, setAnchor] = useState<Pos | null>(readPos);
  /** Where it is drawn: the anchor (or default) fitted to the board. */
  const [shown, setShown] = useState<Pos | null>(null);
  const [pinned, setPinned] = usePanelFlag(PINNED_KEY);
  const [folded, setFolded] = useState(false);
  const compact = pinned || folded;
  const drag = useRef<{ dx: number; dy: number; moved: boolean } | null>(null);

  // Place it, and again whenever the card (folding, new content) or the board
  // (rotation, side panels) changes size.
  const { diff, verdict, over } = props;
  useLayoutEffect(() => {
    const el = ref.current;
    const box = el?.parentElement;
    if (!el || !box) return;
    const place = () => {
      let want = anchor;
      if (!want) {
        const hud = box.querySelector(".gs-hud");
        const top = hud ? hud.getBoundingClientRect().bottom - box.getBoundingClientRect().top + HUD_GAP : 0;
        want = { x: DEFAULT.x, y: top > 0 ? top : DEFAULT.y };
      }
      const next = fitInside(want, el, box);
      setShown((prev) => (prev && prev.x === next.x && prev.y === next.y ? prev : next));
    };
    place();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(place);
    ro.observe(el);
    ro.observe(box);
    return () => ro.disconnect();
  }, [anchor, compact, diff, verdict, over]);

  // News unfolds it (unless pinned folded).
  useEffect(() => {
    setFolded(false);
  }, [diff, verdict, over]);

  // A tap outside folds it. Capture phase, and nothing is stopped: the tap
  // still does whatever it does on the board.
  useEffect(() => {
    if (compact) return;
    function onDown(e: PointerEvent) {
      const el = ref.current;
      if (el && e.target instanceof Node && !el.contains(e.target)) setFolded(true);
    }
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, [compact]);

  function moveTo(p: Pos, persist: boolean) {
    const el = ref.current;
    const box = el?.parentElement;
    if (!el || !box) return;
    const next = fitInside(p, el, box);
    setAnchor(next);
    if (persist) writePos(next);
  }
  function onGripDown(e: ReactPointerEvent<HTMLSpanElement>) {
    if (e.button !== 0 || !ref.current) return;
    const r = ref.current.getBoundingClientRect();
    drag.current = { dx: e.clientX - r.left, dy: e.clientY - r.top, moved: false };
    e.currentTarget.setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  function onGripMove(e: ReactPointerEvent<HTMLSpanElement>) {
    const d = drag.current;
    const box = ref.current?.parentElement;
    if (!d || !box) return;
    const b = box.getBoundingClientRect();
    d.moved = true;
    moveTo({ x: e.clientX - b.left - d.dx, y: e.clientY - b.top - d.dy }, false);
  }
  function onGripUp() {
    const d = drag.current;
    drag.current = null;
    if (d?.moved) setAnchor((p) => { writePos(p); return p; });
  }
  function reset() {
    setAnchor(null);
    writePos(null);
  }
  function onGripKey(e: ReactKeyboardEvent) {
    const step = 16;
    const dx = e.key === "ArrowLeft" ? -step : e.key === "ArrowRight" ? step : 0;
    const dy = e.key === "ArrowUp" ? -step : e.key === "ArrowDown" ? step : 0;
    if (!dx && !dy) return;
    e.preventDefault();
    const cur = shown ?? anchor ?? DEFAULT;
    moveTo({ x: cur.x + dx, y: cur.y + dy }, true);
  }

  const grip = (
    <span
      className="autobattle-grip"
      role="button"
      tabIndex={0}
      aria-label={t("move the autobattle panel (double-tap to reset)")}
      title={t("drag to move · double-tap to reset")}
      onPointerDown={onGripDown}
      onPointerMove={onGripMove}
      onPointerUp={onGripUp}
      onPointerCancel={onGripUp}
      onDoubleClick={reset}
      onKeyDown={onGripKey}
    >
      ⠿
    </span>
  );

  return (
    <div
      ref={ref}
      className={`wb-autobattle-float${compact ? " wb-autobattle-float--compact" : ""}`}
      data-testid="autobattle-float"
      style={shown ? { left: shown.x, top: shown.y } : undefined}
    >
      <AutobattlePanel
        {...props}
        grip={grip}
        compact={compact}
        onToggleCompact={() => {
          // The caret is the explicit choice: folding pins it, unfolding unpins.
          setPinned(!compact);
          setFolded(false);
        }}
      />
    </div>
  );
}
