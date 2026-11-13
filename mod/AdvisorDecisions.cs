using System.Text.Json;
using HarmonyLib;
using HumanPlayRecorder.Ai;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Nodes.Screens.Map;
using MegaCrit.Sts2.Core.Rewards;
using MegaCrit.Sts2.Core.Rooms;
using MegaCrit.Sts2.Core.Runs;
using MegaCrit.Sts2.Core.Entities.Creatures;
using MegaCrit.Sts2.Core.Localization.DynamicVars;
using MegaCrit.Sts2.Core.Nodes.Screens.CardSelection;

namespace HumanPlayRecorder;

public static partial class RecorderMod
{
    sealed record AdvisorDomain(int Sequence, Func<IReadOnlyList<AiAdvisor.LiveOption>> Options);
    static readonly Dictionary<Player, AdvisorDomain> advisorSelectors = new();
    static readonly Dictionary<Player, IReadOnlyList<Reward>> advisorRewards = new();
    static readonly Dictionary<int, long> advisorCaptured = new();
    static readonly DecisionCaptureVersions advisorVersions = new();
    static long advisorDirty;
    internal static void MarkAdvisorDirty() => Interlocked.Increment(ref advisorDirty);
    const int SignatureInterval = 10;
    const int OverrunLimit = 30;
    static long advisorFrames;
    static readonly Dictionary<int, int> advisorOverruns = new();

