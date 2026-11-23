// SimCrystalSphereScreenHandler.cs -- Crystal Sphere minigame, decided by the agent.
// Originally ported from zhiyue/sts2-rl-agent (STS2BridgeMod); rewritten clean-room.

using System;
using System.Collections.Generic;
using System.Linq;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Godot;
using HarmonyLib;
using MegaCrit.Sts2.Core.AutoSlay;
using MegaCrit.Sts2.Core.AutoSlay.Handlers;
using MegaCrit.Sts2.Core.AutoSlay.Helpers;
using MegaCrit.Sts2.Core.Context;
using MegaCrit.Sts2.Core.Events.Custom.CrystalSphereEvent;
using MegaCrit.Sts2.Core.Nodes.CommonUi;
using MegaCrit.Sts2.Core.Nodes.Events.Custom.CrystalSphere;
using MegaCrit.Sts2.Core.Nodes.GodotExtensions;
using MegaCrit.Sts2.Core.Nodes.Screens.Map;
using MegaCrit.Sts2.Core.Nodes.Screens.Overlays;
using MegaCrit.Sts2.Core.Random;
using MegaCrit.Sts2.Core.Runs;

namespace SpireMentor;

public class SimCrystalSphereScreenHandler : IScreenHandler, IHandler
{
    private const string ScreenName = nameof(NCrystalSphereScreen);
    private const string ProceedButtonPath = "%ProceedButton";
    private const string CellsPath = "%Cells";
    private const int OpeningSettleMs = 1000;
    private const int CellClickSettleMs = 500;
    private static readonly TimeSpan OutcomeWait = TimeSpan.FromSeconds(15);
    private static readonly TimeSpan ProceedCloseWait = TimeSpan.FromSeconds(10);
    private const int OverlayRemovalSettleMs = 100;

    public Type ScreenType => typeof(NCrystalSphereScreen);

    public TimeSpan Timeout => TimeSpan.FromHours(24);

    public async Task HandleAsync(Rng random, CancellationToken ct)
    {
        AutoSlayLog.EnterScreen(ScreenName);
        NCrystalSphereScreen screen = AutoSlayer.GetCurrentScreen<NCrystalSphereScreen>();
        await Task.Delay(OpeningSettleMs, ct);

        while (IsShowing(screen))
        {
            ct.ThrowIfCancellationRequested();

            if (AnotherScreenOnTop(screen))
            {
                // E.g. a rewards screen the sphere opened; the drain comes back here later.
                AutoSlayLog.Info("[SimCrystalSphere] Another screen is on top; returning to the drain");
                AutoSlayLog.ExitScreen(ScreenName);
                return;
            }

            NProceedButton? proceed = screen.GetNodeOrNull<NProceedButton>(ProceedButtonPath);
            List<NCrystalSphereCell> cells = HiddenCells(screen);
            bool proceedEnabled = proceed != null && proceed.IsEnabled;

            if (cells.Count == 0 && !proceedEnabled)
            {
                // A reveal is animating: nothing to decide, nothing to send.
                await WaitForOutcomeAsync(screen, ct);
                continue;
            }

            CrystalSphereChoice choice = await ChooseCrystalSphereOption(screen, cells, proceedEnabled, ct);
            switch (choice.Kind)
            {
                case CrystalSphereChoiceKind.Proceed:
                    // Re-read: proceed was validated as enabled when the reply arrived.
                    proceed = screen.GetNodeOrNull<NProceedButton>(ProceedButtonPath);
                    if (proceed != null)
                        await ProceedAndCloseAsync(screen, proceed, ct);
                    AutoSlayLog.ExitScreen(ScreenName);
                    return;

                case CrystalSphereChoiceKind.Wait:
                    await WaitForOutcomeAsync(screen, ct);
                    break;

                case CrystalSphereChoiceKind.Cell:
                    NCrystalSphereCell cell = choice.Cell!;
                    AutoSlayLog.Info($"[SimCrystalSphere] Divining cell ({cell.Entity.X}, {cell.Entity.Y})");
                    // The agent's block order assumes the Big tool (PROTOCOL.md).
                    SimScreenAdvice.SelectBigDivination(screen);
                    cell.EmitSignal(NClickableControl.SignalName.Released, cell);
                    await Task.Delay(CellClickSettleMs, ct);
                    break;
            }
        }

        AutoSlayLog.ExitScreen(ScreenName);
    }

    private static bool IsShowing(NCrystalSphereScreen screen) =>
        GodotObject.IsInstanceValid(screen) && screen.IsVisibleInTree();

