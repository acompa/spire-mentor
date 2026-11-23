import { useViewUpgrades } from "../glossary";
import { t } from "../i18n";

/**
 * The game's "View Upgrades" tick box (`NCardsViewScreen`, the card grids):
 * on, every card that can upgrade is drawn upgraded, its changed numbers in
 * green, so you can read what a Smith or an upgrade would give you (GH
 * #1598). One switch for every grid, remembered across visits.
 */
export default function ViewUpgradesToggle({ className }: { className?: string }) {
  const [on, setOn] = useViewUpgrades();
  return (
    <label className={`wb-viewupgrades${className ? ` ${className}` : ""}`}
      onClick={(e) => e.stopPropagation()}>
      <input type="checkbox" checked={on} onChange={(e) => setOn(e.target.checked)} />
      <span>{t("View upgrades")}</span>
    </label>
  );
}
