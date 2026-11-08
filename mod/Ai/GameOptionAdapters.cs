using System.Text.Json;
using MegaCrit.Sts2.Core.Combat;
using MegaCrit.Sts2.Core.Entities.Cards;
using MegaCrit.Sts2.Core.Entities.Creatures;
using MegaCrit.Sts2.Core.Entities.Merchant;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.Entities.Potions;
using MegaCrit.Sts2.Core.Entities.RestSite;
using MegaCrit.Sts2.Core.Events;
using MegaCrit.Sts2.Core.Map;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Rewards;
using MegaCrit.Sts2.Core.Runs;
using MegaCrit.Sts2.Core.Rooms;
using static HumanPlayRecorder.Ai.AiAdvisor;

namespace HumanPlayRecorder.Ai;

/// <summary>
/// Main-thread projections of original game choices. These produce original input
/// DTOs, never simulator slots. The native original-input folder resolves each DTO
/// independently and admits advice only for a bijection onto its entire legal set.
/// </summary>
public static class GameOptionAdapters
{
    static CardRef Card(CardModel card) { FrameCaptureBudget.Check(); return new(card.Id.Entry, card.CurrentUpgradeLevel); }
    static LiveOption Option(object source, RecordedAction input, DecisionAction action, object? target = null)
    {
        FrameCaptureBudget.Check();
        return new(source, JsonSerializer.Serialize(input, RecordingFormat.Json), action, target);
    }

    public static void Publish(RunState run, Player player, IEnumerable<Reward> rewardLedger,
        IReadOnlyList<LiveOption> options, int? parentSequence = null) =>
        PublishLive(run, player, rewardLedger, options, parentSequence);

    static void PublishLive(RunState run, Player player, IEnumerable<Reward> rewardLedger,
        IReadOnlyList<LiveOption> options, int? parentSequence)
    {
        using var budget = new FrameCaptureBudget();
        int seat = run.GetPlayerSlotIndex(player);
        var snapshot = LiveObservation.Capture(run, seat, options);
        ReplayCheckpoint? before = null;
        try { before = ReplayCheckpoints.Capture(run, rewardLedger); }
        catch (AdviceUnavailableException) { throw; }
        catch (Exception failure) { RecorderMod.SwallowedCheckpointFailure("PublishLive", failure); UseLiveState(); }
        FrameCaptureBudget.Check();
        SetLiveDecision(seat, options, snapshot, before, parentSequence);
    }

    public static void Capture(RunState run, Player player, IEnumerable<Reward> rewardLedger,
        Func<IReadOnlyList<LiveOption>> enumerate, int? parentSequence = null)
    {
        try {
            using var budget = new FrameCaptureBudget();
            Publish(run, player, rewardLedger, enumerate(), parentSequence);
        }
        catch (Exception error) { RefuseDecision(run.GetPlayerSlotIndex(player), error.Message, AdviceUnavailableException.ReasonOf(error)); }
    }

    static TargetRef? Target(Creature? target)
    {
        if (target is null) return null;
        var combat = target.CombatState!;
        int creature = combat.Creatures.ToList().IndexOf(target);
        if (target.IsPlayer)
            return new("player", combat.Players.ToList().IndexOf(target.Player!), target.Player!.Character.Id.Entry, creature);
        int enemy = combat.Enemies.ToList().IndexOf(target);
        if (enemy < 0) {
            var pets = combat.Players.SelectMany(p => p.PlayerCombatState!.Pets).ToList();
            return new(target.IsPet ? "pet" : "creature",
                target.IsPet ? pets.IndexOf(target) : creature, target.Monster?.Id.Entry ?? "", creature);
        }
        return new("enemy", enemy, target.Monster?.Id.Entry ?? "", creature);
    }

    public static IReadOnlyList<LiveOption> Combat(RunState run, Player player, CombatState state) =>
        CombatCandidates(run, player, state, false);

    internal static IReadOnlyList<LiveOption> CombatWithMasks(RunState run, Player player, CombatState state) =>
        CombatCandidates(run, player, state, true);

