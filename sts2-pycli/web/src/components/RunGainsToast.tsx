import type { Gain } from "../runGains";
import CardFrame from "./CardFrame";
import { t } from "../i18n";

const KIND_LABEL: Record<Gain["kind"], string> = {
  relic: "Relic",
  potion: "Potion",
  card: "Card",
  gold: "",
  hp: "",
  max_hp: "",
};

/** What the sign means, said in words on a card's caption. */
const CARD_VERB: Record<Gain["sign"], string> = {
  "+": "Added",
  "-": "Removed",
  "^": "Upgraded",
  "*": "Enchanted",
};

/** How wide the banner draws a card, px. */
const CARD_W = 116;

function signGlyph(g: Gain): string {
  if (g.kind === "gold" || g.kind === "hp" || g.kind === "max_hp") return "";
  return g.sign === "+" ? "+" : g.sign === "-" ? "−" : g.sign === "*" ? "✦" : "↑";
}

/**
 * "What just happened": the relics, cards, potions, gold and HP a run-level
 * step changed (`runGains`), as a banner over the top of the board.
 *
 * The game animates each of these as it lands; the workbench just moved to
 * the next screen, so an event's payout, a pick the AI made or the two cards
 * a Blitz "Random Upgrades" chose were invisible unless you opened the log.
 * Cards are drawn as cards (upgraded or enchanted, as they now are), so you
 * can read what you got (GH #1596). It reports, it does not gate: it sits
 * clear of the controls and stays until your next move replaces it; a tap
 * dismisses it early.
 */
export default function RunGainsToast({ gains, nonce, onDismiss }: {
  gains: Gain[];
  nonce: number;
  onDismiss: () => void;
}) {
  if (gains.length === 0) return null;
  const cards = gains.filter((g) => g.kind === "card" && g.card);
  const rest = gains.filter((g) => !(g.kind === "card" && g.card));
  return (
    <div className="wb-gains" role="status" aria-live="polite" key={nonce}
      onClick={onDismiss} title={t("what that step changed (tap to dismiss)")}>
      {cards.length > 0 && (
        <div className="wb-gains-cards">
          {cards.map((g, i) => (
            <figure key={i} className={`wb-gain-card wb-gain-card--${g.sign === "-" ? "loss" : "gain"}`}>
              <CardFrame card={g.card!} width={CARD_W} dim={g.sign === "-"} />
              {/* Runtime keys: the CARD_VERB / KIND_LABEL constants (listed in i18n/zh/screens.ts). */}
              <figcaption title={t(CARD_VERB[g.sign])}>
                <b aria-hidden="true">{signGlyph(g)}</b> <span className="wb-gain-verb">{t(CARD_VERB[g.sign])}</span>{" "}
                <span className="wb-gain-text">{g.text}</span>
              </figcaption>
            </figure>
          ))}
        </div>
      )}
      {rest.length > 0 && (
        <div className="wb-gains-row">
          {rest.map((g, i) => (
            <span key={i} className={`wb-gain wb-gain--${g.sign === "-" ? "loss" : "gain"} wb-gain--${g.kind}`}>
              {g.image && <img src={g.image} alt="" aria-hidden="true" />}
              {signGlyph(g) && <b aria-hidden="true">{signGlyph(g)}</b>}
              {KIND_LABEL[g.kind] && <span className="wb-gain-kind">{t(KIND_LABEL[g.kind])}</span>}
              <span className="wb-gain-text">{g.text}</span>
            </span>
          ))}
        </div>
      )}
      <span className="wb-gains-close" aria-hidden="true">×</span>
    </div>
  );
}
