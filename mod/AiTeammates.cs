using System.Reflection;
using System.Text.Json;
using HarmonyLib;
using HumanPlayRecorder.Ai;
using MegaCrit.Sts2.Core.CardSelection;
using MegaCrit.Sts2.Core.Combat;
using MegaCrit.Sts2.Core.Commands;
using MegaCrit.Sts2.Core.Entities.Cards;
using MegaCrit.Sts2.Core.Entities.Creatures;
using MegaCrit.Sts2.Core.Entities.Merchant;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.GameActions;
using MegaCrit.Sts2.Core.GameActions.Multiplayer;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Multiplayer;
using MegaCrit.Sts2.Core.Multiplayer.Game;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Game;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Game.Sync;
using MegaCrit.Sts2.Core.Multiplayer.Serialization;
using MegaCrit.Sts2.Core.Multiplayer.Transport;
using MegaCrit.Sts2.Core.Rewards;
using MegaCrit.Sts2.Core.Rooms;
using MegaCrit.Sts2.Core.Runs;

namespace HumanPlayRecorder;

public static partial class RecorderMod
{
    sealed record AiSelectorContext(string Kind, CardSelectorPrefs Prefs);
    sealed record AiSelector(CardModel[] Cards, AiSelectorContext Context)
    {
        public List<CardModel> Selected { get; } = new();
    }
    sealed record AiPublication(AiSeatDecisionLoop Loop, long Generation, bool Live);
    internal sealed record AiReadyDecision(Player Player, AiAdvisor.LiveOption Option,
        RunState Run, AbstractRoom? Room, long Fingerprint, bool Live);
    static readonly Dictionary<Player, AiSelectorContext> aiSelectorContexts = new();
    static readonly Dictionary<Player, AiSelector> aiSelectors = new();
    static readonly Dictionary<Player, uint> aiWaitingChoices = new();
    static readonly Dictionary<Player, List<RewardsSet>> aiRewardSets = new();
    static readonly Dictionary<Player, Task> aiBusy = new();
    static readonly Dictionary<Reward, Task> aiRewardInFlight = new();
    static readonly Dictionary<int, AiPublication> aiPublications = new();
    static readonly HashSet<(Player Player, AbstractRoom Room, string Kind)> aiRoomInputs = new();
    static readonly AsyncLocal<Player?> aiPurchasePlayer = new();
    static readonly HashSet<(ulong Player, int Act)> aiActVotes = new();
    static int? aiPendingAct;