    static IReadOnlyList<LiveOption> CombatCandidates(RunState run, Player player, CombatState state, bool masked)
    {
        var result = new List<LiveOption>();
        int seat = run.GetPlayerSlotIndex(player);
        if (!player.Creature.IsAlive || state.CurrentSide.ToString() != "Player" ||
            player.PlayerCombatState is not { } combat) return result;
        var targets = new Creature?[] { null }.Concat(state.Players.Select(p => (Creature?)p.Creature))
            .Concat(state.Enemies).Concat(state.Players.SelectMany(p => p.PlayerCombatState!.Pets)).ToArray();
        foreach (var (card, hand) in combat.Hand.Cards.Select((card, hand) => (card, hand)))
            if (combat.Phase.ToString() == "Play" && !CombatManager.Instance.IsPlayerReadyToEndTurn(player)) {
                bool playable = MultiplayerAi.IsAi(player.NetId) ? card.CanPlay() : GameUiPlayability.Read(card);
                if (!masked && !playable) continue;
                var targetType = LiveModelData.Property(card, "TargetType", () => card.TargetType);
                bool ValidTarget(Creature? target) => target is null
                    ? targetType is not (TargetType.AnyEnemy or TargetType.AnyAlly)
                    : target.IsAlive && (targetType == TargetType.AnyEnemy
                        ? target.Side != card.Owner.Creature.Side
                        : targetType == TargetType.AnyAlly && target.Side == card.Owner.Creature.Side);
                IEnumerable<Creature?> candidates = !masked ? targets.Where(ValidTarget) : targetType switch {
                    TargetType.AnyEnemy => state.Enemies,
                    TargetType.AnyAlly => targets.Where(t => t?.Side.ToString() == "Player"),
                    _ => new Creature?[] { null }
                };
                foreach (var target in candidates)
                    result.Add(Option(card, new("PlayCard", seat, hand_index: hand, card: Card(card),
                        target: Target(target)), DecisionAction.PlayCard, target) with {
                            Legal = !masked || playable && ValidTarget(target)
                        });
            }
        foreach (var (potion, slot) in player.PotionSlots.Select((potion, slot) => (potion, slot))) {
            if (potion is null) continue;
            if (masked || player.CanUseOrRemovePotions)
                result.Add(Option(potion, new("DiscardPotion", seat, slot_index: slot, potion_id: potion.Id.Entry),
                    DecisionAction.DiscardPotion) with { Legal = player.CanUseOrRemovePotions });
            bool usable = potion.Usage is PotionUsage.AnyTime or PotionUsage.CombatOnly &&
                player.CanUseOrRemovePotions && potion.PassesCustomUsabilityCheck && !potion.IsQueued;
            if (!masked && !usable) continue;
            IEnumerable<Creature?> recipients = !masked ? targets.Where(potion.IsValidTarget) : potion.TargetType switch {
                TargetType.AnyEnemy => state.Enemies,
                TargetType.AnyPlayer => state.Players.Select(p => (Creature?)p.Creature),
                TargetType.Self => new Creature?[] { player.Creature },
                _ => new Creature?[] { null }
            };
            foreach (var target in recipients)
                result.Add(Option(potion, new("UsePotion", seat, slot_index: slot, potion_id: potion.Id.Entry,
                    target: Target(target)), DecisionAction.UsePotion, target) with {
                        Legal = !masked || usable && potion.IsValidTarget(target)
                    });
        }
        if (combat.Phase.ToString() == "Play" && !CombatManager.Instance.IsPlayerReadyToEndTurn(player))
            result.Add(Option(Options.EndTurn, new("EndTurn", seat), DecisionAction.EndTurn));
        return result;
    }

    public static IReadOnlyList<LiveOption> CardReward(int seat, CardReward reward, int rewardIndex)
    {
        var cards = reward.Cards.ToArray();
        var offered = cards.Select(Card).ToArray();
        var result = cards.Select((card, index) => Option(card,
            new RecordedAction("PickCard", seat, card: Card(card), reward_index: rewardIndex,
                reward_type: "card", index: index) { offered_cards = offered }, DecisionAction.PickCard)).ToList();
        if (reward.CanSkip) result.Add(Option(Options.Skip, new("Skip", seat), DecisionAction.Skip));
        if (reward.CanReroll) result.Add(Option(reward, new("RerollCardReward", seat,
            reward_index: rewardIndex, reward_type: "card"), DecisionAction.Select));
        return result.Select(option => option with { Reward = reward }).ToArray();
    }

    public static IReadOnlyList<LiveOption> Events(int seat, IReadOnlyList<EventOption> options,
        int? multiplayerGeneration = null) =>
        options.Select((option, index) => (option, index)).Where(p => !p.option.IsLocked)
            .Select(p => Option(p.option, new("EventChoice", seat, option_id: p.option.TextKey, index: p.index,
                sync_kind: multiplayerGeneration.HasValue ? "event_vote" : null, generation: multiplayerGeneration),
                DecisionAction.EventChoice)).ToArray();

    public static IReadOnlyList<LiveOption> Bundles(int seat, IReadOnlyList<IReadOnlyList<CardModel>> bundles) =>
        bundles.Select((bundle, index) => Option(bundle,
            new("PickCardBundle", seat, cards: bundle.Select(Card).ToArray(), index: index),
            DecisionAction.Select)).ToArray();

