using System.Globalization;
using System.Reflection;
using System.Security.Cryptography;
using HarmonyLib;
using MegaCrit.Sts2.Core.Commands;
using MegaCrit.Sts2.Core.Combat;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.GameActions;
using MegaCrit.Sts2.Core.GameActions.Multiplayer;
using MegaCrit.Sts2.Core.Modding;
using MegaCrit.Sts2.Core.Runs;
using MegaCrit.Sts2.Core.Events;
using MegaCrit.Sts2.Core.Multiplayer;
using MegaCrit.Sts2.Core.Multiplayer.Game;
using MegaCrit.Sts2.Core.Multiplayer.Game.Lobby;
using MegaCrit.Sts2.Core.Map;
using MegaCrit.Sts2.Core.Context;
using MegaCrit.Sts2.Core.TestSupport;

namespace HumanPlayRecorder;

public static partial class RecorderMod
{
    static RunState? run;
    static LocalJournal? journal;
    static int sequence;
    static int runLocalSeat;
    static object? terminalSummary;
    static RunResult? terminalResult;
    static string? terminalAt;
    internal static Task LastCompletion { get; private set; } = Task.CompletedTask;
    static readonly SortedDictionary<int, RecordedAction> pendingInputs = new();
    static readonly Dictionary<GameAction, int> executingInputs = new();
    static readonly HashSet<int> cancelledInputs = new();
    static RecordedAction Track(GameAction gameAction, RecordedAction action)
    {
        action = Stamp(action);
        executingInputs.Add(gameAction, action.InputSequence!.Value);
        return action;
    }
    static RecordedAction Stamp(RecordedAction action)
    {
        if (run is not null && action.player >= 0 && action.player < run.Players.Count &&
            currentCardRewards.TryGetValue(run.Players[action.player], out var reward))
            action = action with { offered_cards = reward.Cards
                .Select(c => new CardRef(c.Id.Entry, c.CurrentUpgradeLevel)).ToArray() };
        var parent = pendingInputs.LastOrDefault(p => p.Value.player == action.player);
        var stamped = action with {
            InputSequence = sequence++,
            ParentSequence = parent.Value is null ? null : parent.Key,
            // Losing replay authority must not discard the live selector's lifetime.
            ReplayBefore=CaptureBoundary<ReplayCheckpoint>(
                () => run is not null ? ReplayCheckpoints.Capture(run,rewardIndices.Keys) : null)
        };
        pendingInputs.Add(stamped.InputSequence.Value, stamped);
        AdvisorStamped(stamped);
        return stamped;
    }
    static readonly HashSet<string> BaseCharacters = [
        "IRONCLAD", "SILENT", "DEFECT", "REGENT", "NECROBINDER", "FOOL"
    ];
    internal const string NetworkModName = Edition.NetworkModName;

    // Recording and sharing (online edition, RecorderMod.Sharing.cs). The offline edition has no
    // implementation of the void ones, and RecorderMod.Offline.cs answers the others.
    static partial void InstallSharing(Harmony harmony);
    // Takes the party agreement StartMultiplayer prepared for the StartCore call that follows it.
    static partial void BeginRunSharing();
    static partial void ResetRunSharing();
    static partial void DropLiveSession();
    static partial void NotifyExcluded(string rejection);
    static partial void StartCaptureLease(RunHeader? header, LocalJournal capturing);
    static partial void StartRecording(RunHeader header, (string id, string version, string dll)[] modFiles);
    static partial void RecordLive(LocalJournal destination, ActionRow row, ref bool recorded);
    static partial void VerifyRunSharing(LocalJournal closing);
    static partial void TakeLiveSession(LocalJournal closed, ref Func<Task>? finish);

