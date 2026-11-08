using Godot;
using HarmonyLib;
using HumanPlayRecorder.Ai;
using HumanPlayRecorder.Settings;
using MegaCrit.Sts2.Core.Context;
using MegaCrit.Sts2.Core.Entities.CardRewardAlternatives;
using MegaCrit.Sts2.Core.Entities.Merchant;
using MegaCrit.Sts2.Core.Nodes.Cards;
using MegaCrit.Sts2.Core.Nodes.Cards.Holders;
using MegaCrit.Sts2.Core.Nodes.Combat;
using MegaCrit.Sts2.Core.Nodes.CommonUi;
using MegaCrit.Sts2.Core.Nodes.Events;
using MegaCrit.Sts2.Core.Nodes.Potions;
using MegaCrit.Sts2.Core.Nodes.Relics;
using MegaCrit.Sts2.Core.Nodes.RestSite;
using MegaCrit.Sts2.Core.Nodes.Rewards;
using MegaCrit.Sts2.Core.Nodes.Screens;
using MegaCrit.Sts2.Core.Nodes.Screens.CardSelection;
using MegaCrit.Sts2.Core.Nodes.Screens.Map;
using MegaCrit.Sts2.Core.Nodes.Screens.Shops;
using MegaCrit.Sts2.Core.Nodes.Screens.TreasureRoomRelic;
using MegaCrit.Sts2.Core.Rewards;
using MegaCrit.Sts2.Core.Rooms;
using MegaCrit.Sts2.Core.Runs;

namespace HumanPlayRecorder.Recommend;

// One postfix per place the game builds an option node. Each badge asks AiAdvisor for the
// object the game itself confirms for that option (see Ai/AiAdvisor.ProbabilityFor); where
// the confirmed object wraps a model (shop entry, reward), the wrapper is asked as well.
static class BadgeHooks
{
    // NCard's origin is the card centre (NCardHolder puts the card at 0,0).
    static readonly Rect2 CardRect = new(-NCard.defaultSize / 2, NCard.defaultSize);
    static readonly AccessTools.FieldRef<NCardRewardSelectionScreen, IReadOnlyList<CardRewardAlternative>> extraOptions =
        AccessTools.FieldRefAccess<NCardRewardSelectionScreen, IReadOnlyList<CardRewardAlternative>>("_extraOptions");

    internal static void Install(Harmony harmony)
    {
        // Each screen's hook stands alone: a game update that renames one method costs only that badge.
        void Post(Type type, string method, string postfix) => UiGuard.Run($"badge hook {type.Name}.{method}", () =>
            harmony.Patch(AccessTools.DeclaredMethod(type, method) ?? throw new MissingMethodException(type.Name, method),
                postfix: new HarmonyMethod(typeof(BadgeHooks), postfix)));
        Post(typeof(NGridCardHolder), nameof(NGridCardHolder.Create), nameof(GridCard));
        Post(typeof(NHandCardHolder), nameof(NHandCardHolder.Create), nameof(HandCard));
        Post(typeof(NSelectedHandCardHolder), nameof(NSelectedHandCardHolder.Create), nameof(SelectedHandCard));
        foreach (var type in new[] { typeof(NDeckCardSelectScreen), typeof(NDeckUpgradeSelectScreen),
            typeof(NDeckTransformSelectScreen), typeof(NDeckEnchantSelectScreen) })
            Post(type, "_Ready", nameof(SelectionCancel));
        Post(typeof(NCardRewardSelectionScreen), nameof(NCardRewardSelectionScreen.RefreshOptions), nameof(CardRewardAlternatives));
        Post(typeof(NChooseABundleSelectionScreen), nameof(NChooseABundleSelectionScreen._Ready), nameof(Bundles));
        Post(typeof(NChooseARelicSelection), nameof(NChooseARelicSelection._Ready), nameof(ChooseRelic));
        Post(typeof(NMerchantInventory), nameof(NMerchantInventory.Initialize), nameof(Merchant));
        Post(typeof(NRewardButton), nameof(NRewardButton._Ready), nameof(Reward));
        Post(typeof(NEventOptionButton), nameof(NEventOptionButton._Ready), nameof(EventOption));
        Post(typeof(NRestSiteButton), nameof(NRestSiteButton._Ready), nameof(RestSite));
        Post(typeof(NMapPoint), "ConnectSignals", nameof(MapPoint));
        Post(typeof(NTreasureRoomRelicHolder), nameof(NTreasureRoomRelicHolder._Ready), nameof(TreasureRelic));
        Post(typeof(NPotionHolder), nameof(NPotionHolder._Ready), nameof(Potion));
        Post(typeof(NEndTurnButton), nameof(NEndTurnButton._Ready), nameof(EndTurn));
        Post(typeof(NProceedButton), nameof(NProceedButton._Ready), nameof(Proceed));
        Post(typeof(NChoiceSelectionSkipButton), nameof(NChoiceSelectionSkipButton._Ready), nameof(Skip));
        Post(typeof(NConfirmButton), nameof(NConfirmButton._Ready), nameof(Confirm));
    }

