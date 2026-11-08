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
    static LiveRunSession? liveSession;
    static RunResult? terminalResult;
    static string? terminalAt;
    static UploadQueue? uploads;
    // Release safety interlock, not an in-game preference. The solo seed replay
    // and four-seat original-DLL transport gates are recorded under evidence/.
    static readonly bool ReleaseReplayVerified = true;
    static string DataDirectory = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "HumanPlayRecorder");
    static LocalStorageBudget Storage = new(DataDirectory);
    internal static void ConfigureOfflineTestDirectory(string path)
    {
        if(!TestMode.IsOn||run is not null)throw new InvalidOperationException("offline test setup only");
        DataDirectory=path;Storage=new(path);
    }
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
        "IRONCLAD", "SILENT", "DEFECT", "REGENT", "NECROBINDER"
    ];
    internal const string NetworkModName = "HumanPlayRecorder-0.1.0";
    sealed class LobbyNonceState(MessageHandlerDelegate<HumanPlayRunNonceMessage> handler,
        MessageHandlerDelegate<HumanPlayConsentMessage> consentHandler)
    {
        public MessageHandlerDelegate<HumanPlayRunNonceMessage> Handler { get; } = handler;
        public MessageHandlerDelegate<HumanPlayConsentMessage> ConsentHandler { get; } = consentHandler;
        public string? Token { get; set; }
        // Net id -> agreed to training data (PartyConsent): on the host what the clients sent,
        // on a client what the host forwarded.
        public Dictionary<ulong, bool> Consent { get; } = new();
    }
    // Set by StartMultiplayer for the StartCore call that follows it; solo runs need no agreement.
    static RunConsent? pendingRunConsent;
    static object? pendingRunService;
    static RunConsent? activeRunConsent;
    static RunHeader? runHeader;
    static object? runService;
    static ulong runSelf;
    static MessageHandlerDelegate<HumanPlayConsentMessage>? runConsentHandler;
    static ConsentWire.MembershipWatch? runMembership;
    // The active capture's permission to go on; sharing switched fully off ends it for good.
    static CaptureLease? captureLease;
    static readonly Dictionary<StartRunLobby, LobbyNonceState> LobbyNonces = new();
    static string Timestamp() => DateTime.UtcNow.ToString("yyyy-MM-dd'T'HH:mm:ss.fffffff'Z'", CultureInfo.InvariantCulture);
    public static void Init()
    {
        var harmony = new Harmony("me.pqp.humanplayrecorder");
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
        foreach (var constructor in AccessTools.GetDeclaredConstructors(typeof(StartRunLobby)))
            harmony.Patch(constructor,
                postfix: new HarmonyMethod(typeof(RecorderMod), nameof(RegisterLobbyNonceHandler)));
        harmony.Patch(AccessTools.Method(typeof(StartRunLobby), "BeginRunForAllPlayers"),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeLobbyBeginRun)));
        harmony.Patch(AccessTools.Method(typeof(StartRunLobby), nameof(StartRunLobby.CleanUp)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(UnregisterLobbyNonceHandler)));
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
        FirstLaunchNotice.Install(harmony);
        StartupConsent.Enable();
        ConsentWire.Install(harmony);
        Settings.SharingSettings.Changed += OwnConsentChanged;
        RunManager.Instance.RoomEntered += CaptureFloor;
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
    static void RegisterLobbyNonceHandler(StartRunLobby __instance)
    {
        if (!__instance.NetService.Type.IsMultiplayer()) return;
        if (LobbyNonces.ContainsKey(__instance)) return;
        MessageHandlerDelegate<HumanPlayRunNonceMessage> handler = (message, senderId) =>
            ReceiveLobbyNonce(__instance, message, senderId);
        MessageHandlerDelegate<HumanPlayConsentMessage> consentHandler = (message, senderId) =>
            ReceiveLobbyConsent(__instance, message, senderId);
        LobbyNonces.Add(__instance, new(handler, consentHandler));
        __instance.NetService.RegisterMessageHandler(handler);
        __instance.NetService.RegisterMessageHandler(consentHandler);
        // Nothing is sent here: whether the host runs this mod is only known once its handshake was
        // seen, and a vanilla host must never receive a packet type it does not know.
        SendOwnConsent(__instance.NetService);
    }
    static void UnregisterLobbyNonceHandler(StartRunLobby __instance)
    {
        if (!LobbyNonces.Remove(__instance, out var state)) return;
        __instance.NetService.UnregisterMessageHandler(state.Handler);
        __instance.NetService.UnregisterMessageHandler(state.ConsentHandler);
    }
    // This player's own answer, to the peers that are known to run this mod (ConsentWire).
    static void SendOwnConsent(object service)
    {
        try { ConsentWire.SendOwn(service, Settings.SharingSettings.TrainingConsent); }
        catch (Exception e) { Godot.GD.Print("[HumanPlayRecorder] consent not sent: " + e.GetType().Name); }
    }
    static void OwnConsentChanged()
    {
        // Both settings off ends the run being captured; switching one back on does not revive it.
        captureLease?.Check();
        foreach (var lobby in LobbyNonces.Keys.ToArray()) SendOwnConsent(lobby.NetService);
        if (activeRunConsent is { } consent && runService is not null)
        {
            SendOwnConsent(runService);
            consent.Observe(runSelf, Settings.SharingSettings.TrainingConsent);
        }
        _ = uploads?.ApplyChoicesAsync();
    }
    // Lobby phase. Whose answer this is comes from the connection that delivered it, and only a peer
    // that runs this mod is believed.
    static void ReceiveLobbyConsent(StartRunLobby lobby, HumanPlayConsentMessage message, ulong headerSender)
    {
        if (!LobbyNonces.TryGetValue(lobby, out var state)) return;
        TakeConsent(lobby.NetService, headerSender, message.agreed, (origin, agreed) => state.Consent[origin] = agreed);
    }
    static void TakeConsent(object service, ulong headerSender, bool agreed, Action<ulong, bool> store)
    {
        if (!ConsentWire.TryOrigin(service, headerSender, out ulong origin)) return;
        if (service is NetHostGameService host)
        {
            if (origin == host.NetId || !ConsentWire.HasMod(host.GetVersionInfoForPeer(origin))) return;
            store(origin, agreed);
            ConsentWire.Relay(host, origin, agreed);
        }
        else store(origin, agreed);
    }
    // The run's table: the local answer is always read from the setting, never from what a host forwarded.
    static RunConsent NewRunConsent(RunState state, object service, IReadOnlyDictionary<ulong, bool> known)
    {
        ulong self = service is NetHostGameService host ? host.NetId : ((NetClientGameService)service).NetId;
        ulong hostId = service is NetClientGameService client ? client.HostNetId : self;
        runSelf = self;
        var table = new Dictionary<ulong, bool>(known) { [self] = Settings.SharingSettings.TrainingConsent };
        return new RunConsent(hostId, state.Players.Select(p => p.NetId), MultiplayerAi.IsAi, table);
    }
    static void BeforeLobbyBeginRun(StartRunLobby __instance)
    {
        if (__instance.NetService.Type != NetGameType.Host ||
            __instance.NetService is not NetHostGameService host) return;
        if (!LobbyNonces.TryGetValue(__instance, out var state))
        { RegisterLobbyNonceHandler(__instance); state = LobbyNonces[__instance]; }
        string token = Convert.ToHexStringLower(RandomNumberGenerator.GetBytes(16));
        state.Token = token;
        ConsentWire.Announce(host, state.Consent, Settings.SharingSettings.TrainingConsent);
        var message = new HumanPlayRunNonceMessage { nonce = token };
        foreach (var peer in __instance.Players.Where(p => p.id != __instance.NetService.NetId))
        {
            var version = host.GetVersionInfoForPeer(peer.id);
            if (version?.otherMods?.Contains(NetworkModName, StringComparer.Ordinal) != true) continue;
            __instance.NetService.SendMessage(message, peer.id);
        }
    }
    static void ReceiveLobbyNonce(StartRunLobby lobby, HumanPlayRunNonceMessage message, ulong senderId)
    {
        if (!LobbyNonces.TryGetValue(lobby, out var state) ||
            lobby.NetService.Type != NetGameType.Client ||
            lobby.NetService is not INetClientGameService client ||
            client.NetClient is null || senderId != client.NetClient.HostNetId ||
            message.nonce.Length != 32 || !message.nonce.All(Uri.IsHexDigit))
            return;
        state.Token = message.nonce.ToLowerInvariant();
    }
    static void StartMultiplayer(RunState state, StartRunLobby lobby)
    {
        ReleaseRunConsent();
        if (!LobbyNonces.TryGetValue(lobby, out var nonce) || nonce.Token is not { } token)
        {
            // Not even this local stub is written while nothing may be recorded.
            if (SharingAllowsRecording) QuarantineStart(state, "multiplayer_run_nonce_unavailable");
            StartCore(state, null);
            return;
        }
        nonce.Token = null; // A nonce is valid for exactly one SetUpNewMultiplayer call.
        pendingRunConsent = NewRunConsent(state, lobby.NetService, nonce.Consent);
        pendingRunService = lobby.NetService;
        StartCore(state, token);
    }
    // Recording is allowed only while training consent or replay saving is on, and the session
    // was not opted out at start-up.
    static bool SharingAllowsRecording => !StartupConsent.RecordingDeclinedThisSession &&
        UploadPolicy.Allowed(Settings.SharingSettings.TrainingConsent, Settings.SharingSettings.ReplaySaving);

    // The run's consent follows it to completion: any change in who plays, or a withdrawal, ends upload eligibility.
    static void BindRunConsent(RunConsent consent, RunHeader header, object service)
    {
        activeRunConsent = consent;
        runHeader = header;
        runService = service;
        consent.Broken += why =>
        {
            Godot.GD.Print("[HumanPlayRecorder] multiplayer run no longer uploadable: " + why);
            header.Admission.Reject();
            journal?.Invalidate();
        };
        if (!consent.Uploadable) { header.Admission.Reject(); journal?.Invalidate(); }
        runConsentHandler = (message, headerSender) =>
            TakeConsent(service, headerSender, message.agreed, (origin, agreed) => consent.Observe(origin, agreed));
        ((INetGameService)service).RegisterMessageHandler(runConsentHandler);
        // Every recorder, host or client, hears about the others leaving, joining or reconnecting.
        runMembership = new ConsentWire.MembershipWatch(service, why => consent.MembershipChanged(why));
    }
    static void ReleaseRunConsent()
    {
        if (runService is INetGameService service && runConsentHandler is not null)
            service.UnregisterMessageHandler(runConsentHandler);
        runMembership?.Dispose();
        runConsentHandler = null; runMembership = null; runService = null;
        activeRunConsent = null; runHeader = null;
    }
    static void QuarantineStart(RunState state, string reason)
    {
        Godot.GD.Print("[HumanPlayRecorder] run start skipped: " + reason);
        var id = Guid.NewGuid().ToString();
        var incomplete = new LocalJournal(Path.Combine(DataDirectory, "incomplete"), id, Storage);
        incomplete.Append(new { kind = "entry", schema = "sts2-human-run-v1", run_id = id,
            seed = state.Rng.Seed.ToString(CultureInfo.InvariantCulture), reason });
        incomplete.Invalidate();
        _ = incomplete.DisposeAsync();
    }
    static void StartCore(RunState state, string? runStartToken)
    {
        var runConsent = pendingRunConsent;
        pendingRunConsent = null;
        bool partyAgreed = runConsent?.Uploadable ?? true;
        if (runConsent is null) ReleaseRunConsent();
        StopAdvisor();
        advisorRecordingHeader = null;
        string decision = "skipped: initialization incomplete";
        try
        {
            if (journal is not null) { var previous = journal; previous.Invalidate(); _ = previous.DisposeAsync(); }
            run = null; journal = null; captureLease = null; sequence = 0;
            terminalSummary = null;
            liveSession = null; terminalResult = null; terminalAt = null;
            pendingInputs.Clear();
            executingInputs.Clear(); cancelledInputs.Clear();
            ResetInventoryAdapterState();
            PerRunContentGate.Active=null;
            bool wantsAdvisor = advisorMain is not null &&
                (Settings.RecommendationSettings.ShowRecommendations || MultiplayerAi.HasAi(state));
            if (wantsAdvisor) Ai.LiveObservation.ObserveRunStart(state);
            string? recordingRejection = StartupConsent.RecordingDeclinedThisSession
                ? "declined at start-up" : state.Players.Count > 1 &&
                    runStartToken is null && !TestMode.IsOn ? "multiplayer_run_nonce_unavailable" : null;
            // Training consent and replay saving both off: nothing is recorded, not even locally.
            if (!UploadPolicy.Allowed(Settings.SharingSettings.TrainingConsent, Settings.SharingSettings.ReplaySaving))
                recordingRejection ??= "training consent and replay saving are both off";
            // Everyone must have agreed, or nothing about this party is recorded for upload.
            if (!partyAgreed)
            {
                recordingRejection ??= "multiplayer party has not all agreed to training data";
                if (!TestMode.IsOn)
                    Godot.Callable.From(() => FirstLaunchNotice.ShowShortNote(Settings.UiText.Get("mod_name"),
                        Settings.UiText.Get("mp_upload_skipped_notice"))).CallDeferred();
            }
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
                if(!TestMode.IsOn) FirstLaunchNotice.ShowExcluded(rejection);
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
            journal = new LocalJournal(Path.Combine(DataDirectory,"incomplete"), id, Storage,
                persist: recordingRejection is null);
            var capturing = journal;
            captureLease = new(() => UploadPolicy.Allowed(Settings.SharingSettings.TrainingConsent, Settings.SharingSettings.ReplaySaving),
                () => {
                    Godot.GD.Print("[HumanPlayRecorder] capture stopped: training consent and replay saving were switched off");
                    header?.Admission.Reject();
                    capturing.Invalidate();
                    if (ReferenceEquals(capturing, journal)) liveSession?.Discard();
                });
            StartAdvisor(header, state.AscensionLevel, typeof(RunState).Assembly.Location, state, localSeat);
            AttachTreasureSynchronizer();
            if (recordingRejection is not null) {
                decision = "skipped: " + recordingRejection;
                return;
            }
            journal.Append(new { kind = "entry", schema = "sts2-human-run-v2",
                action_schema = "sts2-native-input-v1", run_id = id,
                character = state.Players[localSeat].Character.Id.Entry, ascension = state.AscensionLevel,
                seed = state.Rng.Seed.ToString(CultureInfo.InvariantCulture),
                mode, party_size = party.Length, local_seat = localSeat, party,
                started_at = Timestamp(), replay_certified = false });
            BeginLiveSession(header!, modFiles);
            if (runConsent is not null) BindRunConsent(runConsent, header!, pendingRunService!);
            decision = $"recording: ascension {state.AscensionLevel}, party_size {party.Length}, local_seat {localSeat}";
        }
        catch (Exception e)
        {
            journal?.Invalidate();
            RefuseAdvisor("RUN_INITIALIZATION_FAILED");
            run = null;
            liveSession = null;
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
            string phase = action.kind switch {
                "Move" => "MapChoice", "RestOption" => "RestSite",
                "EventChoice" => "Event",
                "BuyCard" or "BuyPotion" or "BuyRelic" or "RemoveCard" => "Shop",
                "Collect" => "Treasure",
                "OpenReward" or "PickCard" or "PickCardBundle" or "RerollCardReward" or
                "PickPotion" or "PickRelicReward" or "SkipRewards" or "SkipPotion" or
                "SkipRelic" or "Skip" => "CardReward",
                _ => "Combat"
            };
            action = action.InputSequence is null ? Stamp(action) : action;
            var row = new ActionRow(action.InputSequence!.Value, phase, Timestamp(),
                action, digest, RecordingFormat.Hash(digest), action.ParentSequence) {before=action.ReplayBefore};
            bool current = ReferenceEquals(destination, journal);
            if (current && liveSession is { } live) live.Record(destination, row);
            else destination.Append(row);
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
        if (activeRunConsent is { } consent)
        {
            // Completion re-checks the table that agreed at the start; anything else stays on the PC.
            if (run is not null) consent.Verify(run.Players.Select(p => p.NetId));
            if (!consent.Uploadable) { runHeader?.Admission.Reject(); journal.Invalidate(); }
            ReleaseRunConsent();
        }
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
        LastCompletion = FinishTerminalAfterAdvisorAsync(closed, header, liveSession,
            terminalResult, terminalAt, RuntimePatchAudit.Entries(), drain, advisorEpoch);
        journal = null; run = null; captureLease = null;
        liveSession = null; terminalResult = null; terminalAt = null;
        PerRunContentGate.Active=null;
    }
    static async Task FinishTerminalAfterAdvisorAsync(LocalJournal closed, RunHeader? header,
        LiveRunSession? session, RunResult? result, string? endedAt, PatchAuditEntry[] finalAudit,
        Task drain, long epoch)
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
        if (session is not null && result is not null && endedAt is not null) {
            session.End(closed, result, endedAt, closed.Complete, finalAudit);
            await session.Completion.ConfigureAwait(false);
        } else await closed.DisposeAsync().ConfigureAwait(false);
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
        return new RunHeader(id, "0.1.0", game.GetName().Version!.ToString(), "",
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
    static void BeginLiveSession(RunHeader header, (string id, string version, string dll)[] modFiles)
    {
        uploads ??= new UploadQueue(Path.Combine(DataDirectory,
            TestMode.IsOn || !ReleaseReplayVerified ? "uncertified" : "pending"), budget: Storage);
        if (ReleaseReplayVerified && !TestMode.IsOn)
        {
            uploads.StampReceipt ??= ReplayLinks.Stamp;
            uploads.Allowed ??= () => UploadPolicy.Allowed(Settings.SharingSettings.TrainingConsent, Settings.SharingSettings.ReplaySaving);
            uploads.Accepted ??= ReplayLinks.Announce;
            _ = uploads.StartAsync();
        }
        var game = typeof(RunState).Assembly;
        UploadMetadata.Validate(header.environment_mods!,header.patch_audit);
        liveSession = new LiveRunSession(header, game.Location, modFiles, uploads, OnLiveFailure,
            message => Godot.GD.Print(message));
    }
    // "Delete all my data": every upload queue of this install is cancelled and purged (UploadQueue.PurgeAllAsync).
    internal static async Task PurgeUploadsAsync(DateTimeOffset at)
    {
        var current = uploads;
        foreach (var name in new[] { "pending", "uncertified" })
        {
            string path = Path.Combine(DataDirectory, name);
            if (current is not null && current.IsAt(path)) { await current.PurgeAllAsync(at); continue; }
            await using var other = new UploadQueue(path, budget: Storage);
            await other.PurgeAllAsync(at);
        }
    }
    static void OnLiveFailure(Exception error)
    {
        if (error is InvalidOperationException { Message: "queue_full" })
            FirstLaunchNotice.ShowQueueFull();
    }
    static void CaptureFloor()
    {
        if (run is null || journal is null || liveSession is null) return;
        try
        {
            if (run.TotalFloor > 0) liveSession.Floor(Digest(run),
                pendingInputs.Count == 0 ? sequence - 1 : pendingInputs.Keys.Min() - 1);
        }
        catch (Exception) { journal.Invalidate(); }
    }
}
