using HarmonyLib;
using MegaCrit.Sts2.Core.CardSelection;
using MegaCrit.Sts2.Core.Commands;
using MegaCrit.Sts2.Core.Entities.Cards;
using MegaCrit.Sts2.Core.Entities.Merchant;
using MegaCrit.Sts2.Core.Entities.Multiplayer;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.Entities.TreasureRelicPicking;
using MegaCrit.Sts2.Core.GameActions;
using MegaCrit.Sts2.Core.GameActions.Multiplayer;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Multiplayer.Game;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Game;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Game.Sync;
using MegaCrit.Sts2.Core.Rewards;
using MegaCrit.Sts2.Core.Rooms;
using MegaCrit.Sts2.Core.Runs;
using MegaCrit.Sts2.Core.Context;

namespace HumanPlayRecorder;

public static partial class RecorderMod
{
    static readonly Dictionary<Reward, int> rewardIndices = new();
    static readonly Dictionary<Player, CardReward> currentCardRewards = new();
    static readonly Dictionary<ulong, int> remoteMerchantPrices = new();
    static readonly Dictionary<(ulong player, string relic), Queue<int?>> pendingTreasureAwards = new();
    static TreasureRoomRelicSynchronizer? treasureSynchronizer;
    sealed record Selection(Player Player, CardModel[] Candidates, RecordedAction Input);
    sealed record BundleSelection(Player Player, IReadOnlyList<CardModel>[] Bundles, RecordedAction Input);