    private static bool AnotherScreenOnTop(NCrystalSphereScreen screen)
    {
        NOverlayStack? stack = NOverlayStack.Instance;
        return stack != null && stack.ScreenCount > 0 && !ReferenceEquals(stack.Peek(), screen);
    }

    /// <summary>Visible cells still hiding their content (empty if %Cells is missing).</summary>
    private static List<NCrystalSphereCell> HiddenCells(NCrystalSphereScreen screen)
    {
        Control? container = screen.GetNodeOrNull<Control>(CellsPath);
        if (container == null)
            return new List<NCrystalSphereCell>();
        return UiHelper.FindAll<NCrystalSphereCell>(container)
            .Where(c => c.Visible && c.Entity.IsHidden)
            .ToList();
    }

    /// <summary>Waits for a reveal to settle: screen gone, proceed enabled, or a screen pushed on top.</summary>
    private static Task WaitForOutcomeAsync(NCrystalSphereScreen screen, CancellationToken ct) =>
        WaitHelper.Until(() =>
            !IsShowing(screen) ||
            (screen.GetNodeOrNull<NProceedButton>(ProceedButtonPath)?.IsEnabled ?? false) ||
            AnotherScreenOnTop(screen),
            ct, OutcomeWait, "Crystal Sphere showed no outcome");

    private static async Task ProceedAndCloseAsync(NCrystalSphereScreen screen, NProceedButton proceed, CancellationToken ct)
    {
        AutoSlayLog.Action("Crystal Sphere: clicking proceed");
        await UiHelper.Click(proceed);
        await WaitHelper.Until(() => !IsShowing(screen) || MapVisible(), ct, ProceedCloseWait,
            "Crystal Sphere did not close after proceed");

        if (IsShowing(screen) && MapVisible())
        {
            // The map is already up but the sphere never popped itself.
            AutoSlayLog.Info("[SimCrystalSphere] Map is visible but the sphere is still up; removing it");
            NOverlayStack.Instance?.Remove(screen);
            await Task.Delay(OverlayRemovalSettleMs, ct);
        }
    }

    private static bool MapVisible()
    {
        NMapScreen? map = NMapScreen.Instance;
        return map != null && GodotObject.IsInstanceValid(map) && map.IsVisibleInTree();
    }

    /// <summary>
    /// One option per hidden cell plus a trailing "proceed" when enabled.
    /// Nothing offered means "wait" without asking. Proceed is re-checked for
    /// every ask and again when the reply arrives.
    /// </summary>
    private static async Task<CrystalSphereChoice> ChooseCrystalSphereOption(
        NCrystalSphereScreen screen, List<NCrystalSphereCell> cells, bool proceedEnabled, CancellationToken ct)
    {
        if (cells.Count == 0 && !proceedEnabled)
        {
            return CrystalSphereChoice.Wait;
        }

        bool proceedOffered = false;

        int index = await SimAgentChoice.AskAsync(
            (_, _) =>
            {
                proceedOffered = ProceedEnabled(screen);
                return BuildState(screen, cells, proceedOffered);
            },
            cells.Count + 1, NonCombatBridgeProtocol.CrystalSphereState, "SimCrystalSphere", ct,
            rejectAtReply: i => i == cells.Count && !(proceedOffered && ProceedEnabled(screen))
                ? $"index {i} (proceed) is not enabled"
                : null);

        return index < cells.Count ? CrystalSphereChoice.Click(cells[index]) : CrystalSphereChoice.Proceed;
    }

    /// <summary>
    /// The `crystal_sphere` message body, for both lanes (this handler and
    /// F7's ask, SimScreenAdvice.DescribeCrystalSphere): one `divine_cell`
    /// option per cell of <paramref name="hiddenCells"/>, in that order, plus
    /// a trailing `proceed` when <paramref name="proceedOffered"/>; then
    /// `divinations_left` / `divinations_total` / `revealed` for the digger.
    /// Those three are omitted together when the minigame cannot be reached,
    /// and the agent falls back to its block order. No envelope.
    /// </summary>
    internal static Dictionary<string, object> BuildState(
        NCrystalSphereScreen screen, List<NCrystalSphereCell> hiddenCells, bool proceedOffered)
    {
        List<Dictionary<string, object>> options = hiddenCells.Select((c, i) => CellOption(c, i)).ToList();
        if (proceedOffered)
        {
            options.Add(new Dictionary<string, object>
            {
                ["index"] = hiddenCells.Count,
                ["action"] = NonCombatBridgeProtocol.ProceedAction,
                ["enabled"] = true,
            });
        }
        var state = new Dictionary<string, object>
        {
            [NonCombatBridgeProtocol.TypeField] = NonCombatBridgeProtocol.CrystalSphereState,
            ["options"] = options,
        };

        (int Left, int Total)? divinations = Divinations(screen);
        Control? container = screen.GetNodeOrNull<Control>(CellsPath);
        if (divinations is { } d && container != null)
        {
            state["divinations_left"] = d.Left;
            state["divinations_total"] = d.Total;
            // What the player sees: a hidden cell's Item is never read out
            // (CrystalSphereWire.Revealed drops hidden cells).
            state["revealed"] = CrystalSphereWire.Revealed(
                UiHelper.FindAll<NCrystalSphereCell>(container)
                    .Where(GodotObject.IsInstanceValid)
                    .Select(c => (c.Entity.X, c.Entity.Y, c.Entity.IsHidden,
                        c.Entity.IsHidden ? null : c.Entity.Item?.GetType().Name)));
        }
        return state;
    }

