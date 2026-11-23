import { Fragment, useEffect, useState, type ReactNode } from "react";
import { DEFAULT_BRIDGE_URL, normalizeBridgeUrl, servedByControlPlane } from "../bridge";
import { t } from "../i18n";

/** A translated sentence with `{name}` slots filled by elements, so the
 *  sentence is never split across t() calls. */
function interp(template: string, parts: Record<string, ReactNode>): ReactNode {
  return template.split(/(\{\w+\})/).map((seg, i) => {
    const m = /^\{(\w+)\}$/.exec(seg);
    return <Fragment key={i}>{m && m[1] in parts ? parts[m[1]] : seg}</Fragment>;
  });
}

/**
 * "Live game…" (browser build, GH #709): connect this page to the advisor's
 * control plane on the machine running the game, so the Game tree, Live
 * search and rewind work with no server in between. The dialog is where the
 * page-side opt-in lives (the URL) and where the other two are explained:
 * the advisor must allow this page's origin, and Chrome asks for its local
 * network permission on the first request.
 */
export default function BridgeConnectDialog({
  open,
  url,
  state,
  error,
  onChange,
  onClose,
}: {
  open: boolean;
  /** The configured control-plane origin, or null. */
  url: string | null;
  state: "off" | "connected" | "unreachable";
  error: string | null;
  onChange: (url: string | null) => void;
  onClose: () => void;
}) {
  const [text, setText] = useState(url ?? DEFAULT_BRIDGE_URL);
  useEffect(() => {
    if (open) setText(url ?? DEFAULT_BRIDGE_URL);
  }, [open, url]);
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);
  if (!open) return null;

  const normalized = normalizeBridgeUrl(text);
  const origin = typeof window !== "undefined" ? window.location.origin : "";
  const status =
    state === "connected" ? t("connected — the advisor answers, a run is live")
    : state === "unreachable" ? t("configured, but the advisor does not answer (see below)")
    : t("not connected");

  return (
    <div role="presentation" onClick={onClose}
      style={{ position: "fixed", inset: 0, background: "rgba(0,0,0,0.55)", zIndex: 60,
        display: "flex", alignItems: "center", justifyContent: "center" }}>
      <div role="dialog" aria-label={t("Live game")} aria-modal="true" onClick={(e) => e.stopPropagation()}
        style={{ background: "var(--panel, #1c1c22)", color: "var(--fg, #eee)", border: "1px solid var(--border, #444)",
          borderRadius: 8, padding: 18, width: "min(560px, 92vw)", display: "flex", flexDirection: "column", gap: 12 }}>
        <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}>
          <h2 style={{ margin: 0, fontSize: 16 }}>🎮 {t("Live game")}</h2>
          <span style={{ color: "var(--muted)", fontSize: 12 }}>{status}</span>
        </div>
        <p style={{ margin: 0, fontSize: 13, lineHeight: 1.45 }}>
          {interp(t("Connect this page to the advisor running next to the game to get the {tree} (rewind the real game to a save point), {search} over the live board, and {rewind}. Three things have to be true, none of them by default:"),
          { tree: <b>{t("Game tree")}</b>, search: <b>{t("Live search")}</b>, rewind: <b>{t("rewind to the viewed floor")}</b> })}
        </p>
        {servedByControlPlane() ? (
          <p style={{ margin: 0, fontSize: 13, lineHeight: 1.45 }}>
            {interp(t("This page is served by the advisor itself (the workbench bundled with the Spire Mentor mod), so it is connected to {origin} already — nothing to allow, no browser prompt. If it stops answering, the advisor has stopped (the game closed, or the AI coach was turned off)."),
            { origin: <code className="mono">{origin}</code> })}
          </p>
        ) : (
        <ol style={{ margin: 0, paddingLeft: 20, fontSize: 13, lineHeight: 1.45 }}>
          <li>
            {interp(t("The advisor allows this page. In the mod's settings set {setting} to {origin} (or run the advisor with {flag})."),
              { setting: <b>Browser workbench origin</b>, origin: <code className="mono">{origin}</code>,
                flag: <code className="mono">--control-allow-origin {origin}</code> })}
          </li>
          <li>{t("The control plane's address below points at that machine (the default is right when the game runs on this one).")}</li>
          <li>{t("Chrome asks once whether this site may reach your local network — allow it. Firefox does not ask.")}</li>
        </ol>
        )}
        <label style={{ display: "flex", flexDirection: "column", gap: 4, fontSize: 13 }}>
          {t("control plane URL")}
          <input className="mono" value={text} onChange={(e) => setText(e.target.value)}
            placeholder={DEFAULT_BRIDGE_URL} aria-label={t("control plane URL")}
            style={{ padding: "6px 8px", borderRadius: 4, border: "1px solid var(--border, #444)",
              background: "var(--bg, #111)", color: "inherit" }} />
        </label>
        {text.trim() && !normalized && (
          <div style={{ color: "var(--bad, #d66)", fontSize: 13 }}>{t("that is not an http(s) URL")}</div>
        )}
        {error && <div style={{ color: "var(--warn, #c90)", fontSize: 13 }}>{error}</div>}
        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
          {url && (
            <button className="mono" onClick={() => onChange(null)}
              style={{ padding: "6px 14px", borderRadius: 4, border: "1px solid var(--border)", cursor: "pointer" }}>
              {t("forget")}
            </button>
          )}
          <button className="mono" onClick={() => normalized && onChange(normalized)} disabled={!normalized}
            style={{ padding: "6px 14px", borderRadius: 4, border: "1px solid var(--border)",
              cursor: normalized ? "pointer" : "default", opacity: normalized ? 1 : 0.5 }}>
            {t("connect")}
          </button>
          <button className="mono" onClick={onClose}
            style={{ padding: "6px 14px", borderRadius: 4, border: "1px solid var(--border)", cursor: "pointer" }}>
            {t("close")}
          </button>
        </div>
      </div>
    </div>
  );
}