    public static IReadOnlyList<LiveOption> Rest(int seat, IReadOnlyList<RestSiteOption> options) =>
        options.Select((option, index) => (option, index)).Where(p => p.option.IsEnabled)
            .Select(p => Option(p.option, new("RestOption", seat, option_id: p.option.OptionId, index: p.index),
                DecisionAction.RestOption)).ToArray();

    /// <summary>Pass the game's allowed destinations, not merely geometric children.</summary>
    public static IReadOnlyList<LiveOption> Map(int seat, IEnumerable<MapPoint> legalDestinations,
        int? multiplayerGeneration = null) =>
        legalDestinations.Select(point => Option(point, new("Move", seat, coord: [point.coord.col, point.coord.row],
            sync_kind: multiplayerGeneration.HasValue ? "map_vote" : null, generation: multiplayerGeneration),
            DecisionAction.Move)).ToArray();

    public static IReadOnlyList<LiveOption> Map(RunState run, Player player)
    {
        bool canCancel = run.Players.Count > 1 && RunManager.Instance.MapSelectionSynchronizer.GetVote(player).HasValue;
        IEnumerable<MapPoint> destinations;
        var map = run.Map;
        var current = run.CurrentMapPoint;
        if (!run.VisitedMapCoords.Any() || current is null) destinations = [map.StartingMapPoint];
        else if (current.coord == map.BossMapPoint.coord && map.SecondBossMapPoint is { } second) destinations = [second];
        else if (current.coord.row == map.GetRowCount() - 1) destinations = [map.BossMapPoint];
        else destinations = MapTravel.GetTravelablePointsFrom(run, current);
        int seat = run.GetPlayerSlotIndex(player);
        int generation = RunManager.Instance.MapSelectionSynchronizer.MapGenerationCount;
        var result = Map(seat, destinations, run.Players.Count > 1 ? generation : null).ToList();
        if (canCancel) result.Add(Option(Options.Skip, new("Move", seat, sync_kind: "map_vote",
            generation: generation, cancelled: true), DecisionAction.Skip));
        return result;
    }

    /// <summary>Candidate order is the original selector's offered order.</summary>
    public static IReadOnlyList<LiveOption> Cards(int seat, IReadOnlyList<CardModel> candidates, bool canSkip)
    {
        var result = candidates.Select((card, index) => Option(card,
            new("Choose", seat, indices: [index], cards: [Card(card)]), DecisionAction.Select)).ToList();
        if (canSkip) result.Add(Option(Options.Skip, new("Choose", seat, indices: [], cards: []), DecisionAction.Confirm));
        return result;
    }

    public static IReadOnlyList<LiveOption> Shop(RunState run, Player player, MerchantRoom room)
    {
        int seat = run.GetPlayerSlotIndex(player);
        if (seat >= room.Inventories.Count)
            throw new AdviceUnavailableException(AdviceReason.ShopSyncing, LiveObservation.ShopListUnavailable);
        return Shop(seat, room.Inventories[seat], run.Players.Count > 1);
    }

    public static IReadOnlyList<LiveOption> Shop(int seat, MerchantInventory inventory, bool multiplayer)
    {
        var result = new List<LiveOption>();
        string? sync = multiplayer ? "merchant_outcome" : null;
        foreach (var (entry, index) in inventory.CardEntries.Select((entry, index) => (entry, index)))
            if (entry.IsStocked && entry.EnoughGold && entry.CreationResult is { } creation)
                result.Add(Option(creation.Card, new("BuyCard", seat, card: Card(creation.Card),
                    index: multiplayer ? null : index, slot_index: multiplayer ? null : index,
                    price: entry.Cost, sync_kind: sync), DecisionAction.BuyCard));
        foreach (var (entry, index) in inventory.RelicEntries.Select((entry, index) => (entry, index)))
            if (entry.IsStocked && entry.EnoughGold && entry.Model is { } relic)
                result.Add(Option(relic, new("BuyRelic", seat, relic_id: relic.Id.Entry,
                    index: multiplayer ? null : index, slot_index: multiplayer ? null : index,
                    price: entry.Cost, sync_kind: sync), DecisionAction.BuyRelic));
        foreach (var (entry, index) in inventory.PotionEntries.Select((entry, index) => (entry, index)))
            if (entry.IsStocked && entry.EnoughGold && entry.Model is { } potion)
                result.Add(Option(potion, new("BuyPotion", seat, potion_id: potion.Id.Entry,
                    index: multiplayer ? null : index, slot_index: multiplayer ? null : index,
                    price: entry.Cost, sync_kind: sync), DecisionAction.BuyPotion));
        if (inventory.CardRemovalEntry is { IsStocked: true, EnoughGold: true } removal)
            result.Add(Option(removal, new("RemoveCard", seat, price: removal.Cost, sync_kind: sync), DecisionAction.RemoveCard));
        result.Add(Option(Options.Proceed, new("LeaveShop", seat), DecisionAction.Proceed));
        return result;
    }

