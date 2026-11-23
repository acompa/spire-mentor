using System;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.CompilerServices;
using HarmonyLib;
using MegaCrit.Sts2.Core.Combat;
using MegaCrit.Sts2.Core.Commands;
using MegaCrit.Sts2.Core.Entities.Cards;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.Hooks;
using MegaCrit.Sts2.Core.Models;

namespace Sts2Headless;

/// <summary>
/// The draw-pile cards a player knows the position of (GH #1710): a card put
/// on top or at the bottom (Headbutt, Havoc's Bottom, a generated card's
/// default) sits there until drawn or moved. A shuffle forgets every
/// position. A card shuffled in at a random spot does not (GH #1797): it most
/// likely lands below the known top cards, and a player keeps planning with
/// them there, as the engine's belief does (its pins survive a random insert).
/// The pile is then "blurred": the known cards are reported in the order the
/// player believes, wherever they physically are now.
///
/// The referee view sends the draw pile sorted; these two lists are the part
/// of its order a player with perfect memory has seen. Observing only: the
/// patches never change what the game does.
/// </summary>
public static class KnownDrawOrder
{
    private sealed class Known
    {
        /// <summary>Top first.</summary>
        public readonly List<CardModel> Top = new();
        /// <summary>Bottom first.</summary>
        public readonly List<CardModel> Bottom = new();
        /// <summary>
        /// A card went in at a random spot since the known cards were placed,
        /// so they may no longer sit exactly at the ends.
        /// </summary>
        public bool Blurred;

        public void Forget()
        {
            Top.Clear();
            Bottom.Clear();
            Blurred = false;
        }

        public void Drop(CardModel card)
        {
            Top.Remove(card);
            Bottom.Remove(card);
            if (Top.Count + Bottom.Count == 0) Blurred = false;
        }
    }

    private static readonly ConditionalWeakTable<CardPile, Known> Piles = new();
    // The position CardPileCmd.Add was asked for, per card, until its
    // AddInternal lands it.
    private static readonly ConditionalWeakTable<CardModel, StrongBox<CardPilePosition>> Asked = new();
    // Cards a shuffle is putting back: their re-add is not a known position.
    private static readonly ConditionalWeakTable<CardModel, object> Shuffled = new();
    private static readonly object Mark = new();
    private static bool _installed;

    public static void Install()
    {
        if (_installed) return;
        _installed = true;
        var harmony = new Harmony("sts2headless.known_draw_order");
        var self = typeof(KnownDrawOrder);
        HarmonyMethod Of(string name) => new(self.GetMethod(name, AccessTools.all));

        // The overload every other Add funnels into: (IEnumerable<CardModel>,
        // CardPile, CardPilePosition, ...). Its trailing parameters differ
        // between builds (v0.111.0 adds isChangingOwners).
        var add = typeof(CardPileCmd).GetMethods()
            .FirstOrDefault(m =>
            {
                if (m.Name != nameof(CardPileCmd.Add)) return false;
                var ps = m.GetParameters();
                return ps.Length >= 3 && ps[0].ParameterType == typeof(IEnumerable<CardModel>)
                    && ps[1].ParameterType == typeof(CardPile) && ps[2].ParameterType == typeof(CardPilePosition);
            }) ?? throw new MissingMethodException(nameof(CardPileCmd), "Add(IEnumerable, CardPile, CardPilePosition, ...)");
        harmony.Patch(add, prefix: Of(nameof(AddPrefix)));

        var pile = typeof(CardPile);
        harmony.Patch(pile.GetMethod(nameof(CardPile.AddInternal))
            ?? throw new MissingMethodException(nameof(CardPile), "AddInternal"), postfix: Of(nameof(AddInternalPostfix)));
        harmony.Patch(pile.GetMethod(nameof(CardPile.RemoveInternal))
            ?? throw new MissingMethodException(nameof(CardPile), "RemoveInternal"), postfix: Of(nameof(RemoveInternalPostfix)));
        harmony.Patch(pile.GetMethod(nameof(CardPile.MoveToTopInternal))
            ?? throw new MissingMethodException(nameof(CardPile), "MoveToTopInternal"), postfix: Of(nameof(MoveToTopPostfix)));
        harmony.Patch(pile.GetMethod(nameof(CardPile.MoveToBottomInternal))
            ?? throw new MissingMethodException(nameof(CardPile), "MoveToBottomInternal"), postfix: Of(nameof(MoveToBottomPostfix)));

        var shuffle = typeof(Hook).GetMethod(nameof(Hook.ModifyShuffleOrder))
            ?? throw new MissingMethodException(nameof(Hook), "ModifyShuffleOrder");
        harmony.Patch(shuffle, postfix: Of(nameof(ShufflePostfix)));
    }