    static string Timestamp() => DateTime.UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'", CultureInfo.InvariantCulture);
    public static void Init()
    {
        var harmony = new Harmony(Edition.HarmonyId);
        InstallAdvisorIntegration(harmony);
        MultiplayerAi.Install(harmony);
        PerRunContentGate.Install(harmony);
        InstallInventoryAdapters(harmony);
        // Record remote reward choices when consumed, not when a packet arrives.
        // PlayerChoiceSynchronizer.WaitForRemoteChoice also covers buffered and
        // headless/replay results, which do not necessarily traverse receive.
        harmony.Unpatch(AccessTools.Method(typeof(PlayerChoiceSynchronizer), "OnReceivePlayerChoice"),
            AccessTools.Method(typeof(RecorderMod), "RemoteRewardCardChoice"));
        harmony.Patch(AccessTools.Method(typeof(PlayerChoiceSynchronizer), nameof(PlayerChoiceSynchronizer.WaitForRemoteChoice)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterRemoteRewardChoice)));
        harmony.Patch(AccessTools.Method(typeof(GameAction), nameof(GameAction.Cancel)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeCancellation)));
        harmony.Patch(AccessTools.Method(typeof(RunManager), "SetUpNewSingleplayer"),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(StartSingleplayer)));
        harmony.Patch(AccessTools.Method(typeof(RunManager), "SetUpNewMultiplayer"),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(StartMultiplayer)));
        harmony.Patch(AccessTools.Method(typeof(RunManager), "SetUpTest"),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(StartTest)));
        harmony.Patch(AccessTools.Method(typeof(PlayCardAction), "ExecuteAction"),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeCard)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterTask)));
        harmony.Patch(AccessTools.Method(typeof(UsePotionAction), "ExecuteAction"),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforePotion)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterTask)));
        harmony.Patch(AccessTools.Method(typeof(RunManager), nameof(RunManager.EnterMapCoord)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeMove)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterTask)));
        harmony.Patch(AccessTools.Method(typeof(PlayerCmd), nameof(PlayerCmd.EndTurn)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeEndTurn)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterVoid)));
        harmony.Patch(AccessTools.Method(typeof(MapSelectionSynchronizer), nameof(MapSelectionSynchronizer.PlayerVotedForMapCoord)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeMapVote)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterVoid)));
        harmony.Patch(AccessTools.Method(typeof(EventSynchronizer), "PlayerVotedForSharedOptionIndex"),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeEventVote)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterVoid)));
        harmony.Patch(AccessTools.Method(typeof(EventSynchronizer), "ChooseOptionForEvent"),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeEventExecute)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterVoid)));
        harmony.Patch(AccessTools.Method(typeof(EventOption), nameof(EventOption.Chosen)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeEvent)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterTask)));
        harmony.Patch(AccessTools.Method(typeof(RestSiteSynchronizer), "ChooseOption"),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeRest)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterRest)));
        harmony.Patch(AccessTools.Method(typeof(RunManager), "OnEnded"),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(Ended)));
        InstallSharing(harmony);
    }
    static void AfterRemoteRewardChoice(Player player, uint choiceId,
        ref Task<PlayerChoiceResult> __result)
    {
        if (run is null || journal is null || LocalContext.IsMe(player) ||
            !currentCardRewards.ContainsKey(player)) return;
        __result = CaptureRemoteRewardChoice(__result, player, choiceId);
    }
    static async Task<PlayerChoiceResult> CaptureRemoteRewardChoice(
        Task<PlayerChoiceResult> task, Player player, uint choiceId)
    {
        var result = await task;
        RewardCardChoice(player, choiceId, result);
        return result;
    }
    static void StartSingleplayer(RunState state) => StartCore(state, null);
    static void StartTest(RunState state) => StartCore(state,
        state.Players.Count > 1 ? $"oracle-{state.Rng.Seed}-{state.Players.Count}" : null);
    static void StartCore(RunState state, string? runStartToken)
    {
        BeginRunSharing();
        StopAdvisor();
        advisorRecordingHeader = null;
        string decision = "skipped: initialization incomplete";
        try
        {
            if (journal is not null) { var previous = journal; previous.Invalidate(); _ = previous.DisposeAsync(); }
            run = null; journal = null; sequence = 0;
            terminalSummary = null;
            terminalResult = null; terminalAt = null;
            ResetRunSharing();
            pendingInputs.Clear();
            executingInputs.Clear(); cancelledInputs.Clear();
            ResetInventoryAdapterState();
            PerRunContentGate.Active=null;
            bool wantsAdvisor = advisorMain is not null &&
                (Settings.RecommendationSettings.ShowRecommendations || MultiplayerAi.HasAi(state));
            if (wantsAdvisor) Ai.LiveObservation.ObserveRunStart(state);
            // Offline edition: always "offline edition", so nothing past the advisor is set up.
            string? recordingRejection = RecordingRejection(state, runStartToken);
            recordingRejection ??= VanillaGuard.ContentPools.NonNeutralizationReason;
            if (MultiplayerAi.HasAi(state)) recordingRejection = "ai_teammates";
            if (recordingRejection is not null && !wantsAdvisor)
            {
                decision = "skipped: " + recordingRejection;
                return;
            }
            if(state.Rng.StringSeed.StartsWith("old",StringComparison.Ordinal))
            {
                recordingRejection ??= "legacy seed format not replayable numerically";
                if (!wantsAdvisor) { decision="skipped: "+recordingRejection; return; }
            }
            if (state.AscensionLevel is < 0 or > 10)
            {
                decision = $"skipped: ascension {state.AscensionLevel}";
                recordingRejection ??= $"ascension {state.AscensionLevel}";
                if (!wantsAdvisor) return;
            }
            if (state.Players.Count is < 1 or > 4)
            {
                decision = "skipped: unsupported party";
                RefuseAdvisor("UNSUPPORTED_PARTY");
                return;
            }
            if (state.Players.Any(p => !BaseCharacters.Contains(p.Character.Id.Entry))) {
                recordingRejection ??= "unsupported party";
                if (!wantsAdvisor) { decision="skipped: "+recordingRejection; return; }
            }
            // LocalContext.NetId is assigned later by RunManager.Launch().
            // SetUpNew* already has NetService, so use that authoritative ID.
            int localSeat = ResolveLocalSeat(state, RunManager.Instance.NetService.NetId);
            if (localSeat < 0)
            {
                decision = "skipped: local network seat unavailable";
                return;
            }
            var (modFiles, rejection) = PrepareRunMods();
            if (rejection is not null)
            {
                decision = "skipped: " + rejection;
                NotifyExcluded(rejection);
                recordingRejection ??= rejection;
                if (!wantsAdvisor) return;
            }
            var contentGate=TestMode.IsOn?null:new PerRunContentGate(VanillaContent.Load());
            PerRunContentGate.Active=contentGate;
            foreach(var p in state.Players)
            {
                contentGate?.Observe(p.Character);
                foreach(var c in p.Deck.Cards) contentGate?.Observe(c);
                foreach(var r in p.Relics) contentGate?.Observe(r);
                foreach(var potion in p.PotionSlots.Where(p=>p is not null)) contentGate?.Observe(potion!);
            }
            foreach(var modifier in state.Modifiers) contentGate?.Observe(modifier);
            if(contentGate is {Trainable:false}) {
                decision="skipped: "+contentGate.Failure;
                recordingRejection ??= contentGate.Failure;
                if (!wantsAdvisor) return;
            }
            run = state;
            runLocalSeat = localSeat;
            var id = Guid.NewGuid().ToString();
            string mode = state.Players.Count == 1 ? "solo" : "multiplayer";
            var party = state.Players.Select((p, seat) => new PartySeat(seat, p.Character.Id.Entry)).ToArray();
            RunHeader? header;
            try { header = CreateRunHeader(state, id, runStartToken ?? id, localSeat, party); }
            catch (Exception) when (wantsAdvisor) {
                header = null;
                recordingRejection ??= "initial replay capture unavailable";
            }
            journal = NewJournal(id, persist: recordingRejection is null);
            StartCaptureLease(header, journal);
            StartAdvisor(header, state.AscensionLevel, typeof(RunState).Assembly.Location, state, localSeat);
            AttachTreasureSynchronizer();
            if (recordingRejection is not null) {
                decision = "skipped: " + recordingRejection;
                return;
            }
            journal.Append(new { kind = "entry", schema = "sts2-human-run-v2",
                action_schema = RecordingFormat.ActionSchema, run_id = id,
                character = state.Players[localSeat].Character.Id.Entry, ascension = state.AscensionLevel,
                seed = state.Rng.Seed.ToString(CultureInfo.InvariantCulture),
                mode, party_size = party.Length, local_seat = localSeat, party,
                started_at = Timestamp(), replay_certified = false });
            StartRecording(header!, modFiles);
            decision = $"recording: ascension {state.AscensionLevel}, party_size {party.Length}, local_seat {localSeat}";
        }
        catch (Exception e)
        {
            journal?.Invalidate();
            RefuseAdvisor("RUN_INITIALIZATION_FAILED");
            run = null;
            DropLiveSession();
            decision = RunStartDiagnostics.Failure(e);
        }
        finally { Godot.GD.Print("[HumanPlayRecorder] run start " + decision); }
    }
    static int ResolveLocalSeat(RunState state, ulong serviceNetId)
    {
        int seat = state.GetPlayerSlotIndex(serviceNetId);
        return seat >= 0 && seat < state.Players.Count ? seat : -1;
    }
    static TargetRef? Target(MegaCrit.Sts2.Core.Entities.Creatures.Creature? target)
    {
        if (target is null) return null;
        var combat = target.CombatState!;
        int creatureIndex = combat.Creatures.ToList().IndexOf(target);
        return target.IsPlayer
            ? new("player", combat.Players.ToList().IndexOf(target.Player!),
                target.Player!.Character.Id.Entry, creatureIndex)
            : new("enemy", combat.Enemies.ToList().IndexOf(target),
                target.Monster!.Id.Entry, creatureIndex);
    }
    static void BeforeCard(PlayCardAction __instance, out RecordedAction? __state)
    {
        __state = null;
        if (run is null || journal is null) return;
        try
        {
            var card = __instance.NetCombatCard.ToCardModelOrNull();
            if (card is null) { journal.Invalidate(); return; }
            int hand = __instance.Player.PlayerCombatState!.Hand.Cards.ToList().IndexOf(card);
            __state = new("PlayCard", run.GetPlayerSlotIndex(__instance.Player),
                hand_index: hand, card: new(card.Id.Entry, card.CurrentUpgradeLevel),
                target: Target(__instance.Target));
            RecordingFormat.Validate(__state);
            __state = Track(__instance, __state);
        }
        catch (Exception) { journal.Invalidate(); __state = null; }
    }
    static void BeforePotion(UsePotionAction __instance, out RecordedAction? __state)
    {
        __state = null;
        if (run is null || journal is null) return;
        try
        {
            var p = __instance.Player;
            var potion = p.GetPotionAtSlotIndex((int)__instance.PotionIndex);
            __state = new("UsePotion", run.GetPlayerSlotIndex(p),
                slot_index: (int)__instance.PotionIndex, potion_id: potion.Id.Entry,
                target: Target(p.Creature.CombatState?.GetCreature(__instance.TargetId)));
            RecordingFormat.Validate(__state);
            __state = Track(__instance, __state);
        }
        catch (Exception) { journal.Invalidate(); __state = null; }
    }
    static void BeforeMove(MapCoord coord,
        out RecordedAction? __state)
    {
        __state = null;
        if (run is null || journal is null) return;
        __state = CaptureBoundary<RecordedAction>(() => {
            if (run.CurrentActIndex == 0 && run.TotalFloor == 0 &&
                coord == run.Map.StartingMapPoint.coord) return null;
            return Stamp(new("Move", 0, coord: [coord.col, coord.row],
                sync_kind: run.Players.Count > 1 ? "map_travel" : null));
        });
    }
    static void BeforeMapVote(Player player, MapVote? destination, out RecordedAction? __state)
    {
        __state = null;
        if (run is null || journal is null || run.Players.Count < 2) return;
        try
        {
            __state = Stamp(new("Move", run.GetPlayerSlotIndex(player),
                coord: destination is { } vote ? [vote.coord.col, vote.coord.row] : null,
                sync_kind: "map_vote",
                generation: destination?.mapGenerationCount ?? RunManager.Instance.MapSelectionSynchronizer.MapGenerationCount,
                cancelled: destination is null ? true : null));
            RecordingFormat.Validate(__state);
        }
        catch (Exception) { journal.Invalidate(); __state = null; }
    }
    static void BeforeEndTurn(Player player,
        out RecordedAction? __state)
    {
        __state = null;
        if (run is null || journal is null) return;
        __state = CaptureBoundary<RecordedAction>(() => CombatManager.Instance.IsPlayerReadyToEndTurn(player)
            ? null : Stamp(new("EndTurn", run.GetPlayerSlotIndex(player))));
    }
    static void AfterVoid(RecordedAction? __state)
    {
        if (__state is null || journal is null || run is null) return;
        _ = Observe(Task.CompletedTask, __state, run, journal);
    }
    static void BeforeEvent(EventOption __instance, out RecordedAction? __state)
    {
        __state = null;
        if (run is null || run.Players.Count != 1) return;
        try
        {
            var events = RunManager.Instance.EventSynchronizer.Events;
            foreach (var e in events)
            {
                int index = e.CurrentOptions.ToList().IndexOf(__instance);
                if (index < 0 || e.Owner is null) continue;
                __state = Stamp(new("EventChoice", run.GetPlayerSlotIndex(e.Owner),
                    option_id: __instance.TextKey, index: index));
                return;
            }
            journal?.Invalidate();
        }
        catch (Exception) { journal?.Invalidate(); }
    }
    static void BeforeEventVote(Player player, uint optionIndex, uint pageIndex,
        out RecordedAction? __state)
    {
        __state = null;
        if (run is null || journal is null || run.Players.Count < 2) return;
        try
        {
            var e = RunManager.Instance.EventSynchronizer.GetEventForPlayer(player);
            if (optionIndex >= e.CurrentOptions.Count) { journal.Invalidate(); return; }
            __state = Stamp(new("EventChoice", run.GetPlayerSlotIndex(player),
                option_id: e.CurrentOptions[(int)optionIndex].TextKey, index: checked((int)optionIndex),
                sync_kind: "event_vote", generation: checked((int)pageIndex)));
            RecordingFormat.Validate(__state);
        }
        catch (Exception) { journal.Invalidate(); __state = null; }
    }
    static void BeforeEventExecute(Player player, int optionIndex, out RecordedAction? __state)
    {
        __state = null;
        if (run is null || journal is null || run.Players.Count < 2) return;
        try
        {
            var e = RunManager.Instance.EventSynchronizer.GetEventForPlayer(player);
            if (optionIndex < 0 || optionIndex >= e.CurrentOptions.Count) { journal.Invalidate(); return; }
            __state = Stamp(new("EventChoice", run.GetPlayerSlotIndex(player),
                option_id: e.CurrentOptions[optionIndex].TextKey, index: optionIndex,
                sync_kind: "event_execute"));
            RecordingFormat.Validate(__state);
        }
        catch (Exception) { journal.Invalidate(); __state = null; }
    }
    static void BeforeRest(RestSiteSynchronizer __instance, Player player, int optionIndex,
        out RecordedAction? __state)
    {
        __state = null;
        if (run is null) return;
        try
        {
            var option = __instance.GetOptionsForPlayer(player)[optionIndex];
            __state = Stamp(new("RestOption", run.GetPlayerSlotIndex(player),
                option_id: option.OptionId, index: optionIndex));
        }
        catch (Exception) { journal?.Invalidate(); }
    }
    static void AfterRest(ref Task<bool> __result, RecordedAction? __state)
    {
        if (__state is null || journal is null || run is null) return;
        __result = ObserveRest(__result, __state, run, journal);
    }
    static async Task<bool> ObserveRest(Task<bool> task, RecordedAction action,
        RunState state, LocalJournal destination)
    {
        bool success = await task;
        if (success) await Observe(Task.CompletedTask, action, state, destination);
        else
        {
            if (action.InputSequence is { } seq) pendingInputs.Remove(seq);
            destination.Invalidate(); // Never silently leave a hole in the input sequence.
        }
        return success;
    }
    static StateDigest Digest(RunState state) => new(state.CurrentActIndex + 1, state.TotalFloor,
        state.Players.Select((p, seat) => new DigestPlayer(seat,
            p.Creature.CurrentHp, p.Gold, p.Deck.Cards.Count,
            p.Deck.Cards.Select(c => new CardRef(c.Id.Entry, c.CurrentUpgradeLevel)).ToArray(),
            p.Relics.Select(r => r.Id.Entry).ToArray(),
            p.PotionSlots.Select(potion => potion?.Id.Entry).ToArray())).ToArray()) {
                replay=ReplayCheckpoints.Capture(state,rewardIndices.Keys)
            };
    static void AfterTask(ref Task __result, RecordedAction? __state)
    {
        if (__state is null || journal is null || run is null) return;
        var destination = journal; var state = run;
        __result = Observe(__result, __state, state, destination);
    }
    static async Task Observe(Task original, RecordedAction action, RunState state, LocalJournal destination)
    {
        // Preserve the game's synchronization context and original exception.
        try { await original; }
        catch
        {
            destination.Invalidate();
            if (ReferenceEquals(destination, journal)) RetireInput(action);
            throw;
        }
        try
        {
            if (!ReferenceEquals(destination, journal))
            { destination.Invalidate(); return; }
            if (action.InputSequence is { } cancelled && cancelledInputs.Remove(cancelled))
            {
                destination.Invalidate();
                RetireInput(action);
                FinishJournalIfReady();
                return;
            }
            var digest = Digest(state);
            // EndTurn returns while enemy/turn-start continuations can run on a
            // worker. Its stable checkpoint is the synchronous input boundary;
            // the next input/floor/terminal boundary verifies resulting effects.
            if(action.kind=="EndTurn")digest=digest with{replay=null};
            string phase = RecordingFormat.Phase(action);
            action = action.InputSequence is null ? Stamp(action) : action;
            var row = new ActionRow(action.InputSequence!.Value, phase, Timestamp(),
                action, digest, RecordingFormat.Hash(digest), action.ParentSequence) {before=action.ReplayBefore};
            bool current = ReferenceEquals(destination, journal);
            bool recorded = false;
            if (current) RecordLive(destination, row, ref recorded);
            if (!recorded) destination.Append(row);
            if (current) AdvisorCompleted(row);
            RetireInput(action);
            FinishJournalIfReady();
        }
        catch (Exception e) {
            destination.Invalidate();
            if (ReferenceEquals(destination, journal)) {
                RetireInput(action);
                SwitchAdvisorToLive();
            }
            SwallowedCheckpointFailure("RecordAction", e);
        }
    }
    static void RetireInput(RecordedAction action)
    {
        if (action.InputSequence is not { } seq) return;
        pendingInputs.Remove(seq);
        foreach (var key in executingInputs.Where(pair => pair.Value == seq).Select(pair => pair.Key).ToArray())
            executingInputs.Remove(key);
    }
    static void BeforeCancellation(GameAction __instance)
    {
        // Cancelling an unexecuted queued card is not a gameplay input.
        // A cancellation after execution began may have partial effects:
        // retain that run locally instead of claiming a successful replay row.
        if (!executingInputs.TryGetValue(__instance, out int seq)) return;
        cancelledInputs.Add(seq);
        journal?.Invalidate();
    }
    static void Ended(bool isVictory)
    {
        if (run is null || journal is null) { StopAdvisor(); return; }
        try
        {
            terminalAt = Timestamp();
            terminalResult = new(isVictory, run.CurrentActIndex + 1, run.TotalFloor,
                isVictory, run.Players[runLocalSeat].Creature.CurrentHp) {terminal_state=Digest(run)};
            terminalSummary = new { kind = "summary", won = isVictory,
                state = Digest(run), ended_at = terminalAt,
                replay_certified = false, reason = "full_run_adapters_pending" };
            FinishJournalIfReady();
        }
        catch (Exception e) {
            advisorRecordingHeader?.Admission.Reject();
            journal?.Invalidate();
            terminalSummary = null;
            StopAdvisor();
            Console.Error.WriteLine("[HumanPlayRecorder] capture failure: "+RunStartDiagnostics.Failure(e));
        }
    }
    static void FinishJournalIfReady()
    {
        // OnEnded can run inside the final awaited action. Closing here before
        // that action's postfix completes would silently drop its last row.
        if (terminalSummary is null || journal is null || pendingInputs.Count != 0) return;
        VerifyRunSharing(journal);
        var contentGate=PerRunContentGate.Active;
        contentGate?.Finish();
        if(contentGate is {Trainable:false}) journal.Invalidate();
        journal.Append(terminalSummary);
        var closed = journal;
        var header = advisorRecordingHeader;
        bool drainAdvisor = advisorActive && advisorLoaded;
        advisorLoaded = false;
        Task drain;
        try {
            drain = drainAdvisor
                ? Ai.AiAdvisor.DrainAsync().WaitAsync(TerminalAdvisorDrainTimeout)
                : Task.CompletedTask;
        } catch (Exception error) {
            drain = Task.FromException(error);
        }
        if (header is not null) header.Admission.AdviceDrained = drain;
        terminalAdvisorEpoch = advisorEpoch;
        terminalAdvisorDrain = drain;
        terminalSummary = null;
        Func<Task>? finishSession = null;
        TakeLiveSession(closed, ref finishSession);
        LastCompletion = FinishTerminalAfterAdvisorAsync(closed, header, finishSession, drain, advisorEpoch);
        journal = null; run = null;
        terminalResult = null; terminalAt = null;
        ResetRunSharing();
        PerRunContentGate.Active=null;
    }
    static async Task FinishTerminalAfterAdvisorAsync(LocalJournal closed, RunHeader? header,
        Func<Task>? finishSession, Task drain, long epoch)
    {
        try { await drain; }
        catch (Exception) {
            header?.Admission.Reject();
            closed.Invalidate();
        }
        finally {
            if (terminalAdvisorEpoch == epoch) terminalAdvisorDrain = null;
            if (advisorEpoch == epoch) StopAdvisor();
        }
        if (finishSession is not null) await finishSession().ConfigureAwait(false);
        else await closed.DisposeAsync().ConfigureAwait(false);
    }
    static ((string id, string version, string dll)[] files, string? rejection) PrepareRunMods()
    {
        if (TestMode.IsOn) return ([], null);
        var mods = ModManager.GetLoadedMods().ToArray();
        return (mods.SelectMany(m => m.assemblies.Select(a=>(m.manifest!.id!,m.manifest.version!,a.Location))).ToArray(), null);
    }
    static RunHeader CreateRunHeader(RunState state, string id, string runStartToken,
        int localSeat, PartySeat[] party)
    {
        var game = typeof(RunState).Assembly;
        return new RunHeader(id, "0.1.0", GameBuild.Version, "",
            state.Players[localSeat].Character.Id.Entry,
            state.Rng.Seed.ToString(CultureInfo.InvariantCulture), [], Timestamp(),
            party.Length == 1 ? "solo" : "multiplayer", party.Length, localSeat, party,
            RecordingFormat.SharedRunId(state.Rng.Seed.ToString(CultureInfo.InvariantCulture), runStartToken)) {
                ascension=state.AscensionLevel,
                ai_teammate_seats=state.Players.Select((p,i)=>(p,i))
                    .Where(pair=>MultiplayerAi.IsAi(pair.p.NetId)).Select(pair=>pair.i).ToArray(),
                training=new(VanillaContent.Load().fingerprint,"trainable"),
                environment_mods=TestMode.IsOn?[]:ModManager.GetLoadedMods()
                    .Select(m=>new EnvironmentMod(m.manifest!.id!,m.manifest.version??"unknown"))
                    .OrderBy(m=>m.id,StringComparer.Ordinal).ToArray(),
                patch_audit=RuntimePatchAudit.Entries(),
                replay_contract="sts2-replay-checkpoint-v1",
                input_seed=state.Rng.Seed.ToString(CultureInfo.InvariantCulture),
                initial_state=Digest(state),
                setup=new(state.Acts.Select(a=>a.Id.Entry).ToArray(),state.Players.Select((p,i)=>{
                    var unlock=p.UnlockState.ToSerializable();
                    return new UnlockCheckpoint(i,unlock.UnlockedEpochs.Order(StringComparer.Ordinal).ToArray(),
                        unlock.EncountersSeen.Select(e=>e.ToString()).Order(StringComparer.Ordinal).ToArray(),unlock.NumberOfRuns);
                }).ToArray())
            };
    }
}
