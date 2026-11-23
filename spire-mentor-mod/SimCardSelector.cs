// SimCardSelector.cs -- ICardSelector that routes every card selection to the agent.
// Originally ported from zhiyue/sts2-rl-agent (STS2BridgeMod); rewritten clean-room.

using System;
using System.Collections.Generic;
using System.Linq;
using System.Text.Json;
using System.Threading;
using MegaCrit.Sts2.Core.Context;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.Entities.CardRewardAlternatives;
using MegaCrit.Sts2.Core.Entities.Cards;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Runs;
using MegaCrit.Sts2.Core.TestSupport;

namespace SpireMentor;

public class SimCardSelector : ICardSelector
{
    /// <summary>
    /// True while <see cref="GetSelectedCards"/> is blocked on the agent.
    /// Read by SimCombatHandler.PlayCardAndWaitAsync: a card whose play
    /// opened a selection legitimately parks in the PlayPile until the pick
    /// resolves, so the post-play "wait for the PlayPile to drain" settle
    /// must stop waiting once this prompt opens (it would otherwise burn its
    /// full timeout on every select-opening card).
    /// </summary>
    internal static volatile bool SelectionOpen;

    /// <summary>
    /// Every card selection the game makes while this selector is installed
    /// (deck upgrade/transform/remove, hand selects, choose-a-card, ...). A
    /// selection with exactly one legal outcome is resolved without asking;
    /// anything else goes to the agent as <c>card_select</c> until it answers
    /// validly.
    /// </summary>
    public async System.Threading.Tasks.Task<IEnumerable<CardModel>> GetSelectedCards(
        IEnumerable<CardModel> options, int minSelect, int maxSelect)
    {
        var (choiceContext, chooser, choiceOptions) = SimCardSelectContext.ConsumeChoice();
        List<CardModel> choices = options.ToList();
        if (choices.Count == 0)
        {
            return new List<CardModel>();
        }
        if (choices.Count <= minSelect)
        {
            // Must take them all: the only legal outcome.
            return choices;
        }

        // Open before the signal: it parks a card play in
        // GatheringPlayerChoice, which SimCombatHandler's settle reads as done
        // unless a pick is open; and it stays open until the action resumes.
        SelectionOpen = true;
        try
        {
            if (choiceContext == null || chooser == null)
            {
                return await AskAgentAsync(choices, minSelect, maxSelect);
            }
            // The game's own bracket, skipped when a selector is installed
            // (GH #1367): a pick in a turn's setup pauses it, so the turn's
            // AfterSideTurnStart hooks run before the pick, as when played by
            // hand and on the headless host. The board is read after.
            await SimGameApi.SignalPlayerChoiceBegun(choiceContext, chooser, choiceOptions);
            IEnumerable<CardModel> picked = await AskAgentAsync(choices, minSelect, maxSelect);
            await choiceContext.SignalPlayerChoiceEnded();
            return picked;
        }
        finally
        {
            SelectionOpen = false;
        }
    }

    /// <summary>
    /// Ask the agent to pick from <paramref name="choices"/> until it answers
    /// validly; a selection with one legal outcome never gets here.
    /// </summary>
    private static async System.Threading.Tasks.Task<IEnumerable<CardModel>> AskAgentAsync(
        List<CardModel> choices, int minSelect, int maxSelect)
    {
        List<Dictionary<string, object?>> cards = IndexedCards(choices);
        RunState? runState = RunManager.Instance.DebugOnlyGetState();
        Player? player = LocalContext.GetMe(runState);

        // Captured by MainFile's CardSelectCmd patches just before the game
        // called us; consuming clears it for the next selection.
        // CLEANROOM2-Q7: only the captured fields the spec lists are sent (the
        // four screen fields plus pending_enchantment, GH #574, read before
        // Consume clears it); source_kind / source_target_index /
        // source_upgraded stay advisor-lane.
        string? pendingEnchantment = SimCardSelectContext.PendingEnchantment;
        (string? source, string? promptKey, string? sourceModel, bool? capturedCancelable) =
            SimCardSelectContext.Consume();
        // Fallback inference only; wrong for mandatory FromChooseACardScreen callers.
        bool cancelable = capturedCancelable ?? minSelect <= 0;

        var message = new Dictionary<string, object>
        {
            [NonCombatBridgeProtocol.TypeField] = NonCombatBridgeProtocol.CardSelectState,
            ["cards"] = cards,
            ["min_select"] = minSelect,
            ["max_select"] = maxSelect,
            ["min_count"] = minSelect,
            ["max_count"] = maxSelect,
            ["cancelable"] = cancelable,
            ["prompt_key"] = promptKey!,
            ["source"] = source!,
            ["source_model"] = sourceModel!,
            // Which enchantment a TO_ENCHANT pick stamps; null otherwise.
            ["pending_enchantment"] = pendingEnchantment!,
            ["combat"] = SimCombatHandler.BuildEmbeddedCombatState(player)!,
        };
        // The game passes no token. Bind to the run's, so a selector stranded
        // by a run timeout does not keep asking during the next run.
        CancellationToken ct = SimAutoSlayer.CurrentRunToken;

        try
        {
            while (true)
            {
                // Fresh envelope (state_id) per ask, so a retry after an error
                // is a new decision message (PROTOCOL.md section 3a).
                SimStateExtractor.ApplyEnvelope(message, runState, player);
                long stateId = SimAgentChoice.ReadStateId(message);
                string json = JsonSerializer.Serialize(message);
                string reply = await SimAgentGate.RequestActionAsync(json, ct, NonCombatBridgeProtocol.CardSelectState);
                IEnumerable<CardModel>? picked = TryParseCardSelectResponse(
                    reply, choices, minSelect, maxSelect, cancelable, stateId, runState, player);
                if (picked != null)
                {
                    return picked;
                }
                Logger.Log("[SimCardSelector] card_select reply rejected; asking again");
            }
        }
        catch (OperationCanceledException)
        {
            // Let the blocked game command unwind with a legal answer.
            List<CardModel> fallback = cancelable
                ? new List<CardModel>()
                : choices.Take(Math.Max(1, minSelect)).ToList();
            Logger.Log($"[SimCardSelector] card_select cancelled; resolving with {fallback.Count} card(s)");
            return fallback;
        }
    }