    // Grid holders are pooled and shared with the deck view, card library and inspect screens;
    // only those inside a choice screen are options. Covers card rewards, choose-a-card, and every
    // NCardGridSelectionScreen (upgrade, remove, transform, enchant, deck and combat-pile picks).
    static bool InCardChoiceScreen(Control host)
    {
        for (Node? node = host.GetParent(); node is not null; node = node.GetParent())
            if (node is NCardRewardSelectionScreen or NChooseACardSelectionScreen or NCardGridSelectionScreen ||
                node is NPlayerHand { IsInCardSelection: true })
                return true;
        return false;
    }

    // Overlays live outside the room tree; resolve their own content first, then the room
    // that opened a generic selector. Evaluate each time because card holders are pooled.
    static RecommendationCategory ScreenCategory(Control host)
    {
        for (Node? node = host; node is not null; node = node.GetParent())
        {
            if (node is NCardRewardSelectionScreen or NChooseABundleSelectionScreen or NRewardsScreen)
                return RecommendationCategory.CardRewards;
            if (node is NChooseARelicSelection) return RecommendationCategory.Relics;
            if (node is NMapScreen) return RecommendationCategory.Map;
        }
        return RunManager.Instance?.DebugOnlyGetState()?.CurrentRoom switch
        {
            EventRoom => RecommendationCategory.Events,
            RestSiteRoom => RecommendationCategory.RestSite,
            MerchantRoom => RecommendationCategory.Merchant,
            TreasureRoom => RecommendationCategory.Relics,
            CombatRoom => RecommendationCategory.Combat,
            _ => RecommendationCategory.CardRewards,
        };
    }

    static void Confirm(NConfirmButton __instance) => UiGuard.Run("selection confirm badge", () =>
        Badges.Attach(__instance, () => RunManager.Instance.DebugOnlyGetState()?.Players.FirstOrDefault(LocalContext.IsMe) is { } player
            ? LiveObservation.ConfirmationOption(player) : AiAdvisor.Options.Confirm, () => ScreenCategory(__instance),
            BadgeSide.Above, context: InCardChoiceScreen));

    static void SelectionCancel(NCardGridSelectionScreen __instance) => UiGuard.Run("selection cancel badge", () =>
    {
        if (AccessTools.Field(__instance.GetType(), "_closeButton")?.GetValue(__instance) is not Control close) return;
        Badges.Attach(close, () => AiAdvisor.Options.Skip, () => ScreenCategory(close),
            BadgeSide.Above, context: InCardChoiceScreen);
    });

    // Every postfix body below runs inside UiGuard: advice UI failures never reach the game's screen code.
    static void GridCard(NGridCardHolder? __result) => UiGuard.Run("card grid badge", () =>
    {
        if (__result is not { } holder) return;
        Badges.Attach(holder, () => holder.CardModel, () => ScreenCategory(holder),
            BadgeSide.Above, () => CardRect, context: InCardChoiceScreen);
    });

