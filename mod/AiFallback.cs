using System.Diagnostics;
using HarmonyLib;
using HumanPlayRecorder.Ai;
using MegaCrit.Sts2.Core.Commands;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.Entities.RestSite;
using MegaCrit.Sts2.Core.GameActions;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Multiplayer.Game;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Game;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Game.Sync;
using MegaCrit.Sts2.Core.Multiplayer.Serialization;
using MegaCrit.Sts2.Core.Rewards;
using MegaCrit.Sts2.Core.Rooms;
using MegaCrit.Sts2.Core.Runs;

namespace HumanPlayRecorder;

public static partial class RecorderMod
{
    internal static Task<byte[]> PreloadAiPolicy(CancellationToken token) => Task.Run(() =>
        PolicyCache.Load(PolicyDirectory, "public-beta+0861bfa1", token), token);
    static readonly AiFallbackDeadline aiFallbackDeadline = new();
    internal const double AiFallbackSeconds = 5;
    // An AI seat whose advice arrived in time acts at least this long before its fallback fires.
    internal const double AiActMarginSeconds = .75;
    static long AiFallbackDelay => (long)(Stopwatch.Frequency * AiFallbackSeconds);
    static readonly Dictionary<Player, Func<PlayerChoiceResult>> aiNativeDefaults = new();
    static readonly Dictionary<Reward, Task<bool>> aiRewardOutcomes = new();
    internal static bool AiRewardFailed(Reward reward) =>
        aiRewardOutcomes.TryGetValue(reward, out var outcome) && outcome.IsCompleted &&
        (!outcome.IsCompletedSuccessfully || !outcome.Result);
    static bool AiHasRewards(Player player) => aiRewardSets.GetValueOrDefault(player, []).Any(set =>
        !RunManager.Instance.RewardsSetSynchronizer.IsRewardsSetCompleted(set));
    static RewardsSet? AiCurrentRewards(Player player) =>
        aiRewardSets.GetValueOrDefault(player, []).LastOrDefault(set =>
            !RunManager.Instance.RewardsSetSynchronizer.IsRewardsSetCompleted(set));
    static bool AiRewardSelecting(RewardsSet set) =>
        set.Rewards.Any(reward => aiRewardInFlight.TryGetValue(reward, out var task) && !task.IsCompleted);
    internal static INetMessage? AiRewardDefault(RewardsSet set, RunLocation location)
    {
        // The native reward selection protocol is polymorphic: no policy DTO or
        // reward-type switch is needed, including for future/custom rewards.
        int index = set.Rewards.FindIndex(reward => !reward.SuccessfullySelected);
        if (index >= 0 && AiRewardFailed(set.Rewards[index])) {
            if (!set.DisallowSkipping)
                return new RewardSetSkippedMessage { setId = set.Id, location = location };
            // A full belt is a recoverable failure. Retry only after the normal
            // discard action has actually changed it, never while still full.
            if (set.Rewards[index] is PotionReward && set.Player.HasOpenPotionSlots)
                return new RewardSelectedMessage { setId = set.Id, rewardIndex = index, location = location };
            // A mandatory non-potion reward has no legal skip. Its retry is not
            // unchanged: nested card choices now take the first card instead of
            // the cancellation that made the earlier attempt return false.
            if (set.Rewards[index] is not PotionReward)
                return new RewardSelectedMessage { setId = set.Id, rewardIndex = index, location = location };
        }
        if (index >= 0 && set.Rewards[index] is PotionReward && !set.Player.HasOpenPotionSlots) {
            if (!set.DisallowSkipping)
                return new RewardSetSkippedMessage { setId = set.Id, location = location };
            int slot = set.Player.PotionSlots.ToList().FindIndex(potion => potion is not null);
            if (slot < 0 || !set.Player.CanUseOrRemovePotions) return null;
            return new RequestEnqueueActionMessage {
                action = new DiscardPotionGameAction(set.Player, (uint)slot,
                    MegaCrit.Sts2.Core.Combat.CombatManager.Instance.IsInProgress).ToNetAction(),
                location = location
            };
        }
        return index < 0 ? null : new RewardSelectedMessage {
            setId = set.Id, rewardIndex = index, location = location
        };
    }
    internal static void AiIsolateFallback<T>(IEnumerable<T> seats, Action<T> act, Action<T, Exception> failed)
    {
        foreach (var seat in seats)
            try { act(seat); }
            catch (Exception error) { failed(seat, error); }
    }