    internal static void InstallAiTeammates(Harmony harmony)
    {
        InstallAiFallback(harmony);
        InstallAiOwnerRules(harmony);
        InstallAiCrystalSphere(harmony);
        InstallAiPresence();
        foreach (var method in typeof(CardSelectCmd).GetMethods(BindingFlags.Public | BindingFlags.Static)
            .Where(method => method.Name.StartsWith("From", StringComparison.Ordinal)))
            harmony.Patch(method, prefix: new HarmonyMethod(typeof(RecorderMod), nameof(AiSelectorPrefs)) { priority = Priority.First });
        harmony.Patch(AccessTools.Method(typeof(PlayerChoiceSynchronizer), nameof(PlayerChoiceSynchronizer.WaitForRemoteChoice)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AiWaitForChoice)));
        harmony.Patch(AccessTools.Method(typeof(RewardsSetSynchronizer), nameof(RewardsSetSynchronizer.BeginRewardsSet)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AiRewardsSet)));
        harmony.Patch(AccessTools.Method(typeof(Reward), nameof(Reward.SelectUnsynchronized)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AiRewardBusy)));
        harmony.Patch(AccessTools.Method(typeof(RestSiteSynchronizer), "ChooseOption"),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AiRestBusy)));
        harmony.Patch(AccessTools.Method(typeof(EventSynchronizer), "ChooseOptionForEvent"),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AiEventBusy)));
        harmony.Patch(AccessTools.Method(typeof(ActionQueueSynchronizer), nameof(ActionQueueSynchronizer.RequestEnqueue)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AiProtocolAction)));
        harmony.Patch(AccessTools.Method(typeof(GameActionPlayerChoiceContext), nameof(GameActionPlayerChoiceContext.SignalPlayerChoiceEnded)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(AiResumeContext)));
        harmony.Patch(AccessTools.Method(typeof(HookPlayerChoiceContext), nameof(HookPlayerChoiceContext.SignalPlayerChoiceBegun)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(AiHookContext)));
        // SetUpSavedMultiplayer is async: a postfix on it runs at its first await, before
        // InitializeShared creates the message buffer. Every multiplayer setup passes
        // InitializeRunLobby synchronously after that buffer exists.
        harmony.Patch(AccessTools.Method(typeof(RunManager), "InitializeRunLobby"),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AiSetUp)));
        harmony.Patch(AccessTools.Method(typeof(RunManager), nameof(RunManager.CleanUp)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(ClearAiTeammates)));
        foreach (var method in typeof(RewardSynchronizer).GetMethods().Where(method => method.Name.StartsWith("SyncLocal", StringComparison.Ordinal)))
            harmony.Patch(method, prefix: new HarmonyMethod(typeof(RecorderMod), nameof(AiPurchaseSync)));
        var hostSend = AccessTools.FirstMethod(typeof(NetHostGameService), m =>
            m.Name == nameof(NetHostGameService.SendMessage) && m.IsGenericMethodDefinition && m.GetParameters().Length == 1);
        harmony.Patch(hostSend.MakeGenericMethod(typeof(RewardObtainedMessage)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(AiPurchaseReward)));
        harmony.Patch(hostSend.MakeGenericMethod(typeof(GoldLostMessage)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(AiPurchaseGold)));
        harmony.Patch(AccessTools.Method(typeof(OneOffSynchronizer), nameof(OneOffSynchronizer.DoLocalMerchantCardRemoval)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(AiMerchantRemoval)));
        harmony.Patch(AccessTools.Method(typeof(OneOffSynchronizer), nameof(OneOffSynchronizer.DoLocalTreasureRoomRewards)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AiTreasureOpened)));
        harmony.Patch(AccessTools.Method(typeof(CombatStateSynchronizer), nameof(CombatStateSynchronizer.StartSync)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AiCombatStateSync)));
    }

    // Every peer's StartSync sends its own player; the run waits for all lobby
    // IDs. A virtual seat has no game of its own, so the host sends the AI
    // player's state through the native peer path when the sync begins.
    static void AiCombatStateSync(INetGameService ____netService, RunState ____runState,
        TaskCompletionSource? ____syncCompletionSource)
    {
        if (____syncCompletionSource is null || ____netService.Type != NetGameType.Host) return;
        foreach (var player in ____runState.Players.Where(p => MultiplayerAi.IsAi(p.NetId)).ToArray())
            MultiplayerAi.Submit(____netService, player.NetId, new SyncPlayerDataMessage { player = player.ToSerializable() });
    }

    static bool AiOwned(Player player) => MultiplayerAi.HostsAi && MultiplayerAi.IsAi(player.NetId);
    static void AiHookContext(HookPlayerChoiceContext __instance, ref ulong ____localPlayerId)
    {
        // This context's controller lives on the host. Only its routing identity
        // changes; LocalContext and the player's game state remain untouched.
        if (__instance.Owner is { } player && AiOwned(player)) ____localPlayerId = player.NetId;
    }
    static void AiResumeContext(GameActionPlayerChoiceContext __instance)
    {
        if (MultiplayerAi.HostsAi && MultiplayerAi.IsAi(__instance.Action.OwnerId) &&
            !MegaCrit.Sts2.Core.TestSupport.TestFlags.ShouldSendResumeForRemotePlayers)
            RunManager.Instance.ActionQueueSynchronizer.RequestResumeActionAfterPlayerChoice(__instance.Action);
    }
    static void AiSetUp(INetGameService netService)
    {
        if (!netService.Type.IsMultiplayer()) return;
        ClearAiTeammates();
        RunManager.Instance.RunLocationTargetedBuffer.RegisterMessageHandler<AiShopPurchaseMessage>(AiPurchaseMessage);
    }
    static void ClearAiTeammates()
    {
        ClearAiPresence();
        aiFallbackDeadline.Clear();
        aiNativeDefaults.Clear();
        foreach (var publication in aiPublications.Values) publication.Loop.Invalidate();
        aiPublications.Clear();
        aiSelectorContexts.Clear();
        aiSelectors.Clear();
        aiWaitingChoices.Clear();
        aiRewardSets.Clear();
        aiBusy.Clear();
        aiRewardInFlight.Clear();
        aiRewardOutcomes.Clear();
        aiRoomInputs.Clear();
        aiActVotes.Clear();
        aiPendingAct = null;
    }

    static void AiSelectorPrefs(MethodBase __originalMethod, object[] __args)
    {
        var parameters = __originalMethod.GetParameters();
        Player? player = __args.OfType<Player>().FirstOrDefault() ??
            __args.OfType<IReadOnlyList<CardModel>>().FirstOrDefault()?.FirstOrDefault()?.Owner;
        if (player is null || !AiOwned(player)) return;
        CardSelectorPrefs? prefs = __args.OfType<CardSelectorPrefs>().Cast<CardSelectorPrefs?>().FirstOrDefault();
        if (prefs is null) {
            bool canSkip = Array.FindIndex(parameters, parameter => parameter.Name == "canSkip") is int at &&
                at >= 0 && __args[at] is true;
            prefs = AiSingleScreenPrefs(canSkip);
        }
        string name = __originalMethod.Name;
        string kind = name == nameof(CardSelectCmd.FromChooseACardScreen) ? "index" :
            name.Contains("SimpleGrid", StringComparison.Ordinal) ? "indexes" :
            name.Contains("Deck", StringComparison.Ordinal) ? "deck" : "combat";
        aiSelectorContexts[player] = new(kind, prefs.Value);
        if (__args.OfType<IReadOnlyList<CardModel>>().FirstOrDefault() is { } offered) {
            var candidates = offered.ToArray();
            var bounds = prefs.Value;
            aiNativeDefaults[player] = () => AiDefaultSelector(candidates, kind, bounds);
        }
        if (__args.OfType<IReadOnlyList<IReadOnlyList<CardModel>>>().FirstOrDefault() is { } bundles)
            aiNativeDefaults[player] = () => PlayerChoiceResult.FromIndex(bundles.Count > 0 ? 0 : -1);
        var context = __args.OfType<PlayerChoiceContext>().FirstOrDefault();
        var source = __args.OfType<AbstractModel>().FirstOrDefault() ?? context?.LastInvolvedModel;
        var pile = __args.OfType<CardPile>().FirstOrDefault();
        var enchantment = __args.OfType<EnchantmentModel>().FirstOrDefault();
        int amount = Array.FindIndex(parameters, parameter => parameter.Name == "amount") is int index && index >= 0
            ? (int)__args[index] : 0;
        LiveObservation.RememberSelection(player, prefs.Value, pile?.Type.ToString() ??
            (kind == "deck" ? "Deck" : "Hand"), source, enchantment, amount);
    }
    // The native choose-a-card screen takes a card on click. The (min 0, max 1)
    // constructor would demand a confirm the observation never offers
    // (RememberSingleSelection), so a skippable pick (Lead Paperweight) toggled forever.
    internal static CardSelectorPrefs AiSingleScreenPrefs(bool canSkip) =>
        new(CardSelectorPrefs.UpgradeSelectionPrompt, canSkip ? 0 : 1, 1) { RequireManualConfirmation = false };
    /// <summary>Toggles one card of an AI selection; true when the selection is complete and is submitted now.</summary>
    internal static bool AiToggleSelection(List<CardModel> selected, CardModel card, CardSelectorPrefs prefs)
    {
        if (!selected.Remove(card)) selected.Add(card);
        return !prefs.RequireManualConfirmation && selected.Count >= prefs.MaxSelect;
    }
    static void AiOfferSelection(Player player, CardModel[] candidates)
    {
        if (!AiOwned(player) || !aiSelectorContexts.TryGetValue(player, out var context)) return;
        aiSelectors[player] = new(candidates, context);
        aiNativeDefaults[player] = () => AiDefaultSelector(candidates, context.Kind, context.Prefs);
    }
    static void AiWaitForChoice(Player player, uint choiceId, ref Task<PlayerChoiceResult> __result)
    {
        if (!AiOwned(player)) return;
        Godot.GD.Print($"[HumanPlayRecorder] AI choice wait seat={run?.GetPlayerSlotIndex(player)} choice={choiceId} " +
            $"default={aiNativeDefaults.ContainsKey(player)}");
        aiWaitingChoices[player] = choiceId;
        __result = AiAwaitChoice(__result, player, choiceId);
        MarkAdvisorDirty();
    }
    static async Task<PlayerChoiceResult> AiAwaitChoice(Task<PlayerChoiceResult> task, Player player, uint choiceId)
    {
        try { return await task; }
        catch (Exception error) {
            Godot.GD.Print($"[HumanPlayRecorder] AI choice wait failed choice={choiceId} error={error.GetType().Name}");
            throw;
        }
        finally {
            if (aiWaitingChoices.GetValueOrDefault(player, uint.MaxValue) == choiceId) {
                aiWaitingChoices.Remove(player);
                aiNativeDefaults.Remove(player);
                aiFallbackDeadline.Progress(player);
                aiSelectors.Remove(player);
                LiveObservation.ForgetSelection(player);
                MarkAdvisorDirty();
            }
        }
    }
    static void AiRewardsSet(RewardsSet set)
    {
        if (!AiOwned(set.Player)) return;
        if (!aiRewardSets.TryGetValue(set.Player, out var stack)) aiRewardSets[set.Player] = stack = new();
        stack.Add(set);
        MarkAdvisorDirty();
    }
    static void AiRewardBusy(Reward __instance, Task<bool> __result)
    {
        if (MultiplayerAi.IsAi(__instance.Player.NetId)) {
            aiRewardOutcomes[__instance] = __result;
            var tracked = AiTrack(__result);
            aiRewardInFlight[__instance] = tracked;
            aiBusy[__instance.Player] = tracked;
        }
    }
    static void AiRestBusy(Player player, Task<bool> __result)
    {
        if (AiOwned(player)) aiBusy[player] = AiTrack(__result);
    }
    static void AiEventBusy(Player player, List<Task> ____pendingOptionTasks)
    {
        if (AiOwned(player) && ____pendingOptionTasks.LastOrDefault() is { } task)
            aiBusy[player] = AiTrack(task);
    }
    static async Task AiTrack(Task task)
    {
        try { await task; }
        catch (OperationCanceledException) { }
        catch (Exception error) { Godot.GD.Print("[HumanPlayRecorder] AI action failed: " + error); }
        finally { MarkAdvisorDirty(); }
    }

    static IReadOnlyList<AiAdvisor.LiveOption> AiOptions(Player player)
    {
        int seat = run!.GetPlayerSlotIndex(player);
        var manager = RunManager.Instance;
        if (aiWaitingChoices.ContainsKey(player)) {
            if (currentCardRewards.TryGetValue(player, out var reward))
                return GameOptionAdapters.CardReward(seat, reward, rewardIndices[reward])
                    .Where(option => AiCurrentRewards(player)?.DisallowSkipping != true ||
                        option.Action != AiAdvisor.DecisionAction.Skip).ToArray();
            if (aiSelectors.TryGetValue(player, out var selector))
                return LiveObservation.SelectionOptions(seat, player, selector.Cards);
            if (advisorSelectors.TryGetValue(player, out var bundle)) return bundle.Options();
            throw new AdviceUnavailableException(AdviceReason.StateNotExposed, "AI_REMOTE_CHOICE_DOMAIN_UNAVAILABLE");
        }
        if (aiRewardSets.TryGetValue(player, out var sets)) {
            sets.RemoveAll(set => manager.RewardsSetSynchronizer.IsRewardsSetCompleted(set));
            if (sets.LastOrDefault() is { } set)
                return AiRewardSelecting(set) ? [] : GameOptionAdapters.Rewards(seat, set);
        }
        if (aiBusy.TryGetValue(player, out var busy) && !busy.IsCompleted ||
            manager.NetService.IsGameLoading) return [];
        // The executor animates one queued action at a time; it is not an input
        // lock. Like a real peer, another combat seat can already queue its input.
        if (manager.ActionExecutor.IsRunning && run.CurrentRoom is not CombatRoom) return [];
        if (MegaCrit.Sts2.Core.Nodes.Screens.Map.NMapScreen.Instance is
            { IsOpen: true, IsTravelEnabled: true, IsTraveling: false } || AiRoomFinished(player)) {
            if (manager.MapSelectionSynchronizer.GetVote(player).HasValue) return [];
            return GameOptionAdapters.Map(run, player);
        }
        if (run.CurrentRoom is CombatRoom combat)
            return GameOptionAdapters.Combat(run, player, combat.CombatState);
        if (run.CurrentRoom is EventRoom) {
            var model = manager.EventSynchronizer.GetEventForPlayer(player);
            if (model.IsFinished || manager.EventSynchronizer.GetPlayerVote(player).HasValue) return [];
            return GameOptionAdapters.Events(seat, model.CurrentOptions);
        }
        if (run.CurrentRoom is RestSiteRoom)
            return GameOptionAdapters.Rest(seat, manager.RestSiteSynchronizer.GetOptionsForPlayer(player));
        if (run.CurrentRoom is TreasureRoom && manager.TreasureRoomRelicSynchronizer.CurrentRelics is { } relics) {
            if (aiRoomInputs.Contains((player, run.CurrentRoom, "treasure_vote"))) return [];
            return GameOptionAdapters.Treasure(seat, relics, true);
        }
        if (run.CurrentRoom is MerchantRoom merchant) {
            if (aiRoomInputs.Contains((player, merchant, "shop_done"))) return [];
            return GameOptionAdapters.Shop(run, player, merchant);
        }
        return [];
    }

    // A real peer opens the map and votes as soon as its own part of the room is done,
    // while others still read rewards; the game accepts votes from the room's entry on.
    static bool AiRoomFinished(Player player)
    {
        if (run!.Players.Count < 2 || run.CurrentRoom is not { } room || run.Map is MegaCrit.Sts2.Core.Map.NullActMap ||
            run.CurrentMapPoint?.PointType == MegaCrit.Sts2.Core.Map.MapPointType.Boss) return false;
        var manager = RunManager.Instance;
        return room switch {
            // Rewards are offered a moment after the win; until then the room is not done.
            CombatRoom combat => !CombatManager.Instance.IsInProgress &&
                aiCombatWon is { } won && ReferenceEquals(won.Room, combat) &&
                System.Diagnostics.Stopwatch.GetTimestamp() - won.At > 2 * System.Diagnostics.Stopwatch.Frequency,
            EventRoom => manager.EventSynchronizer.GetEventForPlayer(player).IsFinished,
            RestSiteRoom => manager.RestSiteSynchronizer.GetChosenOptionIndex(player.NetId).HasValue,
            TreasureRoom => manager.TreasureRoomRelicSynchronizer.GetPlayerVote(player).voteReceived,
            MerchantRoom merchant => aiRoomInputs.Contains((player, merchant, "shop_done")),
            _ => false,
        };
    }

    static int? AiParent(Player player)
    {
        if (advisorSelectors.TryGetValue(player, out var selector) &&
            pendingInputs.TryGetValue(selector.Sequence, out var pending)) return pending.ParentSequence;
        int seat = run!.GetPlayerSlotIndex(player);
        return pendingInputs.LastOrDefault(pair => pair.Value.player == seat &&
            pair.Value.kind == "OpenReward").Value?.InputSequence;
    }
    static long AiReplayFingerprint(ReplayCheckpoint checkpoint, IReadOnlyList<AiAdvisor.LiveOption> options)
    {
        byte[] json = JsonSerializer.SerializeToUtf8Bytes(new {
            checkpoint, options = options.Select(option => option.Binding)
        }, RecordingFormat.Json);
        return System.Buffers.Binary.BinaryPrimitives.ReadInt64LittleEndian(
            System.Security.Cryptography.SHA256.HashData(json));
    }
    static void AiPublished(int seat, long fingerprint, IReadOnlyList<AiAdvisor.LiveOption> options, bool live = true)
    {
        if (run is null || !AiOwned(run.Players[seat])) return;
        var loop = aiPublications.TryGetValue(seat, out var publication) ? publication.Loop : new AiSeatDecisionLoop();
        aiPublications[seat] = new(loop, loop.Publish(fingerprint, options), live);
    }
    static bool AiTryPrepare(Player player, IReadOnlyList<AiAdvisor.LiveOption> current,
        List<AiReadyDecision> ready)
    {
        int seat = run!.GetPlayerSlotIndex(player);
        // A searched decision waits for its search, at most AiAdvisor.SearchWaitSeconds after capture
        // and never past the seat's fallback deadline (AiAdvisor.SeatDeadline).
        if (!aiPublications.TryGetValue(seat, out var publication) ||
            AiAdvisor.AdviceForSeat(seat) is not { Count: > 0 } advice || AiAdvisor.AwaitingSearch(seat)) {
            AiSuspendPresence(player);
            return false;
        }
        long signature = AiDecisionFingerprint(run, seat, current, publication.Live);
        string? searched = AiAdvisor.SearchChoiceForSeat(seat);
        if (!publication.Loop.TryPeek(publication.Generation, signature, current, advice, out var chosen, searched, AiReopensClosedCardReward)) {
            // A teammate's action changes the shared snapshot. Await fresh advice
            // without restarting this seat's clock if the policy keeps its action.
            AiSuspendPresence(player);
            return false;
        }
        if (!AiThink(player, chosen!, current, advice)) return true;
        if (!publication.Loop.TryTake(publication.Generation, signature, current, advice, out chosen, searched, AiReopensClosedCardReward)) return false;
        // Revoke this seat's result before entering code which may synchronously
        // enqueue/complete an action or request a nested choice. Other seats keep
        // theirs: each is re-projected and must match the live state (TryTake).
        AiAdvisor.InvalidateSeat(seat);
        advisorCaptured.Remove(seat);
        publication.Loop.Invalidate();
        MarkAdvisorDirty();
        // Prepare the whole frame before the first enqueue can mutate a card's
        // queued flag or synchronously enter an effect.
        ready.Add(new(player, chosen!, run, run.CurrentRoom, signature, publication.Live));
        return true;
    }
    // Reward-reopen guard: a card reward this AI closed without taking a card (C#
    // CardReward.OnSelect returned false; NRewardButton re-enables it) is not opened again
    // on the same rewards screen, or a greedy teammate would open and skip it forever.
    static bool AiReopensClosedCardReward(AiAdvisor.LiveOption option) =>
        option.Action == AiAdvisor.DecisionAction.OpenReward &&
        option.Option is MegaCrit.Sts2.Core.Rewards.CardReward reward && AiRewardFailed(reward);
    static long AiDecisionFingerprint(RunState state, int seat, IReadOnlyList<AiAdvisor.LiveOption> current, bool live)
    {
        if (!live) return AiReplayFingerprint(ReplayCheckpoints.Capture(state, rewardIndices.Keys), current);
        // Dispatch runs after capture's scope closes; live previews still require
        // a bounded projection. A surrounding capture budget remains authoritative.
        using var budget = new FrameCaptureBudget(System.Diagnostics.Stopwatch.GetTimestamp,
            (long)(System.Diagnostics.Stopwatch.Frequency * AiCaptureMilliseconds / 1000));
        return LiveObservation.Signature(state, seat, current);
    }

    internal static void AiDispatchReady(IReadOnlyList<AiReadyDecision> ready)
    {
        AiIsolateFallback(ready, entry => {
            if (run is not { } activeRun || !ReferenceEquals(activeRun, entry.Run) ||
                !ReferenceEquals(activeRun.CurrentRoom, entry.Room)) return;
            AiClearPresence(entry.Player);
            var current = AiOptions(entry.Player);
            if (!current.Any(option => option.Legal &&
                option.Binding == entry.Option.Binding && option.Action == entry.Option.Action &&
                ReferenceEquals(option.Option, entry.Option.Option) &&
                ReferenceEquals(option.Target, entry.Option.Target))) return;
            // Legality alone is insufficient: an earlier enqueue can synchronously
            // change shared gameplay while this exact action remains legal.
            if (AiDecisionFingerprint(entry.Run, entry.Run.GetPlayerSlotIndex(entry.Player), current, entry.Live) !=
                entry.Fingerprint) {
                MarkAdvisorDirty();
                return;
            }
            AiExecute(entry.Player, entry.Option);
        }, (entry, error) => AdvisorTrace($"AI dispatch seat={run!.GetPlayerSlotIndex(entry.Player)} error={error.GetType().Name}"));
    }
    static void AiSubmit(Player player, MegaCrit.Sts2.Core.Multiplayer.Serialization.INetMessage message) =>
        MultiplayerAi.Submit(RunManager.Instance.NetService, player.NetId, message);
    static void AiEnqueue(Player player, GameAction action)
    {
        var queue = RunManager.Instance.ActionQueueSet;
        GameAction? enqueued = null;
        void Capture(GameAction queued) { if (queued.OwnerId == player.NetId) enqueued ??= queued; }
        queue.ActionEnqueued += Capture;
        try {
            AiSubmit(player, new RequestEnqueueActionMessage {
                action = action.ToNetAction(), location = RunManager.Instance.RunLocationTargetedBuffer.CurrentLocation
            });
        }
        finally { queue.ActionEnqueued -= Capture; }
        // A paused player queue holds the action without changing any state, so
        // without this the seat would resubmit the same decision every frame.
        if (enqueued is not null) aiBusy[player] = AiTrack(enqueued.CompletionTask);
    }

    // The 5 s fallback clock is never reset here. It runs on what the game waits
    // for (AiFallbackFrame's decision identity), so only the game moving on
    // restarts it; an action the game ignores or a select/deselect loop cannot.
    static void AiExecute(Player player, AiAdvisor.LiveOption option)
    {
        var input = option.Input;
        var manager = RunManager.Instance;
        var location = manager.RunLocationTargetedBuffer.CurrentLocation;
        if (aiWaitingChoices.TryGetValue(player, out uint choiceId)) {
            PlayerChoiceResult result;
            if (currentCardRewards.TryGetValue(player, out var cardReward)) {
                if (input.kind == "Skip" && (!cardReward.CanSkip || AiCurrentRewards(player)?.DisallowSkipping == true))
                    throw new InvalidOperationException("AI cannot skip a mandatory card reward");
                int? index = input.index;
                if (input.kind == "RerollCardReward") {
                    var alternatives = MegaCrit.Sts2.Core.Entities.CardRewardAlternatives.CardRewardAlternative.Generate(cardReward);
                    int reroll = alternatives.ToList().FindIndex(alternative => alternative.OptionId == "REROLL");
                    if (reroll < 0) throw new InvalidOperationException("AI reward reroll is no longer available");
                    index = cardReward.Cards.Count() + reroll;
                }
                result = PlayerChoiceResult.FromIndex(input.kind == "Skip" ? null : index);
            }
            else if (input.kind == "PickCardBundle") result = PlayerChoiceResult.FromIndex(input.index);
            else if (aiSelectors.TryGetValue(player, out var selector)) {
                if (ReferenceEquals(option.Option, AiAdvisor.Options.Skip)) selector.Selected.Clear();
                if (option.Action != AiAdvisor.DecisionAction.Confirm && option.Option is CardModel card) {
                    bool complete = AiToggleSelection(selector.Selected, card, selector.Context.Prefs);
                    LiveObservation.RememberSelectedCards(player, selector.Selected);
                    if (!complete) return;
                }
                result = selector.Context.Kind switch {
                    "index" => PlayerChoiceResult.FromIndex(selector.Selected.Count == 0 ? -1 :
                        Array.IndexOf(selector.Cards, selector.Selected[0])),
                    "indexes" => PlayerChoiceResult.FromIndexes(selector.Selected.Select(card => Array.IndexOf(selector.Cards, card)).ToList()),
                    "deck" => PlayerChoiceResult.FromMutableDeckCards(selector.Selected),
                    _ => PlayerChoiceResult.FromMutableCombatCards(selector.Selected)
                };
            }
            else throw new InvalidOperationException("AI choice has no captured domain");
            AiSubmit(player, new PlayerChoiceMessage { choiceId = choiceId, result = result.ToNetData() });
            return;
        }
        switch (input.kind) {
            case "PlayCard":
                AiEnqueue(player, new PlayCardAction((CardModel)option.Option, (Creature?)option.Target)); break;
            case "UsePotion":
                AiEnqueue(player, new UsePotionAction((PotionModel)option.Option, (Creature?)option.Target, CombatManager.Instance.IsInProgress)); break;
            case "DiscardPotion":
                AiEnqueue(player, new DiscardPotionGameAction(player, (uint)input.slot_index!.Value,
                    CombatManager.Instance.IsInProgress)); break;
            case "EndTurn":
                AiEnqueue(player, new EndPlayerTurnAction(player, player.PlayerCombatState!.TurnNumber)); break;
            case "Move":
                AiEnqueue(player, new VoteForMapCoordAction(player, run!.MapLocation,
                    new MapVote { coord = ((MegaCrit.Sts2.Core.Map.MapPoint)option.Option).coord,
                        mapGenerationCount = manager.MapSelectionSynchronizer.MapGenerationCount })); break;
            case "EventChoice":
                if (option.Option is MegaCrit.Sts2.Core.Events.EventOption {
                    TextKey: "TRIAL.pages.REJECT.options.DOUBLE_DOWN" }) {
                    Godot.GD.Print("[HumanPlayRecorder] AI fallback reason=human-abandon-popup default=Trial.Accept");
                    input = input with { index = 0 }; // Native Accept path; an AI cannot confirm abandoning the human run.
                }
                if (manager.EventSynchronizer.IsShared)
                    AiSubmit(player, new VotedForSharedEventOptionMessage { optionIndex = (uint)input.index!.Value,
                        pageIndex = (uint)AccessTools.Field(typeof(EventSynchronizer), "_pageIndex").GetValue(manager.EventSynchronizer)!,
                        location = location });
                else AiSubmit(player, new OptionIndexChosenMessage { type = OptionIndexType.Event,
                    optionIndex = (uint)input.index!.Value, location = location });
                break;
            case "RestOption":
                AiSubmit(player, new OptionIndexChosenMessage { type = OptionIndexType.RestSite,
                    optionIndex = (uint)input.index!.Value, location = location }); break;
            case "OpenReward": case "Collect": case "PickPotion": case "PickRelicReward":
                if (run!.CurrentRoom is TreasureRoom && input.kind == "PickRelicReward") {
                    aiRoomInputs.Add((player, run.CurrentRoom, "treasure_vote"));
                    AiEnqueue(player, new PickRelicAction(player, input.reward_index));
                }
                else {
                    var set = aiRewardSets[player].Last();
                    AiSubmit(player, new RewardSelectedMessage { setId = set.Id,
                        rewardIndex = input.reward_index!.Value, location = location });
                }
                break;
            case "SkipRewards":
                if (aiRewardSets[player].Last().DisallowSkipping)
                    throw new InvalidOperationException("AI cannot skip mandatory rewards");
                AiSubmit(player, new RewardSetSkippedMessage { setId = aiRewardSets[player].Last().Id, location = location }); break;
            case "BuyCard": case "BuyRelic": case "BuyPotion": case "RemoveCard":
                AiBuy(player, option); break;
            case "LeaveShop":
                aiRoomInputs.Add((player, run!.CurrentRoom!, "shop_done")); break;
            default: throw new InvalidOperationException("AI_INPUT_UNSUPPORTED: " + input.kind);
        }
    }

    static void AiProtocolAction(GameAction action)
    {
        if (!MultiplayerAi.HostsAi || MultiplayerAi.IsAi(action.OwnerId) || run is null) return;
        // These are protocol acknowledgements, not policy choices.
        if (action is ReadyToBeginEnemyTurnAction)
            foreach (var player in run.Players.Where(p => MultiplayerAi.IsAi(p.NetId)))
                AiEnqueue(player, new ReadyToBeginEnemyTurnAction(player));
        if (action is VoteToMoveToNextActAction vote) aiPendingAct = vote.CurrentActIndex;
    }
    static void AiTreasureOpened()
    {
        if (!MultiplayerAi.HostsAi || run?.CurrentRoom is not TreasureRoom room) return;
        foreach (var player in run.Players.Where(player => MultiplayerAi.IsAi(player.NetId)))
            if (aiRoomInputs.Add((player, room, "treasure_open")))
                AiSubmit(player, new TreasureChestOpenedMessage {
                    Location = RunManager.Instance.RunLocationTargetedBuffer.CurrentLocation
                });
    }
    static void AiProtocolFrame()
    {
        if (!MultiplayerAi.HostsAi || run is null || aiPendingAct is not { } act) return;
        foreach (var player in run.Players.Where(p => MultiplayerAi.IsAi(p.NetId))) {
            if (aiWaitingChoices.ContainsKey(player) || aiBusy.TryGetValue(player, out var busy) && !busy.IsCompleted ||
                aiRewardSets.TryGetValue(player, out var sets) &&
                sets.Any(set => !RunManager.Instance.RewardsSetSynchronizer.IsRewardsSetCompleted(set))) continue;
            if (aiActVotes.Add((player.NetId, act))) AiEnqueue(player, new VoteToMoveToNextActAction(player, act));
        }
    }

    static void AiBuy(Player player, AiAdvisor.LiveOption option)
    {
        var inventory = ((MerchantRoom)run!.CurrentRoom!).Inventories[run.GetPlayerSlotIndex(player)];
        var entries = inventory.AllEntries.ToArray();
        int index = Array.FindIndex(entries, entry => entry switch {
            MerchantCardEntry card => ReferenceEquals(card.CreationResult?.Card, option.Option),
            MerchantRelicEntry relic => ReferenceEquals(relic.Model, option.Option),
            MerchantPotionEntry potion => ReferenceEquals(potion.Model, option.Option),
            _ => ReferenceEquals(entry, option.Option)
        });
        if (index < 0) throw new InvalidOperationException("AI merchant entry is stale");
        var message = new AiShopPurchaseMessage { playerId = player.NetId, entryIndex = index,
            price = entries[index].Cost, location = RunManager.Instance.RunLocationTargetedBuffer.CurrentLocation };
        if (RunManager.Instance.NetService is INetHostGameService host)
        {
            foreach (var peer in HumanPeers(host).Where(peer => RecorderPeer(host, peer)))
                host.SendMessage(message, peer);
        }
        AiPurchaseMessage(message, RunManager.Instance.NetService.NetId);
    }
    static IEnumerable<ulong> HumanPeers(INetHostGameService host) =>
        run!.Players.Select(p => p.NetId).Where(id => id != host.NetId && !MultiplayerAi.IsAi(id));
    static bool RecorderPeer(INetHostGameService host, ulong peer) =>
        host.GetVersionInfoForPeer(peer)?.otherMods?.Contains(NetworkModName, StringComparer.Ordinal) == true;
    static readonly MethodInfo sendWithSender = AccessTools.Method(typeof(NetHostGameService), "SendMessageToClientInternal");
    // Peers without this mod never get AiShopPurchaseMessage. They learn an AI seat's merchant
    // results the way they learn a human's: the game's own sync messages, sent by that seat.
    static void AiTellVanillaPeers<T>(T message, Player player) where T : INetMessage
    {
        if (RunManager.Instance.NetService is NetHostGameService host)
            TellVanillaPeers(host, HumanPeers(host), message, player.NetId);
    }
    internal static void TellVanillaPeers<T>(NetHostGameService host, IEnumerable<ulong> peers, T message, ulong sender)
        where T : INetMessage
    {
        var send = sendWithSender.MakeGenericMethod(typeof(T));
        foreach (var peer in peers.Where(peer => !RecorderPeer(host, peer)))
            send.Invoke(host, [message, peer, message.Mode.ToChannelId(), (ulong?)sender]);
    }
    static bool AiPurchaseReward(RewardObtainedMessage message) => AiForwardPurchaseSync(message);
    static bool AiPurchaseGold(GoldLostMessage message) => AiForwardPurchaseSync(message);
    static bool AiForwardPurchaseSync<T>(T message) where T : INetMessage
    {
        if (aiPurchasePlayer.Value is not { } player) return true;
        AiTellVanillaPeers(message, player);
        return false;
    }
    static void AiPurchaseMessage(AiShopPurchaseMessage message, ulong senderId)
    {
        var manager = RunManager.Instance;
        if (manager.NetService.Type == NetGameType.Host && senderId != manager.NetService.NetId) return;
        if (manager.NetService is INetClientGameService client && senderId != client.NetClient?.HostNetId) return;
        if (!message.location.Equals(manager.RunLocationTargetedBuffer.CurrentLocation)) return;
        var state = manager.DebugOnlyGetState();
        var player = state?.Players.FirstOrDefault(p => p.NetId == message.playerId && MultiplayerAi.IsAi(p.NetId));
        if (state?.CurrentRoom is not MerchantRoom merchant || player is null) return;
        var inventory = merchant.Inventories[state.GetPlayerSlotIndex(player)];
        var entries = inventory.AllEntries.ToArray();
        if (message.entryIndex < 0 || message.entryIndex >= entries.Length ||
            !entries[message.entryIndex].IsStocked || !entries[message.entryIndex].EnoughGold ||
            entries[message.entryIndex].Cost != message.price)
            throw new InvalidDataException("AI_MERCHANT_DOMAIN_MISMATCH");
        aiBusy[player] = AiTrack(AiPurchase(player, inventory, entries[message.entryIndex]));
        MarkAdvisorDirty();
    }
    static async Task AiPurchase(Player player, MerchantInventory inventory, MerchantEntry entry)
    {
        var previous = aiPurchasePlayer.Value;
        aiPurchasePlayer.Value = player;
        try {
            if (entry is MerchantCardRemovalEntry removal) {
                AiTellVanillaPeers(new MerchantCardRemovalMessage { goldCost = removal.Cost,
                    Location = RunManager.Instance.RunLocationTargetedBuffer.CurrentLocation }, player);
                // DoMerchantCardRemoval is the game's synchronized remote path.
                bool removed = await (Task<bool>)AccessTools.Method(typeof(OneOffSynchronizer), "DoMerchantCardRemoval")
                    .Invoke(RunManager.Instance.OneOffSynchronizer, [player, removal.Cost, true])!;
                if (removed) {
                    removal.SetUsed();
                    await MegaCrit.Sts2.Core.Hooks.Hook.AfterItemPurchased(player.RunState, player, removal, removal.Cost);
                    removal.InvokePurchaseCompleted(removal);
                }
            }
            else await entry.OnTryPurchaseWrapper(inventory);
        }
        finally { aiPurchasePlayer.Value = previous; }
    }
    // On the host the sync message is built and then routed to vanilla peers only; mod peers
    // replay the whole purchase from AiShopPurchaseMessage.
    static bool AiPurchaseSync() => aiPurchasePlayer.Value is null || RunManager.Instance.NetService is NetHostGameService;
    static bool AiMerchantRemoval(OneOffSynchronizer __instance, int goldCost, bool cancelable, ref Task<bool> __result)
    {
        if (aiPurchasePlayer.Value is not { } player) return true;
        AiTellVanillaPeers(new MerchantCardRemovalMessage { goldCost = goldCost,
            Location = RunManager.Instance.RunLocationTargetedBuffer.CurrentLocation }, player);
        __result = (Task<bool>)AccessTools.Method(typeof(OneOffSynchronizer), "DoMerchantCardRemoval")
            .Invoke(__instance, [player, goldCost, cancelable])!;
        return false;
    }
}