    /// <summary>Each card serialized plus its position in the option list.</summary>
    private static List<Dictionary<string, object?>> IndexedCards(IReadOnlyList<CardModel> cards)
    {
        var result = new List<Dictionary<string, object?>>(cards.Count);
        for (int i = 0; i < cards.Count; i++)
        {
            Dictionary<string, object?> card = SimStateExtractor.SerializeCard(cards[i]);
            card["index"] = i;
            result.Add(card);
        }
        return result;
    }

    /// <summary>
    /// Post-combat card pick (<c>card_reward</c>). Synchronous by interface,
    /// so it blocks on the agent request. <c>default</c> means skip.
    /// </summary>
    public CardRewardSelection GetSelectedCardReward(
        IReadOnlyList<CardCreationResult> options, IReadOnlyList<CardRewardAlternative> alternatives)
    {
        if (options.Count == 0)
        {
            return default;
        }

        RunState? runState = RunManager.Instance.DebugOnlyGetState();
        Player? player = LocalContext.GetMe(runState);
        var message = new Dictionary<string, object>
        {
            [NonCombatBridgeProtocol.TypeField] = NonCombatBridgeProtocol.CardRewardState,
            ["cards"] = IndexedCards(options.Select(o => o.Card).ToList()),
            ["can_skip"] = true,
        };
        CancellationToken ct = SimAutoSlayer.CurrentRunToken;

        while (true)
        {
            // Fresh envelope (state_id) per ask (PROTOCOL.md section 3a).
            SimStateExtractor.ApplyEnvelope(message, runState, player);
            long stateId = SimAgentChoice.ReadStateId(message);
            string json = JsonSerializer.Serialize(message);
            string reply;
            try
            {
                // CLEANROOM2-Q8: the request runs on the thread pool so its
                // awaits never need the (blocked) calling thread's context.
                reply = System.Threading.Tasks.Task
                    .Run(() => SimAgentGate.RequestActionAsync(json, ct, NonCombatBridgeProtocol.CardRewardState))
                    .GetAwaiter().GetResult();
            }
            catch (OperationCanceledException)
            {
                Logger.Log("[SimCardSelector] card_reward cancelled; skipping the reward");
                return default;
            }

            if (!SimProtocolHelpers.TryParse(reply, runState, player, out JsonElement root) ||
                !SimProtocolHelpers.CheckStateId(root, stateId, runState, player))
            {
                continue;
            }

            try
            {
                string action = ReadAction(root);
                if (action == NonCombatBridgeProtocol.SkipAction)
                {
                    Logger.Log("[SimCardSelector] Agent skipped the card reward");
                    return default;
                }
                if (action == NonCombatBridgeProtocol.ChooseAction)
                {
                    if (!root.TryGetProperty("index", out JsonElement indexEl) ||
                        indexEl.ValueKind != JsonValueKind.Number ||
                        !indexEl.TryGetInt32(out int index))
                    {
                        SimProtocolHelpers.SendError("malformed_action",
                            "\"choose\" requires an integer \"index\"", runState, player);
                    }
                    else if (index >= options.Count)
                    {
                        Logger.Log($"[SimCardSelector] card_reward index {index} >= {options.Count}: treated as skip");
                        return default;
                    }
                    else if (index >= 0)
                    {
                        CardModel card = options[index].Card;
                        Logger.Log($"[SimCardSelector] Agent took card reward {card.Id.Entry}");
                        return new CardRewardSelection { card = card, alternative = null! };
                    }
                    else
                    {
                        SimProtocolHelpers.SendError("invalid_index",
                            $"index {index} out of range ({options.Count} cards)", runState, player);
                    }
                }
                else
                {
                    SimProtocolHelpers.SendError("unknown_action",
                        $"Unrecognized action \"{action}\" for card_reward", runState, player);
                }
            }
            catch (Exception ex)
            {
                Logger.Log($"[SimCardSelector] Error handling card_reward reply: {ex.Message}");
                SimProtocolHelpers.SendError("internal_error", ex.Message, runState, player);
            }
            Logger.Log("[SimCardSelector] card_reward reply rejected; asking again");
        }
    }