    static void InstallAiFallback(Harmony harmony)
    {
        harmony.Patch(AccessTools.Method(typeof(RelicSelectCmd), nameof(RelicSelectCmd.FromChooseARelicScreen)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(AiRelicDefault)));
        harmony.Patch(AccessTools.Method(typeof(MendRestSiteOption), nameof(MendRestSiteOption.OnSelect)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(AiMendDefault)));
    }
    static void AiRelicDefault(Player player, IReadOnlyList<RelicModel> relics)
    {
        if (AiOwned(player)) aiNativeDefaults[player] = () => PlayerChoiceResult.FromIndex(relics.Count > 0 ? 0 : -1);
    }
    static void AiMendDefault(MendRestSiteOption __instance)
    {
        var player = (Player)AccessTools.Property(typeof(RestSiteOption), "Owner").GetValue(__instance)!;
        if (AiOwned(player)) aiNativeDefaults[player] = () => PlayerChoiceResult.FromPlayerId(
            player.RunState.Players.FirstOrDefault(other => other != player && other.Creature.IsAlive)?.NetId);
    }
    internal static PlayerChoiceResult AiDefaultSelector(CardModel[] cards, string kind,
        MegaCrit.Sts2.Core.CardSelection.CardSelectorPrefs prefs)
    {
        var selected = cards.Take(prefs.Cancelable ? 0 : Math.Max(0, prefs.MinSelect)).ToArray();
        return kind switch {
            "index" => PlayerChoiceResult.FromIndex(selected.Length == 0 ? -1 : 0),
            "indexes" => PlayerChoiceResult.FromIndexes(Enumerable.Range(0, selected.Length).ToList()),
            "deck" => PlayerChoiceResult.FromMutableDeckCards(selected),
            _ => PlayerChoiceResult.FromMutableCombatCards(selected)
        };
    }
    static void AiFallbackFrame()
    {
        if (!MultiplayerAi.HostsAi || run is null) return;
        AiIsolateFallback(run.Players.Where(player => MultiplayerAi.IsAi(player.NetId)), player => {
            object? decision = null;
            Action? execute = null;
            if (aiWaitingChoices.TryGetValue(player, out uint choice)) {
                PlayerChoiceResult? result = null;
                if (currentCardRewards.TryGetValue(player, out var reward))
                    result = PlayerChoiceResult.FromIndex(0);
                else if (aiNativeDefaults.TryGetValue(player, out var native)) result = native();
                if (result is not null) {
                    AdvisorTrace($"AI choice fallback pending seat={run.GetPlayerSlotIndex(player)} choice={choice}");
                    decision = ("choice", choice);
                    execute = () => AiSubmit(player, new PlayerChoiceMessage { choiceId = choice, result = result.ToNetData() });
                }
                else AdvisorTrace($"AI choice wait seat={run.GetPlayerSlotIndex(player)} choice={choice} has no default");
            }
            else if (AiCurrentRewards(player) is { } set) {
                var message = AiRewardSelecting(set) || RunManager.Instance.ActionExecutor.IsRunning ? null :
                    AiRewardDefault(set, RunManager.Instance.RunLocationTargetedBuffer.CurrentLocation);
                if (message is not null) {
                    decision = (set, message.GetType(), message is RewardSelectedMessage select ? select.rewardIndex : -1);
                    execute = () => AiSubmit(player, message);
                }
            }
            else if (RunManager.Instance.ActionExecutor.IsRunning && run.CurrentRoom is not CombatRoom ||
                aiBusy.TryGetValue(player, out var busy) && !busy.IsCompleted) { }
            else if (run.CurrentRoom is CombatRoom { CombatState.CurrentSide: MegaCrit.Sts2.Core.Combat.CombatSide.Player } &&
                player.PlayerCombatState is { } combat && combat.Phase.ToString() == "Play" &&
                !MegaCrit.Sts2.Core.Combat.CombatManager.Instance.IsPlayerReadyToEndTurn(player) &&
                !AiHasRewards(player)) {
                // Every play moves a card or spends energy, so a turn the AI is still
                // playing keeps restarting the clock; a stalled turn does not.
                decision = (run.CurrentRoom, combat.TurnNumber, combat.Energy, combat.Hand.Cards.Count,
                    combat.DrawPile.Cards.Count, combat.DiscardPile.Cards.Count, combat.ExhaustPile.Cards.Count,
                    player.Potions.Count());
                execute = () => AiEnqueue(player, new EndPlayerTurnAction(player, combat.TurnNumber));
            }
            else {
                var options = AiOptions(player);
                var option = options.FirstOrDefault(option => option.Legal &&
                    option.Action is AiAdvisor.DecisionAction.Proceed or AiAdvisor.DecisionAction.Confirm)
                    ?? options.FirstOrDefault(option => option.Legal);
                if (option is not null) {
                    decision = (run.CurrentRoom, string.Join("\n", options.Select(candidate => candidate.Binding)));
                    execute = () => AiExecute(player, option);
                }
                else if (run.CurrentRoom is RestSiteRoom && AiRestSiteOpen(player)) {
                    // Only disabled options remain (nothing to upgrade, a relic that keeps
                    // the rest site open): a human leaves, which sends the native skip.
                    decision = (run.CurrentRoom, "rest-skip");
                    execute = () => AiSubmit(player, new RestSiteSkippedMessage {
                        Location = RunManager.Instance.RunLocationTargetedBuffer.CurrentLocation });
                }
            }
            if (!aiFallbackDeadline.Due(player, decision, Stopwatch.GetTimestamp(), AiFallbackDelay) ||
                execute is null) return;
            execute();
            AiClearPresence(player);
            Godot.GD.Print($"[HumanPlayRecorder] AI fallback seat={run.GetPlayerSlotIndex(player)} reason=policy-unavailable-or-unsupported wait=5s");
            aiFallbackDeadline.Progress(player);
            AiAdvisor.InvalidateDecisions();
            advisorCaptured.Clear();
            MarkAdvisorDirty();
        }, (player, error) => Godot.GD.Print($"[HumanPlayRecorder] AI fallback retry seat={run.GetPlayerSlotIndex(player)} error={error.GetType().Name}"));
    }
    /// <summary>True while the game still waits for this seat's rest site to complete.</summary>
    internal static bool AiRestSiteOpen(Player player)
    {
        var sites = (System.Collections.IList)AccessTools.Field(typeof(RestSiteSynchronizer), "_restSites")
            .GetValue(RunManager.Instance.RestSiteSynchronizer)!;
        int seat = run?.GetPlayerSlotIndex(player) ?? -1;
        if (seat < 0 || seat >= sites.Count) return false;
        var site = sites[seat]!;
        var done = (TaskCompletionSource)AccessTools.Field(site.GetType(), "completionTaskSource").GetValue(site)!;
        return !done.Task.IsCompleted;
    }
    internal static bool AiFallbackPending(Player player, uint choice, long now, PlayerChoiceResult result,
        Action<uint, PlayerChoiceResult> submit)
    {
        if (!aiFallbackDeadline.Due(player, ("choice", choice), now, AiFallbackDelay)) return false;
        submit(choice, result);
        aiFallbackDeadline.Progress(player);
        return true;
    }
}