    static void InstallAdvisorDecisions(Harmony harmony)
    {
        LiveModelData.Install(harmony);
        GameUiPlayability.Install(harmony);
        harmony.Patch(AccessTools.Method(typeof(RecorderMod), nameof(NewSelection)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AdvisorSelection)));
        harmony.Patch(AccessTools.Method(typeof(RecorderMod), nameof(BeforeBundle)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(AdvisorBundle)));
        harmony.Patch(AccessTools.Method(typeof(RecorderMod), nameof(IndexRewards)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AdvisorRewards)));
        if (Godot.Engine.GetMainLoop() is Godot.SceneTree tree)
            tree.ProcessFrame += AdvisorFrame;
        else Godot.GD.Print("[HumanPlayRecorder] advisor frame hook unavailable: no SceneTree at install");
        RunManager.Instance.RoomEntered += AdvisorRoomEntered;
        harmony.Patch(AccessTools.Method(typeof(CardModel), nameof(CardModel.UpdateDynamicVarPreview)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AdvisorPreview)));
        foreach (var type in new[] { typeof(NCombatPileCardSelectScreen), typeof(NDeckCardSelectScreen),
            typeof(NDeckEnchantSelectScreen), typeof(NSimpleCardSelectScreen), typeof(NDeckUpgradeSelectScreen),
            typeof(NDeckTransformSelectScreen) })
            harmony.Patch(AccessTools.DeclaredMethod(type, "OnCardClicked"),
                postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AdvisorSelectedCards)));
    }
    static void AdvisorPreview(CardModel __instance, Creature? target, DynamicVarSet dynamicVarSet) =>
        AdvisorBoundary(() => {
            if (advisorActive && LiveObservation.ObserveEnginePreview(__instance, target, dynamicVarSet)) MarkAdvisorDirty();
        });
    static void AdvisorSelectedCards(object __instance, CardModel card) => AdvisorBoundary(() => {
        var field = AccessTools.Field(__instance.GetType(), "_selectedCards");
        if (field?.GetValue(__instance) is IEnumerable<CardModel> cards)
            LiveObservation.RememberSelectedCards(card.Owner, cards);
        MarkAdvisorDirty();
    });

    static void AdvisorRoomEntered() => AdvisorMain(() => {
        ClearAdvisorDecisions();
        if (advisorActive) AiAdvisor.InvalidateDecisions();
    });

    static void ClearAdvisorDecisions()
    {
        advisorSelectors.Clear();
        advisorRewards.Clear();
        advisorCaptured.Clear();
        advisorVersions.Clear();
        advisorOverruns.Clear();
        MarkAdvisorDirty();
    }

    static void AdvisorSelection(Selection? __result) => AdvisorBoundary(() => {
        if (__result is not { } selection || run is null) return;
        int seat = run.GetPlayerSlotIndex(selection.Player);
        advisorSelectors[selection.Player] = new(selection.Input.InputSequence!.Value,
            () => LiveObservation.SelectionOptions(seat, selection.Player, selection.Candidates));
        MarkAdvisorDirty();
    });

    static void AdvisorBundle(Player player, IReadOnlyList<IReadOnlyList<CardModel>> bundles) => AdvisorBoundary(() => {
        if (run is null) return;
        int seat = run.GetPlayerSlotIndex(player);
        advisorSelectors[player] = new(sequence, () => GameOptionAdapters.Bundles(seat, bundles));
        MarkAdvisorDirty();
    });

    static void AdvisorRewards(RewardsSet rewards) => AdvisorBoundary(() => {
        foreach (var group in rewards.Rewards.GroupBy(r => r.Player))
            advisorRewards[group.Key] = group.ToArray();
        MarkAdvisorDirty();
    });

    static IReadOnlyList<AiAdvisor.LiveOption> AdvisorOptions(Player player, out int? parent)
    {
        parent = null;
        int seat = run!.GetPlayerSlotIndex(player);
        if (AiOwned(player)) { parent = AiParent(player); return AiOptions(player); }
        if (LiveObservation.CurrentSelectionOptions(run, player) is { } visibleSelection) {
            parent = pendingInputs.LastOrDefault(p => p.Value.player == seat && p.Value.kind == "Choose").Value?.ParentSequence;
            return visibleSelection;
        }
        if (advisorSelectors.TryGetValue(player, out var selection)) {
            if (pendingInputs.TryGetValue(selection.Sequence, out var pending)) {
                parent = pending.ParentSequence;
                return selection.Options();
            }
            advisorSelectors.Remove(player);
        }
        if (currentCardRewards.TryGetValue(player, out var cardReward) &&
            pendingInputs.LastOrDefault(p => p.Value.player == seat && p.Value.kind == "OpenReward").Value is { } opening) {
            parent = opening.InputSequence;
            return GameOptionAdapters.CardReward(seat, cardReward, rewardIndices[cardReward]);
        }
        if (NMapScreen.Instance is { IsOpen: true, IsTravelEnabled: true, IsTraveling: false })
            return GameOptionAdapters.Map(run, player);
        if (advisorRewards.TryGetValue(player, out var rewards) && rewards.Any(r => !r.SuccessfullySelected))
            return GameOptionAdapters.Rewards(seat, rewards, true);
        if (run.CurrentRoom is CombatRoom combat)
            return GameOptionAdapters.Combat(run, player, combat.CombatState);
        if (run.CurrentRoom is EventRoom) {
            // The room is entered a few frames before the synchronizer fills its per-player events.
            if (RunManager.Instance.EventSynchronizer.Events.Count <= seat)
                throw new AdviceUnavailableException(AdviceReason.ScreenLoading, "추천 불가: 이벤트를 아직 불러오는 중입니다.");
            return GameOptionAdapters.Events(seat,
                RunManager.Instance.EventSynchronizer.GetEventForPlayer(player).CurrentOptions);
        }
        if (run.CurrentRoom is RestSiteRoom)
            return GameOptionAdapters.Rest(seat, RunManager.Instance.RestSiteSynchronizer.GetOptionsForPlayer(player));
        if (run.CurrentRoom is TreasureRoom treasure &&
            RunManager.Instance.TreasureRoomRelicSynchronizer.CurrentRelics is { } relics) {
            // The solo chest's gold is not settled while it opens (InventoryAdapters
            // ChestOpening): nothing to advise until it is.
            if (run.Players.Count == 1 && ChestOpening(treasure)) return [];
            return GameOptionAdapters.Treasure(seat, relics, run.Players.Count > 1);
        }
        if (run.CurrentRoom is MerchantRoom merchant) {
            return GameOptionAdapters.Shop(run, player, merchant);
        }
        // No screen is waiting for this player (a room transition, a closed map): there is
        // nothing to advise, which is not a refusal.
        return [];
    }

    static void AdvisorFrame() => AdvisorMain(() => {
        // Every seat's projection covers the whole party, so its cost grows with
        // the player count; a flat 4 ms refused every 4-player combat capture.
        FrameCaptureBudget.BeginFrame(FrameCaptureBudget.Milliseconds * Math.Max(1, run?.Players.Count ?? 1));
        AiPresenceFrame();
        AiFallbackFrame();
        EnsureLiveAdvisor();
        AiProtocolFrame();
        if (!advisorActive || !advisorLoaded || run is null || AiAdvisor.Status != AdvisorStatus.Ready) {
            AdvisorTrace($"frame idle active={advisorActive} loaded={advisorLoaded} run={run is not null} status={AiAdvisor.Status}");
            return;
        }
        AdvisorTrace("frame capturing room=" + run.CurrentRoom?.GetType().Name);
        if (advisorReplayable && AdvisorRequiresLiveContent) SwitchAdvisorToLive();
        // Each seat samples its unsignalled-change check on its own frame, so the checks
        // never stack with each other or with a capture inside one frame budget.
        long frame = ++advisorFrames;
        bool Verify(Player player) => (frame + run!.GetPlayerSlotIndex(player)) % SignatureInterval == 0;
        // The 4 ms frame budget keeps badge captures from stalling frames. AI seats
        // get their own pass and budget: a 3-4 player projection can exceed 4 ms on
        // its own, and a refused human seat must not starve the teammates' turns.
        using (new FrameCaptureBudget()) {
            foreach (var player in run.Players.Where(player => !AiOwned(player)))
                if (!AdvisorSeat(player, Verify(player), ai: false)) break;
        }
        FrameCaptureBudget.BeginPass();
        var ready = new List<AiReadyDecision>();
        using (new FrameCaptureBudget(System.Diagnostics.Stopwatch.GetTimestamp,
            (long)(System.Diagnostics.Stopwatch.Frequency * AiCaptureMilliseconds / 1000))) {
            foreach (var player in run.Players.Where(AiOwned))
                if (!AdvisorSeat(player, Verify(player), ai: true, ready)) break;
        }
        AiDispatchReady(ready);
    });

    const double AiCaptureMilliseconds = 40;
    // Returns false when the rest of this pass must wait for the next frame.
    static bool AdvisorSeat(Player player, bool verifyFrame, bool ai,
        List<AiReadyDecision>? ready = null)
    {
            var run = RecorderMod.run!;
            int seat = run.GetPlayerSlotIndex(player);
            // Only our combat hand has game-produced UI playability.
            if (run.CurrentRoom is CombatRoom && seat != runLocalSeat && !ai) return true;
            bool verifying = false;
            try {
                FrameCaptureBudget.Check();
                long dirty = Volatile.Read(ref advisorDirty);
                // A human seat captured at this dirty mark waits for the sampled check;
                // deriving its options every idle frame was the steady per-frame cost.
                if (!ai && !verifyFrame && advisorCaptured.TryGetValue(seat, out long captured) && captured == dirty)
                    return true;
                var options = AdvisorOptions(player, out int? parent).ToArray();
                if (options.Length == 0) {
                    if (ai) AiClearPresence(player);
                    else { AiAdvisor.InvalidateSeat(seat); advisorCaptured.Remove(seat); }
                    return true;
                }
                if (ai && AiTryPrepare(player, options, ready!)) return true;
                if (advisorCaptured.TryGetValue(seat, out long previous) && previous == dirty) {
                    // The unsignalled-change check projects the whole state; sample it
                    // instead of paying a full projection every frame.
                    if (!verifyFrame) return true;
                    verifying = true;
                    long signature = LiveObservation.Signature(run, seat, options);
                    if (!advisorVersions.Changed(seat, signature)) return true;
                    // A changed screen is recaptured next frame; that is not a refusal.
                    advisorCaptured.Remove(seat);
                    AiAdvisor.InvalidateSeat(seat);
                    return true;
                }
                LiveObservation.Snapshot? snapshot = null;
                string? missing = null;
                var missingReason = AdviceReason.Other;
                try { snapshot = LiveObservation.Capture(run, seat, options); }
                catch (AdviceUnavailableException error) { missing = error.Message; missingReason = error.Reason; }
                ReplayCheckpoint? before = null;
                if (advisorReplayable) {
                    try { before = ReplayCheckpoints.Capture(run, rewardIndices.Keys); }
                    catch (AdviceUnavailableException) { throw; }
                    catch (Exception failure) { SwallowedCheckpointFailure("AdvisorSeat", failure); journal?.Invalidate(); SwitchAdvisorToLive(); }
                }
                FrameCaptureBudget.Check();
                advisorOverruns.Remove(seat);
                if (snapshot is not null) {
                    AiAdvisor.SetLiveDecision(seat, options, snapshot, before, parent);
                    AiPublished(seat, snapshot.Fingerprint, options);
                    advisorVersions.Commit(seat, snapshot.Fingerprint);
                    advisorCaptured[seat] = dirty;
                }
                else if (before is not null) {
                    AiAdvisor.SetReplayDecision(seat, options, before, parent, missing!, missingReason);
                    AiPublished(seat, AiReplayFingerprint(before, options), options, live: false);
                }
                else {
                    string reason = missing ?? "추천 불가: 현재 선택 화면을 읽을 수 없습니다.";
                    AdvisorTrace($"refused seat={seat} reason={reason}");
                    AiAdvisor.RefuseDecision(seat, reason, missingReason);
                }
            } catch (Exception error) {
                if (FrameCaptureBudget.Exhausted) {
                    // A slow frame is not a refusal: keep the shown decision and retry
                    // next frame; refuse only when captures keep overrunning.
                    if (verifying) return false;
                    advisorCaptured.Remove(seat);
                    int overruns = advisorOverruns.GetValueOrDefault(seat) + 1;
                    advisorOverruns[seat] = overruns;
                    if (overruns < OverrunLimit) return false;
                }
                else if (error is not AdviceUnavailableException) AdvisorTrace("capture error " + RunStartDiagnostics.Describe(error));
                else AdvisorTrace($"refused seat={seat} reason={error.Message}");
                advisorCaptured.Remove(seat);
                // A budget overrun can reach here wrapped in another exception: still the budget's refusal.
                AiAdvisor.RefuseDecision(seat, error.Message, FrameCaptureBudget.Exhausted
                    ? AdviceReason.BudgetExceeded : AdviceUnavailableException.ReasonOf(error));
                // One teammate's unavailable domain must not hold back the others.
                if (error is AdviceUnavailableException) return ai;
            }
            return true;
    }
}