    private static string ReadAction(JsonElement root) =>
        root.ValueKind == JsonValueKind.Object &&
        root.TryGetProperty("action", out JsonElement actionEl) &&
        actionEl.ValueKind == JsonValueKind.String
            ? actionEl.GetString() ?? ""
            : "";

    /// <summary>
    /// Validates a <c>card_select</c> reply. Returns the selected cards (empty
    /// = skip/cancel), or null to ask again (the matching error was sent).
    /// </summary>
    private static IEnumerable<CardModel>? TryParseCardSelectResponse(
        string reply, List<CardModel> options, int minSelect, int maxSelect, bool cancelable,
        long expectedStateId, RunState? runState, Player? player)
    {
        if (!SimProtocolHelpers.TryParse(reply, runState, player, out JsonElement root) ||
            !SimProtocolHelpers.CheckStateId(root, expectedStateId, runState, player))
        {
            return null;
        }

        try
        {
            string action = ReadAction(root);

            if (action == NonCombatBridgeProtocol.SkipAction || action == NonCombatBridgeProtocol.CancelAction)
            {
                if (!cancelable)
                {
                    SendNotCancelable(minSelect, runState, player);
                    return null;
                }
                Logger.Log($"[SimCardSelector] Agent declined the selection ({action})");
                return new List<CardModel>();
            }

            if (action != NonCombatBridgeProtocol.ChooseAction)
            {
                SimProtocolHelpers.SendError("unknown_action",
                    $"Unrecognized action \"{action}\" for card_select", runState, player);
                return null;
            }

            // Single-select spelling.
            if (root.TryGetProperty("index", out JsonElement indexEl) && indexEl.ValueKind == JsonValueKind.Number)
            {
                bool isInt = indexEl.TryGetInt32(out int index);
                if (isInt && index >= options.Count)
                {
                    // An index past the end selects nothing: a skip, if the screen allows one.
                    switch (CardSelectRules.ForEmptyChoice(minSelect, cancelable))
                    {
                        case CardSelectRules.EmptyChoice.Skip:
                            return new List<CardModel>();
                        case CardSelectRules.EmptyChoice.NotCancelable:
                            SendNotCancelable(minSelect, runState, player);
                            return null;
                    }
                }
                if (isInt && index >= 0 && index < options.Count && minSelect <= 1)
                {
                    return new List<CardModel> { options[index] };
                }
                SimProtocolHelpers.SendError("invalid_index",
                    $"index {indexEl} not valid ({options.Count} cards, min_select {minSelect})", runState, player);
                return null;
            }

            // Multi-select spelling: "indices" preferred, "indexes" legacy.
            JsonElement list;
            bool hasList =
                (root.TryGetProperty("indices", out list) && list.ValueKind == JsonValueKind.Array) ||
                (root.TryGetProperty("indexes", out list) && list.ValueKind == JsonValueKind.Array);
            if (!hasList)
            {
                SimProtocolHelpers.SendError("malformed_action",
                    "\"choose\" requires \"index\" or an \"indices\" array", runState, player);
                return null;
            }

            var selected = new List<CardModel>();
            foreach (JsonElement el in list.EnumerateArray())
            {
                // CLEANROOM2-Q9: duplicates are not rejected (the spec lists none).
                if (el.ValueKind != JsonValueKind.Number || !el.TryGetInt32(out int i) || i < 0 || i >= options.Count)
                {
                    SimProtocolHelpers.SendError("invalid_index",
                        $"indices element {el} not valid ({options.Count} cards)", runState, player);
                    return null;
                }
                selected.Add(options[i]);
            }

            if (selected.Count == 0)
            {
                switch (CardSelectRules.ForEmptyChoice(minSelect, cancelable))
                {
                    case CardSelectRules.EmptyChoice.Skip:
                        return selected;
                    case CardSelectRules.EmptyChoice.NotCancelable:
                        SendNotCancelable(minSelect, runState, player);
                        return null;
                }
            }
            if (selected.Count >= minSelect && selected.Count <= maxSelect)
            {
                return selected;
            }
            SimProtocolHelpers.SendError("invalid_count",
                $"selected {selected.Count} card(s); need between {minSelect} and {maxSelect}", runState, player);
            return null;
        }
        catch (Exception ex)
        {
            Logger.Log($"[SimCardSelector] Error handling card_select reply: {ex.Message}");
            SimProtocolHelpers.SendError("internal_error", ex.Message, runState, player);
            return null;
        }
    }

    private static void SendNotCancelable(int minSelect, RunState? runState, Player? player) =>
        SimProtocolHelpers.SendError("not_cancelable",
            $"this selection is not cancelable; choose at least {Math.Max(1, minSelect)} card(s)", runState, player);
}