    static void InstallInventoryAdapters(Harmony h)
    {
        InstallChoiceContexts(h);
        h.Patch(AccessTools.Method(typeof(MerchantEntry), nameof(MerchantEntry.OnTryPurchaseWrapper)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforePurchase)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterRest)));
        // Card removal declares its own wrapper rather than overriding the base wrapper.
        h.Patch(AccessTools.DeclaredMethod(typeof(MerchantCardRemovalEntry),
                nameof(MerchantCardRemovalEntry.OnTryPurchaseWrapper),
                [typeof(MerchantInventory), typeof(bool), typeof(bool)]),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforePurchase)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterRest)));
        h.Patch(AccessTools.Method(typeof(RewardsSet), nameof(RewardsSet.Offer)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeRewards)));
        h.Patch(AccessTools.Method(typeof(RewardsSetSynchronizer), nameof(RewardsSetSynchronizer.BeginRewardsSet),
                [typeof(RewardsSet)]),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeSynchronizedRewards)));
        h.Patch(AccessTools.Method(typeof(Reward), nameof(Reward.SelectUnsynchronized)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeReward)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterReward)));
        foreach (var type in new[] { typeof(CardReward), typeof(PotionReward), typeof(RelicReward) })
            h.Patch(AccessTools.DeclaredMethod(type, nameof(Reward.OnSkipped)),
                postfix: new HarmonyMethod(typeof(RecorderMod), nameof(RewardSkipped)));
        h.Patch(AccessTools.Method(typeof(DiscardPotionGameAction), "ExecuteAction"),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeDiscard)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterTask)));
        h.Patch(AccessTools.Method(typeof(PickRelicAction), "ExecuteAction"),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeTreasurePick)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterTask)));
        h.Patch(AccessTools.Method(typeof(TreasureRoomRelicSynchronizer), "OnPicked"),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeTreasureVote)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterVoid)));
        h.Patch(AccessTools.Method(typeof(RelicCmd), nameof(RelicCmd.Obtain),
            [typeof(RelicModel), typeof(Player), typeof(int)]),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterTreasureObtain)));
        h.Patch(AccessTools.Method(typeof(PlayerChoiceSynchronizer), nameof(PlayerChoiceSynchronizer.SyncLocalChoice)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(RewardCardChoice)));
        h.Patch(AccessTools.Method(typeof(PlayerChoiceSynchronizer), "OnReceivePlayerChoice"),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(RemoteRewardCardChoice)));
        h.Patch(AccessTools.Method(typeof(RewardSynchronizer), "HandleGoldLostMessage"),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeRemoteMerchantGold)));
        h.Patch(AccessTools.Method(typeof(RewardSynchronizer), "HandleRewardObtainedMessage"),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeRemoteMerchantReward)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterVoid)));
        h.Patch(AccessTools.Method(typeof(OneOffSynchronizer), "HandleMerchantCardRemoval"),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeRemoteMerchantCardRemoval)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterVoid)));
        h.Patch(AccessTools.Method(typeof(CardSelectCmd), nameof(CardSelectCmd.FromSimpleGrid)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeGrid)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterSelection)));
        h.Patch(AccessTools.Method(typeof(CardSelectCmd), nameof(CardSelectCmd.FromChooseACardScreen)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeSingle)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterSingle)));
        foreach (var (method, prefix) in new[] {
            (nameof(CardSelectCmd.FromDeckGeneric), nameof(BeforeDeck)),
            (nameof(CardSelectCmd.FromDeckForUpgrade), nameof(BeforeUpgrade)),
            (nameof(CardSelectCmd.FromDeckForTransformation), nameof(BeforeTransform)),
            (nameof(CardSelectCmd.FromHand), nameof(BeforeHand)),
            (nameof(CardSelectCmd.FromSimpleGridForRewards), nameof(BeforeRewardGrid))
        })
            h.Patch(AccessTools.Method(typeof(CardSelectCmd), method),
                prefix: new HarmonyMethod(typeof(RecorderMod), prefix),
                postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterSelection)));
        h.Patch(AccessTools.Method(typeof(CardSelectCmd), nameof(CardSelectCmd.FromHandForUpgrade)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeHandUpgrade)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterSingle)));
        h.Patch(AccessTools.Method(typeof(CardSelectCmd), nameof(CardSelectCmd.FromCombatPile),
            [typeof(PlayerChoiceContext), typeof(CardPile), typeof(Player),
                typeof(CardSelectorPrefs), typeof(Func<CardModel, bool>)]),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforePile)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterSelection)));
        h.Patch(AccessTools.Method(typeof(CardSelectCmd), nameof(CardSelectCmd.FromDeckForEnchantment),
            [typeof(IReadOnlyList<CardModel>), typeof(EnchantmentModel), typeof(int), typeof(CardSelectorPrefs)]),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeEnchant)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterSelection)));
        h.Patch(AccessTools.Method(typeof(CardSelectCmd), nameof(CardSelectCmd.FromChooseABundleScreen)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeBundle)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterBundle)));
    }
    static void ResetInventoryAdapterState()
    {
        if (treasureSynchronizer is not null)
            treasureSynchronizer.RelicsAwarded -= TreasureAwarded;
        treasureSynchronizer = null;
        rewardIndices.Clear();
        currentCardRewards.Clear();
        remoteMerchantPrices.Clear();
        pendingTreasureAwards.Clear();
    }
    static void AttachTreasureSynchronizer()
    {
        if (run is null || run.Players.Count < 2) return;
        treasureSynchronizer = RunManager.Instance.TreasureRoomRelicSynchronizer;
        treasureSynchronizer.RelicsAwarded += TreasureAwarded;
    }
    static void BeforeDeck(Player player, CardSelectorPrefs prefs, Func<CardModel, bool>? filter,
        Func<CardModel, int>? sortingOrder, out Selection? __state)
    {
        __state = CaptureBoundary(() => {
            Ai.LiveObservation.RememberDeckSelection(player, prefs, ChoiceSource(null));
            IEnumerable<CardModel> cards = player.Deck.Cards;
            if (filter is not null) cards = cards.Where(filter);
            if (sortingOrder is not null) cards = cards.OrderBy(sortingOrder);
            return NewSelection(player, cards);
        });
    }
    static void BeforeUpgrade(Player player, CardSelectorPrefs prefs, out Selection? __state) =>
        __state = CaptureBoundary(() => {
            Ai.LiveObservation.RememberDeckSelection(player, prefs, ChoiceSource(null));
            return NewSelection(player, player.Deck.Cards.Where(c => c.IsUpgradable));
        });
    static void BeforeTransform(Player player, CardSelectorPrefs prefs, out Selection? __state) =>
        __state = CaptureBoundary(() => {
            Ai.LiveObservation.RememberDeckSelection(player, prefs, ChoiceSource(null));
            return NewSelection(player, player.Deck.Cards.Where(c => c.Type != CardType.Quest && c.IsTransformable));
        });
    static void BeforeHand(Player player, Func<CardModel, bool>? filter, PlayerChoiceContext context,
        CardSelectorPrefs prefs, AbstractModel source, out Selection? __state) =>
        __state = CaptureBoundary(() => {
            var cards = player.PlayerCombatState!.Hand.Cards.Where(filter ?? (_ => true)).ToArray();
            Ai.LiveObservation.RememberHandSelection(player, prefs, ChoiceSource(context, source), cards);
            return NewSelection(player, cards);
        });
    static void BeforeHandUpgrade(Player player, PlayerChoiceContext context, AbstractModel source,
        out Selection? __state) =>
        __state = CaptureBoundary(() => {
            var cards = player.PlayerCombatState!.Hand.Cards.Where(c => c.IsUpgradable).ToArray();
            var prefs = new CardSelectorPrefs(new MegaCrit.Sts2.Core.Localization.LocString(
                "gameplay_ui", "CHOOSE_CARD_UPGRADE_HEADER"), 1);
            Ai.LiveObservation.RememberHandSelection(player, prefs, ChoiceSource(context, source), cards);
            return NewSelection(player, cards);
        });
    static void BeforeRewardGrid(Player player, List<CardCreationResult> cards, PlayerChoiceContext context,
        CardSelectorPrefs prefs, out Selection? __state) =>
        __state = CaptureBoundary(() => {
            Ai.LiveObservation.RememberSelection(player, prefs, "None", ChoiceSource(context));
            return NewSelection(player, cards.Select(c => c.Card));
        });
    static void BeforePile(Player player, CardPile pile, Func<CardModel, bool>? filter,
        PlayerChoiceContext context, CardSelectorPrefs prefs,
        out Selection? __state)
    {
        // Native pending-choice indices use the underlying pile order, not a
        // draw-pile UI's rarity-sorted display order.
        __state = CaptureBoundary(() => {
            Ai.LiveObservation.RememberSelection(player, prefs, pile.Type.ToString(), ChoiceSource(context));
            return NewSelection(player, pile.Cards.Where(filter ?? (_ => true)));
        });
    }
    static void BeforeEnchant(IReadOnlyList<CardModel> cards, EnchantmentModel enchantment,
        int amount, CardSelectorPrefs prefs, out Selection? __state)
    {
        __state = CaptureBoundary<Selection>(() => {
            if (cards.Count == 0 || run is null) return null;
            var player = cards[0].Owner;
            Ai.LiveObservation.RememberDeckSelection(player, prefs, ChoiceSource(null), enchantment, amount);
            return NewSelection(player, cards.OrderBy(c => player.Deck.Cards.ToList().IndexOf(c)));
        });
    }
    static void BeforeBundle(Player player, IReadOnlyList<IReadOnlyList<CardModel>> bundles,
        out BundleSelection? __state)
    {
        __state = null;
        if (run is null || journal is null) return;
        try
        {
            if (bundles.Count == 0) { journal.Invalidate(); return; }
            __state = new(player, bundles.ToArray(),
                Stamp(new("PickCardBundle", run.GetPlayerSlotIndex(player))));
        }
        catch (Exception) { journal.Invalidate(); }
    }
    static void AfterBundle(ref Task<IEnumerable<CardModel>> __result, BundleSelection? __state)
    {
        if (__state is not null && run is not null && journal is not null)
            __result = CaptureBundle(__result, __state, run, journal);
    }
    static async Task<IEnumerable<CardModel>> CaptureBundle(Task<IEnumerable<CardModel>> task,
        BundleSelection selection, MegaCrit.Sts2.Core.Runs.RunState state, LocalJournal destination)
    {
        var selected = await task;
        try
        {
            var chosen = selected.ToArray();
            int index = Array.FindIndex(selection.Bundles, b => ReferenceEquals(b, selected));
            if (index < 0)
            {
                var matches = selection.Bundles.Select((bundle, i) => (bundle, i))
                    .Where(x => x.bundle.Count == chosen.Length &&
                        x.bundle.Zip(chosen).All(pair => ReferenceEquals(pair.First, pair.Second)))
                    .Select(x => x.i).ToArray();
                if (matches.Length != 1) { destination.Invalidate(); return selected; }
                index = matches[0];
            }
            var action = selection.Input with {
                index = index,
                cards = chosen.Select(c => new CardRef(c.Id.Entry, c.CurrentUpgradeLevel)).ToArray()
            };
            RecordingFormat.Validate(action);
            await Observe(Task.CompletedTask, action, state, destination);
        }
        catch (Exception) { destination.Invalidate(); }
        return selected;
    }
    static void BeforeTreasurePick(PickRelicAction __instance, Player ____player, int? ____relicIndex,
        out RecordedAction? __state)
    {
        __state = null;
        if (run is null || journal is null) return;
        if (run.Players.Count != 1) return; // Multiplayer transport is a separate verification gate.
        try
        {
            var relics = RunManager.Instance.TreasureRoomRelicSynchronizer.CurrentRelics;
            if (____relicIndex is not { } index || relics is null || index < 0 || index >= relics.Count)
            { journal.Invalidate(); return; }
            var relic = relics[index];
            __state = Track(__instance, new("Collect", run.GetPlayerSlotIndex(____player),
                reward_index: index, reward_type: "relic", relic_id: relic.Id.Entry));
            RecordingFormat.Validate(__state);
        }
        catch (Exception) { journal.Invalidate(); __state = null; }
    }
    static void BeforeTreasureVote(Player player, int? index, out RecordedAction? __state)
    {
        __state = null;
        if (run is null || journal is null || run.Players.Count < 2) return;
        try
        {
            if (index is null)
                __state = Stamp(new("PickRelicReward", run.GetPlayerSlotIndex(player),
                    reward_type: "relic", sync_kind: "treasure_vote", cancelled: true));
            else
            {
                var relics = RunManager.Instance.TreasureRoomRelicSynchronizer.CurrentRelics;
                if (relics is null || index < 0 || index >= relics.Count)
                { journal.Invalidate(); return; }
                __state = Stamp(new("PickRelicReward", run.GetPlayerSlotIndex(player),
                    reward_index: index, reward_type: "relic", relic_id: relics[index.Value].Id.Entry,
                    sync_kind: "treasure_vote"));
            }
            RecordingFormat.Validate(__state);
        }
        catch (Exception) { journal.Invalidate(); __state = null; }
    }
    static void TreasureAwarded(List<RelicPickingResult> results)
    {
        if (run is null || journal is null || run.Players.Count < 2) return;
        try
        {
            var offered = RunManager.Instance.TreasureRoomRelicSynchronizer.CurrentRelics;
            foreach (var result in results)
            {
                if (result.player is null || result.type == RelicPickingResultType.Skipped) continue;
                int found = offered?.ToList().FindIndex(r => ReferenceEquals(r, result.relic) || r.Id == result.relic.Id) ?? -1;
                var key = (result.player.NetId, result.relic.Id.Entry);
                if (!pendingTreasureAwards.TryGetValue(key, out var queue))
                    pendingTreasureAwards[key] = queue = new();
                queue.Enqueue(found >= 0 ? found : null);
            }
        }
        catch (Exception) { journal.Invalidate(); }
    }
    static void AfterTreasureObtain(ref Task<RelicModel> __result, RelicModel relic, Player player)
    {
        if (run is null || journal is null || run.Players.Count < 2) return;
        var key = (player.NetId, relic.Id.Entry);
        if (!pendingTreasureAwards.TryGetValue(key, out var queue) || queue.Count == 0) return;
        int? offeredIndex = queue.Dequeue();
        if (queue.Count == 0) pendingTreasureAwards.Remove(key);
        __result = FinishTreasureObtain(__result, player, relic, offeredIndex, run, journal);
    }
    static async Task<RelicModel> FinishTreasureObtain(Task<RelicModel> task, Player player,
        RelicModel relic, int? offeredIndex, RunState state, LocalJournal destination)
    {
        var result = await task;
        try
        {
            var action = Stamp(new("PickRelicReward", state.GetPlayerSlotIndex(player),
                reward_index: offeredIndex, reward_type: "relic", relic_id: relic.Id.Entry,
                sync_kind: "treasure_pick"));
            RecordingFormat.Validate(action);
            await Observe(Task.CompletedTask, action, state, destination);
        }
        catch (Exception) { destination.Invalidate(); }
        return result;
    }
    static void BeforePurchase(MerchantEntry __instance, Player ____player, MerchantInventory? inventory,
        out RecordedAction? __state)
    {
        __state = null;
        if (run is null || journal is null) return;
        try
        {
            int seat = run.GetPlayerSlotIndex(____player);
            int price = __instance.Cost;
            bool multiplayer = run.Players.Count > 1;
            __state = __instance switch
            {
                MerchantCardEntry c when inventory is not null && c.CreationResult is not null =>
                    new("BuyCard", seat, card: new(c.CreationResult.Card.Id.Entry,
                        c.CreationResult.Card.CurrentUpgradeLevel),
                        index: multiplayer ? null : inventory.CardEntries.ToList().IndexOf(c),
                        slot_index: multiplayer ? null : inventory.CardEntries.ToList().IndexOf(c), price: price,
                        sync_kind: multiplayer ? "merchant_outcome" : null),
                MerchantPotionEntry p when inventory is not null && p.Model is not null =>
                    new("BuyPotion", seat, potion_id: p.Model.Id.Entry,
                        index: multiplayer ? null : inventory.PotionEntries.ToList().IndexOf(p),
                        slot_index: multiplayer ? null : inventory.PotionEntries.ToList().IndexOf(p), price: price,
                        sync_kind: multiplayer ? "merchant_outcome" : null),
                MerchantRelicEntry r when inventory is not null && r.Model is not null =>
                    new("BuyRelic", seat, relic_id: r.Model.Id.Entry,
                        index: multiplayer ? null : inventory.RelicEntries.ToList().IndexOf(r),
                        slot_index: multiplayer ? null : inventory.RelicEntries.ToList().IndexOf(r), price: price,
                        sync_kind: multiplayer ? "merchant_outcome" : null),
                MerchantCardRemovalEntry => new("RemoveCard", seat, price: price,
                    sync_kind: multiplayer ? "merchant_outcome" : null),
                _ => null
            };
            if (__state is null) journal.Invalidate();
            else { RecordingFormat.Validate(__state); __state = Stamp(__state); }
        }
        catch (Exception) { journal.Invalidate(); __state = null; }
    }
    static void IndexRewards(RewardsSet rewards) => CaptureBoundary(() => {
        if (run is null) return;
        Ai.LiveObservation.RememberRewards(rewards);
        for (int i = 0; i < rewards.Rewards.Count; i++)
        {
            var reward=rewards.Rewards[i];rewardIndices[reward]=i;
            if(reward is CardReward card)foreach(var candidate in card.Cards)PerRunContentGate.Seen(candidate);
            if(reward is RelicReward {Relic:{} relic})PerRunContentGate.Seen(relic);
            if(reward is PotionReward {Potion:{} potion})PerRunContentGate.Seen(potion);
        }
    });
    static void BeforeRewards(RewardsSet __instance) => IndexRewards(__instance);
    static void BeforeSynchronizedRewards(RewardsSet __0) => IndexRewards(__0);
    static string RewardType(Reward reward) => reward switch {
        CardReward => "card", GoldReward => "gold", PotionReward => "potion",
        RelicReward => "relic", _ => "unsupported"
    };
    static void BeforeReward(Reward __instance, out RecordedAction? __state)
    {
        // An AI seat's card-reward default (pick the first card) must exist even
        // when recording below gives up on this reward.
        if (__instance is CardReward aiCards && MultiplayerAi.IsAi(__instance.Player.NetId))
            currentCardRewards[__instance.Player] = aiCards;
        __state = CaptureBoundary<RecordedAction>(() => {
            if (run is null || journal is null) return null;
            if (!rewardIndices.TryGetValue(__instance, out int index) || RewardType(__instance) == "unsupported")
            { journal.Invalidate(); return null; }
            if (__instance is CardReward cardReward) currentCardRewards[__instance.Player] = cardReward;
            int seat = run.GetPlayerSlotIndex(__instance.Player);
            RecordedAction action = __instance switch {
                PotionReward p => new("PickPotion", seat, reward_index: index, reward_type: "potion",
                    potion_id: p.Potion?.Id.Entry),
                RelicReward r => new("PickRelicReward", seat, reward_index: index, reward_type: "relic",
                    relic_id: r.Relic?.Id.Entry),
                CardReward => new("OpenReward", seat, reward_index: index, reward_type: "card"),
                _ => new("Collect", seat, reward_index: index, reward_type: "gold")
            };
            return Stamp(action);
        });
    }
    static void AfterReward(Reward __instance, ref Task<bool> __result, RecordedAction? __state)
    {
        // Unrecorded AI card reward: FinishReward will not run, so the AI default
        // registered in BeforeReward ends with the reward's own selection loop.
        if (__state is null && __instance is CardReward && MultiplayerAi.IsAi(__instance.Player.NetId))
            __result = ForgetAiCardReward(__result, __instance.Player);
        if (__state is null || journal is null || run is null) return;
        __result = FinishReward(__result, __state, run, journal);
    }
    static async Task<bool> ForgetAiCardReward(Task<bool> task, Player player)
    {
        try { return await task; }
        finally { currentCardRewards.Remove(player); }
    }
    static async Task<bool> FinishReward(Task<bool> task, RecordedAction action,
        MegaCrit.Sts2.Core.Runs.RunState state, LocalJournal destination)
    {
        try { return await ObserveRest(task, action, state, destination); }
        finally
        {
            if (action.kind == "OpenReward" && action.player >= 0 && action.player < state.Players.Count)
                currentCardRewards.Remove(state.Players[action.player]);
        }
    }
    static void RewardSkipped(Reward __instance) => CaptureBoundary(() => {
        if (run is null || journal is null) return;
        if (!rewardIndices.TryGetValue(__instance, out int index))
        { journal.Invalidate(); return; }
        string type = RewardType(__instance);
        string kind = type switch { "potion" => "SkipPotion", "relic" => "SkipRelic", _ => "SkipRewards" };
        _ = Observe(Task.CompletedTask, new(kind, run.GetPlayerSlotIndex(__instance.Player),
            reward_index: index, reward_type: type), run, journal);
    });
    static void BeforeDiscard(DiscardPotionGameAction __instance, Player ____player, uint ____potionSlotIndex,
        out RecordedAction? __state)
    {
        __state = null;
        if (run is null || journal is null) return;
        try
        {
            var potion = ____player.GetPotionAtSlotIndex((int)____potionSlotIndex);
            if (potion is null) return; // Original action cancels an empty slot.
            __state = Track(__instance, new("DiscardPotion", run.GetPlayerSlotIndex(____player),
                slot_index: (int)____potionSlotIndex, potion_id: potion.Id.Entry));
        }
        catch (Exception) { journal.Invalidate(); }
    }
    static void RewardCardChoice(Player player, uint choiceId, PlayerChoiceResult result)
    {
        if (run is null || journal is null || !currentCardRewards.TryGetValue(player, out var reward)) return;
        try
        {
            int? index = result.AsIndexOrNull();
            var cards = reward.Cards.ToArray();
            int seat = run.GetPlayerSlotIndex(player);
            RecordedAction action;
            if (index is null)
            {
                // TestMode represents the built-in Skip alternative as null.
                action = new("Skip", seat);
            }
            else if (index >= 0 && index < cards.Length)
            {
                action = new("PickCard", seat, index: index,
                    card: new(cards[index.Value].Id.Entry, cards[index.Value].CurrentUpgradeLevel),
                    reward_index: rewardIndices[reward], reward_type: "card", choice_id: choiceId);
            }
            else
            {
                int alternative = index.Value - cards.Length;
                int skipAlternative = reward.CanSkip ? 0 : -1;
                int rerollAlternative = reward.CanReroll ? (reward.CanSkip ? 1 : 0) : -1;
                if (alternative == skipAlternative)
                    action = new("Skip", seat);
                else if (alternative == rerollAlternative)
                    action = new("RerollCardReward", seat,
                        reward_index: rewardIndices[reward], reward_type: "card", choice_id: choiceId);
                else
                { journal.Invalidate(); return; } // Unknown/custom alternatives stay fail-closed.
            }
            // This is a choice boundary before the awaiting reward continuation;
            // it is not a claim that the acquired card is already in the deck.
            _ = Observe(Task.CompletedTask, action, run, journal);
        }
        catch (Exception) { journal.Invalidate(); }
    }
    static void RemoteRewardCardChoice(Player player, uint choiceId, NetPlayerChoiceResult result)
    {
        if (run is null || journal is null || LocalContext.IsMe(player)) return;
        try { RewardCardChoice(player, choiceId, PlayerChoiceResult.FromNetData(player, run, result)); }
        catch (Exception) { journal.Invalidate(); }
    }
    static Player? RemotePlayer(ulong senderId) => run?.Players.FirstOrDefault(p => p.NetId == senderId);
    static void BeforeRemoteMerchantGold(GoldLostMessage message, ulong senderId)
    {
        if (run is null || journal is null || run.Players.Count < 2 || run.CurrentRoom is not MerchantRoom) return;
        var player = RemotePlayer(senderId);
        if (player is null || LocalContext.IsMe(player)) return;
        if (message.goldLost < 0 || remoteMerchantPrices.ContainsKey(senderId))
        { journal.Invalidate(); return; }
        remoteMerchantPrices[senderId] = message.goldLost;
    }
    static void BeforeRemoteMerchantReward(RewardObtainedMessage message, ulong senderId,
        out RecordedAction? __state)
    {
        __state = null;
        if (run is null || journal is null || run.Players.Count < 2 || run.CurrentRoom is not MerchantRoom) return;
        var player = RemotePlayer(senderId);
        if (player is null || LocalContext.IsMe(player) || message.wasSkipped) return;
        if (!remoteMerchantPrices.Remove(senderId, out int price)) { journal.Invalidate(); return; }
        try
        {
            int seat = run.GetPlayerSlotIndex(player);
            __state = message.rewardType switch
            {
                MegaCrit.Sts2.Core.Rewards.RewardType.Card when message.cardModel is not null =>
                    Stamp(new("BuyCard", seat, card: new(message.cardModel.Id.Entry,
                        message.cardModel.CurrentUpgradeLevel), price: price, sync_kind: "merchant_outcome")),
                MegaCrit.Sts2.Core.Rewards.RewardType.Potion when message.potionModel is not null =>
                    Stamp(new("BuyPotion", seat, potion_id: message.potionModel.Id.Entry,
                        price: price, sync_kind: "merchant_outcome")),
                MegaCrit.Sts2.Core.Rewards.RewardType.Relic when message.relicModel is not null =>
                    Stamp(new("BuyRelic", seat, relic_id: message.relicModel.Id.Entry,
                        price: price, sync_kind: "merchant_outcome")),
                _ => null
            };
            if (__state is null) { journal.Invalidate(); return; }
            RecordingFormat.Validate(__state);
        }
        catch (Exception) { journal.Invalidate(); __state = null; }
    }
    static void BeforeRemoteMerchantCardRemoval(MerchantCardRemovalMessage message, ulong senderId,
        out RecordedAction? __state)
    {
        __state = null;
        if (run is null || journal is null || run.Players.Count < 2 || run.CurrentRoom is not MerchantRoom) return;
        var player = RemotePlayer(senderId);
        if (player is null || LocalContext.IsMe(player)) return;
        try
        {
            if (message.goldCost < 0) { journal.Invalidate(); return; }
            __state = Stamp(new("RemoveCard", run.GetPlayerSlotIndex(player), price: message.goldCost,
                sync_kind: "merchant_outcome"));
            RecordingFormat.Validate(__state);
        }
        catch (Exception) { journal.Invalidate(); __state = null; }
    }
    static void BeforeGrid(Player player, IReadOnlyList<CardModel> cardsIn,
        PlayerChoiceContext context, CardSelectorPrefs prefs, out Selection? __state)
        => __state = CaptureBoundary(() => {
            Ai.LiveObservation.RememberSelection(player, prefs, "None", ChoiceSource(context));
            return NewSelection(player, cardsIn);
        });
    static void BeforeSingle(Player player, IReadOnlyList<CardModel> cards,
        PlayerChoiceContext context, bool canSkip, out Selection? __state)
        => __state = CaptureBoundary(() => {
            Ai.LiveObservation.RememberSingleSelection(player, canSkip, ChoiceSource(context));
            return NewSelection(player, cards);
        });
    static Selection? NewSelection(Player player, IEnumerable<CardModel> cards)
    {
        if(run is null)return null;
        var candidates=cards.ToArray();
        // An AI seat's default answer must not depend on recording: a failed stamp
        // below would otherwise leave its pending choice with nothing to submit.
        AiOfferSelection(player,candidates);
        foreach(var candidate in candidates)PerRunContentGate.Seen(candidate);
        return new(player,candidates,Stamp(new("Choose",run.GetPlayerSlotIndex(player))));
    }
    static void AfterSelection(ref Task<IEnumerable<CardModel>> __result, Selection? __state)
    {
        if (__state is not null && run is not null && journal is not null)
            __result = CaptureSelection(__result, __state, run, journal);
    }
    static async Task<IEnumerable<CardModel>> CaptureSelection(Task<IEnumerable<CardModel>> task,
        Selection selection, MegaCrit.Sts2.Core.Runs.RunState state, LocalJournal destination)
    {
        IEnumerable<CardModel> selected;
        try { selected = await task; }
        finally { Ai.LiveObservation.ForgetSelection(selection.Player); }
        try
        {
            var chosen = selected.ToArray();
            int[] indices = chosen.Select(c => Array.IndexOf(selection.Candidates, c)).ToArray();
            var action = selection.Input with {
                indices = indices,
                cards = chosen.Select(c => new CardRef(c.Id.Entry, c.CurrentUpgradeLevel)).ToArray()
            };
            RecordingFormat.Validate(action);
            await Observe(Task.CompletedTask, action, state, destination);
        }
        catch (Exception) { destination.Invalidate(); }
        return selected;
    }
    static void AfterSingle(ref Task<CardModel?> __result, Selection? __state)
    {
        if (__state is not null && run is not null && journal is not null)
            __result = CaptureSingle(__result, __state, run, journal);
    }
    static async Task<CardModel?> CaptureSingle(Task<CardModel?> task, Selection selection,
        MegaCrit.Sts2.Core.Runs.RunState state, LocalJournal destination)
    {
        var card = await task;
        await CaptureSelection(Task.FromResult<IEnumerable<CardModel>>(
            card is null ? [] : [card]), selection, state, destination);
        return card;
    }
}