    // Combat hand, including the hand's own select mode (exhaust/discard/upgrade a card in hand).
    static void HandCard(NHandCardHolder __result) => UiGuard.Run("hand card badge", () =>
        Badges.Attach(__result, () => __result.CardModel, () => RecommendationCategory.Combat, BadgeSide.Above, () => CardRect));

    static void SelectedHandCard(NSelectedHandCardHolder __result) => UiGuard.Run("selected hand card badge", () =>
        Badges.Attach(__result, () => __result.CardModel, () => RecommendationCategory.Combat, BadgeSide.Above, () => CardRect));

    static void CardRewardAlternatives(NCardRewardSelectionScreen __instance) => UiGuard.Run("card reward alternative badges", () =>
    {
        var alternatives = extraOptions(__instance);
        var buttons = __instance.GetNode("UI/RewardAlternatives").GetChildren()
            .OfType<NCardRewardAlternativeButton>().Where(b => !b.IsQueuedForDeletion()).ToList();
        for (int i = 0; i < buttons.Count && i < alternatives.Count; i++)
        {
            var alternative = alternatives[i];
            Badges.Attach(buttons[i], () => alternative, () => RecommendationCategory.CardRewards, BadgeSide.Above,
                alternate: alternative.OptionId == "Skip" ? AiAdvisor.Options.Skip : null);
        }
    });

    static void Bundles(NChooseABundleSelectionScreen __instance) => UiGuard.Run("card pack badges", () =>
    {
        foreach (var bundle in __instance.GetNode("%BundleRow").GetChildren().OfType<NCardBundle>())
            Badges.Attach(bundle.Hitbox, () => bundle.Bundle, () => RecommendationCategory.CardRewards, BadgeSide.Above);
    });

    static void ChooseRelic(NChooseARelicSelection __instance) => UiGuard.Run("relic choice badges", () =>
    {
        foreach (var holder in __instance.GetNode("RelicRow").GetChildren().OfType<NRelicBasicHolder>())
            Badges.Attach(holder, () => holder.Relic?.Model, () => RecommendationCategory.Relics, BadgeSide.Below);
    });

    static IEnumerable<T> Descendants<T>(Node root) where T : Node
    {
        foreach (var child in root.GetChildren())
        {
            if (child is T found) yield return found;
            foreach (var inner in Descendants<T>(child)) yield return inner;
        }
    }

    // Shop cards, relics, potions and the card removal service. The price label sits under each
    // item, so the badge goes above it. Potions are the exception: the relic row's prices are
    // just above them, so a badge above a potion reads as a relic label; it goes under the
    // potion's own price instead.
    static void Merchant(NMerchantInventory __instance) => UiGuard.Run("shop badges", () =>
    {
        foreach (var slot in Descendants<NMerchantSlot>(__instance).ToList())
        {
            if (slot.Entry is not { } entry) continue;
            Func<object?> model = entry switch
            {
                MerchantCardEntry card => () => card.CreationResult?.Card,
                MerchantRelicEntry relic => () => relic.Model,
                MerchantPotionEntry potion => () => potion.Model,
                _ => () => entry,
            };
            bool isPotion = entry is MerchantPotionEntry;
            // Each badge stays inside its own slot's hitbox sideways (its full width in the hitbox's own coordinates)
            // and steps aside from any other slot's price, such as a hovered card's price growing into the next row.
            var slots = Descendants<NMerchantSlot>(__instance).ToList();
            Badges.Attach(slot.Hitbox, model, () => RecommendationCategory.Merchant,
                isPotion ? BadgeSide.Below : BadgeSide.Above, isPotion ? () => WithPrice(slot) : null,
                alternate: entry is MerchantCardRemovalEntry ? null : entry,
                cell: () => (0f, slot.Hitbox.Size.X),
                avoid: () => slots.Where(other => !ReferenceEquals(other, slot) && GodotObject.IsInstanceValid(other))
                    .Select(PriceContent).OfType<Rect2>().ToList());
        }
    });