    private static bool IsDraw(CardPile pile) => pile.Type == PileType.Draw;

    private static void AddPrefix(ref IEnumerable<CardModel> cards, CardPile newPile, CardPilePosition position)
    {
        if (!IsDraw(newPile)) return;
        var list = cards.ToList();
        cards = list;
        foreach (var card in list)
        {
            if (Shuffled.TryGetValue(card, out _)) continue;
            Asked.AddOrUpdate(card, new StrongBox<CardPilePosition>(position));
        }
    }

    private static void AddInternalPostfix(CardPile __instance, CardModel card)
    {
        if (!IsDraw(__instance)) return;
        var known = Piles.GetOrCreateValue(__instance);
        if (Shuffled.TryGetValue(card, out _))
        {
            Shuffled.Remove(card);
            Asked.Remove(card);
            return;
        }
        var position = Asked.TryGetValue(card, out var box) ? box.Value : CardPilePosition.None;
        Asked.Remove(card);
        switch (position)
        {
            case CardPilePosition.Top:
                known.Top.Insert(0, card);
                break;
            case CardPilePosition.Bottom:
                known.Bottom.Insert(0, card);
                break;
            default:
                // A random spot, or a path that did not say: the player
                // does not know where it went. The known cards keep their
                // believed places (GH #1797).
                if (known.Top.Count + known.Bottom.Count > 0) known.Blurred = true;
                break;
        }
    }

    private static void RemoveInternalPostfix(CardPile __instance, CardModel card)
    {
        if (IsDraw(__instance) && Piles.TryGetValue(__instance, out var known)) known.Drop(card);
    }

    private static void MoveToTopPostfix(CardPile __instance, CardModel card)
    {
        if (!IsDraw(__instance)) return;
        var known = Piles.GetOrCreateValue(__instance);
        known.Drop(card);
        known.Top.Insert(0, card);
    }

    private static void MoveToBottomPostfix(CardPile __instance, CardModel card)
    {
        if (!IsDraw(__instance)) return;
        var known = Piles.GetOrCreateValue(__instance);
        known.Drop(card);
        known.Bottom.Insert(0, card);
    }

    private static void ShufflePostfix(Player player, List<CardModel> cards, bool isInitialShuffle)
    {
        var pile = PileType.Draw.GetPile(player);
        if (pile != null && Piles.TryGetValue(pile, out var known)) known.Forget();
        // The regular shuffle puts every card back through Add; the initial
        // one reorders the pile in place.
        if (isInitialShuffle) return;
        foreach (var card in cards) Shuffled.AddOrUpdate(card, Mark);
    }

    /// <summary>
    /// The known cards' indices: top first, bottom first. Fail closed: if a
    /// remembered card is no longer where it was seen (a move these patches
    /// did not observe), forget everything and report nothing, so the output
    /// never depends on hidden order beyond what was recorded. On a blurred
    /// pile the cards need only still be in the pile, and are reported in the
    /// believed order at their physical indices (the referee view sends them
    /// at their believed places).
    /// </summary>
    public static (List<int> Top, List<int> Bottom) Positions(CardPile? pile)
    {
        var top = new List<int>();
        var bottom = new List<int>();
        if (pile == null || !Piles.TryGetValue(pile, out var known)) return (top, bottom);
        var cards = pile.Cards;
        if (known.Blurred)
        {
            var at = new Dictionary<CardModel, int>(ReferenceEqualityComparer.Instance);
            for (var i = 0; i < cards.Count; i++) at[cards[i]] = i;
            if (known.Top.Concat(known.Bottom).Any(c => !at.ContainsKey(c)))
            {
                known.Forget();
                return (top, bottom);
            }
            top.AddRange(known.Top.Select(c => at[c]));
            bottom.AddRange(known.Bottom.Select(c => at[c]));
            return (top, bottom);
        }
        var intact = known.Top.Count + known.Bottom.Count <= cards.Count;
        for (var i = 0; intact && i < known.Top.Count; i++)
            intact = ReferenceEquals(cards[i], known.Top[i]);
        for (var i = 0; intact && i < known.Bottom.Count; i++)
            intact = ReferenceEquals(cards[cards.Count - 1 - i], known.Bottom[i]);
        if (!intact)
        {
            known.Forget();
            return (top, bottom);
        }
        for (var i = 0; i < known.Top.Count; i++) top.Add(i);
        for (var i = 0; i < known.Bottom.Count; i++) bottom.Add(cards.Count - 1 - i);
        return (top, bottom);
    }
}
