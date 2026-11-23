// CardSelectRules.cs -- whether a `card_select` "choose" reply that selects
// NOTHING is a skip the screen allows.
//
// Game-free on purpose (no Godot, no sts2.dll) so tests/unit can link it.

namespace SpireMentor;

internal static class CardSelectRules
{
    /// <summary>What to do with a reply that selects no cards.</summary>
    internal enum EmptyChoice
    {
        /// <summary>Accept it: the selection is skipped.</summary>
        Skip,

        /// <summary>Refuse it with <c>not_cancelable</c>, like an explicit skip.</summary>
        NotCancelable,

        /// <summary>Not an empty choice this screen understands: the caller's
        /// usual <c>invalid_index</c> / <c>invalid_count</c> error.</summary>
        Invalid,
    }

    /// <summary>
    /// A "choose" that selects nothing (single select: an index at or past the
    /// end; multi select: an empty <c>indices</c>) is a skip only when the
    /// screen allows one at all (<paramref name="minSelect"/> &lt;= 0) AND the
    /// game says it is <paramref name="cancelable"/>.
    ///
    /// <para><c>CardSelectCmd.FromChooseACardScreen</c> calls the selector with
    /// <c>(cards, 0, 1)</c> even when its <c>canSkip</c> is false (Toolbox,
    /// Knowledge Demon), so <paramref name="minSelect"/> 0 alone does not make
    /// a choice optional. Accepting the empty choice there let an agent skip a
    /// choice the real UI forces (issue #1326).</para>
    /// </summary>
    public static EmptyChoice ForEmptyChoice(int minSelect, bool cancelable) =>
        minSelect > 0 ? EmptyChoice.Invalid
        : cancelable ? EmptyChoice.Skip
        : EmptyChoice.NotCancelable;

    /// <summary>
    /// Whether an in-hand selection (<c>NPlayerHand</c>) that could be
    /// confirmed with <paramref name="selected"/> cards taken still offers
    /// the <paramref name="remaining"/> cards in hand, instead of confirming.
    ///
    /// <para>"Any number" and "up to N" hand selects (Gambler's Brew,
    /// Ashwater, Purity) enable the confirm after the first card. Confirming
    /// there cut every such answer to one card: the advice named five cards
    /// for Gambler's Brew and the AI discarded one Defend (issue #1770). The
    /// advisor answers <c>skip</c> (confirm) itself once its set is taken
    /// (<c>selected_count</c> in the state), as on the combat pile grid
    /// (<see cref="CardGridRules.OffersMorePicks"/>).</para>
    ///
    /// <para>At <paramref name="maxSelect"/>, or with no card left, it is
    /// confirm-only: nothing else could be picked.</para>
    /// </summary>
    public static bool HandOffersMorePicks(int selected, int maxSelect, int remaining) =>
        selected < maxSelect && remaining > 0;
}