    /// <summary>The screen's minigame (private `_entity`, NCrystalSphereScreen).</summary>
    private static readonly FieldInfo? EntityField =
        AccessTools.Field(typeof(NCrystalSphereScreen), "_entity");

    /// <summary>The largest DivinationCount seen per screen: the total.</summary>
    private static readonly ConditionalWeakTable<NCrystalSphereScreen, StrongBox<int>> TotalSeen = new();

    /// <summary>
    /// The minigame's DivinationCount and the largest count seen on this
    /// screen instance (its total, when the first look was before the first
    /// reveal), or null when the minigame cannot be reached. Each call is
    /// also a look: DescribeCrystalSphere calls it on every poll so the total
    /// is caught before F7 or the player divines.
    /// </summary>
    internal static (int Left, int Total)? Divinations(NCrystalSphereScreen screen)
    {
        try
        {
            if (!GodotObject.IsInstanceValid(screen))
            {
                return null;
            }
            if (EntityField?.GetValue(screen) is not CrystalSphereMinigame minigame)
            {
                LogUnreadableOnce(EntityField == null
                    ? "NCrystalSphereScreen._entity was not found"
                    : "NCrystalSphereScreen._entity is not a CrystalSphereMinigame");
                return null;
            }
            int left = minigame.DivinationCount;
            StrongBox<int> seen = TotalSeen.GetValue(screen, _ => new StrongBox<int>(left));
            seen.Value = Math.Max(seen.Value, left);
            return (left, seen.Value);
        }
        catch (Exception ex)
        {
            LogUnreadableOnce(ex.Message);
            return null;
        }
    }

    private static bool _unreadableLogged;

    /// <summary>
    /// Says, once per session, that the minigame cannot be read: the board
    /// then goes out without divinations_left / divinations_total /
    /// revealed, so the advisor plays the #872 rule, not the digger.
    /// Divinations runs on every poll; logging each failure would flood.
    /// </summary>
    private static void LogUnreadableOnce(string why)
    {
        if (_unreadableLogged)
        {
            return;
        }
        _unreadableLogged = true;
        Logger.Log($"[SimCrystalSphere] Could not read the minigame ({why}); the crystal_sphere message " +
                   "goes out without the digger's fields, so the advisor plays the #872 rule (logged once).");
    }

    private static bool ProceedEnabled(NCrystalSphereScreen screen)
    {
        if (!GodotObject.IsInstanceValid(screen))
            return false;
        NProceedButton? proceed = screen.GetNodeOrNull<NProceedButton>(ProceedButtonPath);
        return proceed != null && proceed.IsEnabled;
    }

    private static Dictionary<string, object> CellOption(NCrystalSphereCell cell, int index) => new()
    {
        ["index"] = index,
        ["action"] = NonCombatBridgeProtocol.DivineCellAction,
        ["x"] = cell.Entity.X,
        ["y"] = cell.Entity.Y,
        ["enabled"] = true,
    };

    private enum CrystalSphereChoiceKind
    {
        Proceed,
        Cell,
        Wait,
    }

    private readonly record struct CrystalSphereChoice(CrystalSphereChoiceKind Kind, NCrystalSphereCell? Cell)
    {
        public static CrystalSphereChoice Proceed => new(CrystalSphereChoiceKind.Proceed, null);

        public static CrystalSphereChoice Wait => new(CrystalSphereChoiceKind.Wait, null);

        public static CrystalSphereChoice Click(NCrystalSphereCell cell) => new(CrystalSphereChoiceKind.Cell, cell);
    }
}
