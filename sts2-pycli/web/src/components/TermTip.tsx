import { useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { assetUrl } from "../api";
import RichText from "./RichText";

export interface TermTipContent {
  title: string;
  /** The game's text, `[gold]…[/gold]` markup kept. */
  description: string;
  /** An icon, relative to the art root. */
  image?: string | null;
}

/** Width of the popover, px. The game's hover tips are about this wide. */
const TIP_W = 260;

/**
 * A term the game would explain on hover (GH #1599): "Adroit", "Exhaust",
 * "Burning Blood". The words keep their colour and gain a dotted underline;
 * hovering (or focusing) them shows the term's own tip, as the game's
 * `NHoverTipSet` does over event and shop text.
 *
 * The popover is portalled to `<body>` and placed from the term's rect, so an
 * option button's `overflow: hidden` cannot clip it, and it flips above the
 * term when there is no room below.
 */
export default function TermTip({ tip, children, className, style }: {
  tip: TermTipContent; children: ReactNode; className?: string; style?: CSSProperties;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ left: number; top: number; above: boolean } | null>(null);
  useLayoutEffect(() => {
    if (!open || !ref.current) return;
    const r = ref.current.getBoundingClientRect();
    const left = Math.max(8, Math.min(window.innerWidth - TIP_W - 8, r.left));
    const above = r.bottom + 160 > window.innerHeight;
    setPos({ left, top: above ? r.top - 6 : r.bottom + 6, above });
  }, [open]);
  return (
    <span ref={ref} className={className ?? "wb-term"} style={style} tabIndex={-1}
      onMouseEnter={() => setOpen(true)} onMouseLeave={() => setOpen(false)}
      onFocus={() => setOpen(true)} onBlur={() => setOpen(false)}>
      {children}
      {open && pos && createPortal(
        <div role="tooltip" className="wb-term-tip"
          style={{ left: pos.left, top: pos.top, width: TIP_W,
            transform: pos.above ? "translateY(-100%)" : undefined }}>
          <div className="wb-term-tip-head">
            {tip.image && <img src={assetUrl(tip.image)} alt="" aria-hidden="true" />}
            <span>{tip.title}</span>
          </div>
          {tip.description && (
            <div className="wb-term-tip-body"><RichText text={tip.description} tips={false} /></div>
          )}
        </div>,
        document.body,
      )}
    </span>
  );
}