    // What a slot's price draws (icon and number), in global coordinates.
    static Rect2? PriceContent(NMerchantSlot slot)
    {
        if (slot.GetNodeOrNull<Control>("Cost") is not { Visible: true } price) return null;
        Rect2? content = null;
        foreach (var part in Descendants<Control>(price).Where(c => c.Visible))
            content = content is { } so ? so.Merge(part.GetGlobalRect()) : part.GetGlobalRect();
        return content;
    }

    // The slot's hitbox grown to include its price row, in the hitbox's own coordinates.
    static Rect2 WithPrice(NMerchantSlot slot)
    {
        var hitbox = slot.Hitbox;
        var rect = new Rect2(Vector2.Zero, hitbox.Size);
        if (slot.GetNodeOrNull<Control>("Cost") is { } price)
            rect = rect.Merge(hitbox.GetGlobalTransform().AffineInverse() * price.GetGlobalRect());
        return rect;
    }

    // Combat rewards (gold, potion, relic, card reward, linked "pick one" sets).
    static void Reward(NRewardButton __instance) => UiGuard.Run("reward badge", () =>
    {
        if (__instance.Reward is not { } reward) return;
        object? inner = reward switch { RelicReward r => r.Relic, PotionReward p => p.Potion, _ => null };
        Badges.Attach(__instance, () => reward, () => reward switch
        {
            RelicReward => RecommendationCategory.Relics,
            PotionReward => RecommendationCategory.Potions,
            _ => RecommendationCategory.CardRewards,
        }, BadgeSide.RightInside, alternate: inner);
    });

    // Single-player events, ancients and multiplayer shared event votes.
    static void EventOption(NEventOptionButton __instance) => UiGuard.Run("event option badge", () =>
        Badges.Attach(__instance, () => __instance.Option, () => RecommendationCategory.Events, BadgeSide.Left));

    static void RestSite(NRestSiteButton __instance) => UiGuard.Run("rest site badge", () =>
        Badges.Attach(__instance, () => __instance.Option, () => RecommendationCategory.RestSite, BadgeSide.Above));

    // Map travel, single-player and multiplayer votes; all map point kinds call this.
    static void MapPoint(NMapPoint __instance) => UiGuard.Run("map point badge", () =>
        Badges.Attach(__instance, () => __instance.Point, () => RecommendationCategory.Map, BadgeSide.Right));

    // Multiplayer shared treasure relic picks (one relic per player, each player votes).
    static void TreasureRelic(NTreasureRoomRelicHolder __instance) => UiGuard.Run("treasure relic badge", () =>
        Badges.Attach(__instance, () => __instance.Relic?.Model, () => RecommendationCategory.Relics, BadgeSide.Above));

    static void Potion(NPotionHolder __instance) => UiGuard.Run("potion badge", () =>
        Badges.Attach(__instance, () => __instance.Potion?.Model, () => RecommendationCategory.Potions, BadgeSide.Below, compact: true));

    static void EndTurn(NEndTurnButton __instance) => UiGuard.Run("end turn badge", () =>
        Badges.Attach(__instance, () => AiAdvisor.Options.EndTurn, () => RecommendationCategory.Combat, BadgeSide.Above));

    static void Proceed(NProceedButton __instance) => UiGuard.Run("proceed badge", () =>
        Badges.Attach(__instance, () => AiAdvisor.Options.Proceed, () => ScreenCategory(__instance), BadgeSide.Above));

    static void Skip(NChoiceSelectionSkipButton __instance) => UiGuard.Run("skip badge", () =>
        Badges.Attach(__instance, () => AiAdvisor.Options.Skip, () => ScreenCategory(__instance), BadgeSide.Above));
}