    public static IReadOnlyList<LiveOption> Rewards(int seat, IReadOnlyList<Reward> rewards, bool canProceed)
    {
        var result = new List<LiveOption>();
        for (int index = 0; index < rewards.Count; index++) {
            var reward = rewards[index];
            if (reward.SuccessfullySelected) continue;
            switch (reward) {
                case MegaCrit.Sts2.Core.Rewards.CardReward:
                    result.Add(Option(reward, new("OpenReward", seat, reward_index: index, reward_type: "card"), DecisionAction.OpenReward));
                    break;
                case RelicReward { Relic: { } relic }:
                    result.Add(Option(relic, new("PickRelicReward", seat, reward_index: index,
                        reward_type: "relic", relic_id: relic.Id.Entry), DecisionAction.PickRelic));
                    break;
                case PotionReward { Potion: { } potion }:
                    result.Add(Option(potion, new("PickPotion", seat, reward_index: index,
                        reward_type: "potion", potion_id: potion.Id.Entry), DecisionAction.PickPotion));
                    break;
                case GoldReward:
                    result.Add(Option(reward, new("Collect", seat, reward_index: index, reward_type: "gold"), DecisionAction.Collect));
                    break;
                default: throw new InvalidOperationException("ORIGINAL_REWARD_KIND_UNSUPPORTED");
            }
        }
        if (canProceed) result.Add(Option(Options.Proceed, new("SkipRewards", seat), DecisionAction.Proceed));
        return result;
    }

    public static IReadOnlyList<LiveOption> Rewards(int seat, RewardsSet set) =>
        Rewards(seat, set.Rewards, !set.DisallowSkipping);

    public static IReadOnlyList<LiveOption> PotionReward(int seat, PotionReward reward, int rewardIndex, bool canSkip)
    {
        var result = Rewards(seat, [reward], false).Select(o => o with {
            Binding = JsonSerializer.Serialize(new RecordedAction("PickPotion", seat, reward_index: rewardIndex,
                reward_type: "potion", potion_id: reward.Potion?.Id.Entry), RecordingFormat.Json)
        }).ToList();
        if (canSkip) result.Add(Option(Options.Skip, new("SkipPotion", seat, reward_index: rewardIndex,
            reward_type: "potion"), DecisionAction.Skip));
        return result;
    }

    public static IReadOnlyList<LiveOption> RelicReward(int seat, RelicReward reward, int rewardIndex, bool canSkip)
    {
        var result = Rewards(seat, [reward], false).Select(o => o with {
            Binding = JsonSerializer.Serialize(new RecordedAction("PickRelicReward", seat, reward_index: rewardIndex,
                reward_type: "relic", relic_id: reward.Relic?.Id.Entry), RecordingFormat.Json)
        }).ToList();
        if (canSkip) result.Add(Option(Options.Skip, new("SkipRelic", seat, reward_index: rewardIndex,
            reward_type: "relic"), DecisionAction.Skip));
        return result;
    }

    public static IReadOnlyList<LiveOption> TreasureVote(int seat, IReadOnlyList<RelicModel> relics,
        int generation, bool canAbstain)
    {
        var result = relics.Select((relic, index) => Option(relic,
            new("PickRelicReward", seat, index: index, relic_id: relic.Id.Entry, reward_type: "relic",
                sync_kind: "treasure_vote", generation: generation), DecisionAction.Vote)).ToList();
        if (canAbstain) result.Add(Option(Options.Skip, new("PickRelicReward", seat, sync_kind: "treasure_vote",
            generation: generation, cancelled: true), DecisionAction.Skip));
        return result;
    }

    public static IReadOnlyList<LiveOption> Treasure(int seat, IReadOnlyList<RelicModel> relics, bool multiplayer)
    {
        var result = relics.Select((relic, index) => Option(relic,
            new(multiplayer ? "PickRelicReward" : "Collect", seat, reward_index: index,
                reward_type: "relic", relic_id: relic.Id.Entry,
                sync_kind: multiplayer ? "treasure_vote" : null), DecisionAction.PickRelic)).ToList();
        result.Add(Option(Options.Skip, multiplayer
            ? new("PickRelicReward", seat, reward_type: "relic", sync_kind: "treasure_vote", cancelled: true)
            : new("SkipRelic", seat, reward_type: "relic"), DecisionAction.Skip));
        return result;
    }
}
