using HarmonyLib;
using MegaCrit.Sts2.Core.Context;
using MegaCrit.Sts2.Core.Entities.Merchant;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.Models.Relics;

namespace HumanPlayRecorder;

public static partial class RecorderMod
{
    static void InstallAiOwnerRules(Harmony harmony) =>
        harmony.Patch(AccessTools.Method(typeof(MembershipCard), nameof(MembershipCard.ModifyMerchantPrice)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AiMembershipPrice)));

    static void AiMembershipPrice(MembershipCard __instance, Player player, decimal originalPrice, ref decimal __result)
    {
        // The original hook already discounts local humans. AI ownership must
        // have the same price on host and real clients without swapping NetId.
        if (player == __instance.Owner && MultiplayerAi.IsAi(player.NetId) && !LocalContext.IsMe(player))
            __result = originalPrice * (__instance.DynamicVars["Discount"].BaseValue / 100m);
    }
}
