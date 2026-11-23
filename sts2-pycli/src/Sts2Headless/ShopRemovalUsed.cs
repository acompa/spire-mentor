using System;
using HarmonyLib;
using MegaCrit.Sts2.Core.Entities.Merchant;
using MegaCrit.Sts2.Core.Hooks;

namespace Sts2Headless;

/// <summary>
/// A bought shop card removal is spent (Spire Bench seed 170000025). Up to
/// v0.111.0 the game marks it used only from its shop UI
/// (<c>NMerchantCardRemoval.SetUsed</c> on <c>PurchaseCompleted</c> /
/// <c>OnCardRemovalUsed</c>), which headless does not run. <c>DoRemoveCard</c>
/// marks the host's own purchase; Lord's Parasol buys the removal itself
/// (<c>PurchaseEverything</c>, <c>ignoreCost</c>) and left it on offer again.
/// Every successful purchase reaches <c>Hook.AfterItemPurchased</c>, so the
/// removal is marked there. <c>SetUsed</c> is idempotent.
/// </summary>
public static class ShopRemovalUsed
{
    private static bool _installed;

    public static void Install()
    {
        if (_installed) return;
        _installed = true;
        var hook = typeof(Hook).GetMethod(nameof(Hook.AfterItemPurchased))
            ?? throw new MissingMethodException(nameof(Hook), nameof(Hook.AfterItemPurchased));
        new Harmony("sts2headless.shop_removal_used").Patch(
            hook, prefix: new HarmonyMethod(typeof(ShopRemovalUsed).GetMethod(nameof(Prefix), AccessTools.all)));
    }

    private static void Prefix(MerchantEntry itemPurchased)
    {
        if (itemPurchased is MerchantCardRemovalEntry removal) removal.SetUsed();
    }
}
