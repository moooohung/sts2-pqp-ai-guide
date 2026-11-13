using System.Globalization;
using System.Security.Cryptography;
using HarmonyLib;
using MegaCrit.Sts2.Core.Multiplayer;
using MegaCrit.Sts2.Core.Multiplayer.Game;
using MegaCrit.Sts2.Core.Multiplayer.Game.Lobby;
using MegaCrit.Sts2.Core.Runs;
using MegaCrit.Sts2.Core.TestSupport;

namespace HumanPlayRecorder;

// Online edition: recording to disk, party consent, the lobby run nonce and the upload session.
// The offline edition compiles none of this (Editions.props).
public static partial class RecorderMod
{
    static LiveRunSession? liveSession;
    static UploadQueue? uploads;
    // Release safety interlock, not an in-game preference. The solo seed replay
    // and four-seat original-DLL transport gates are recorded under evidence/.
    static readonly bool ReleaseReplayVerified = true;
    static string DataDirectory = Edition.DataDirectory;
    static LocalStorageBudget Storage = new(DataDirectory);
    internal static void ConfigureOfflineTestDirectory(string path)
    {
        if(!TestMode.IsOn||run is not null)throw new InvalidOperationException("offline test setup only");
        DataDirectory=path;Storage=new(path);
    }
    static string PolicyDirectory => Path.Combine(DataDirectory, "policy-cache");
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
    // The agreement StartCore took for the run it is starting (BeginRunSharing).
    static RunConsent? startingRunConsent;
    static RunConsent? activeRunConsent;
    static RunHeader? runHeader;
    static object? runService;
    static ulong runSelf;
    static MessageHandlerDelegate<HumanPlayConsentMessage>? runConsentHandler;
    static ConsentWire.MembershipWatch? runMembership;
    // The active capture's permission to go on; sharing switched fully off ends it for good.
    static CaptureLease? captureLease;
    static readonly Dictionary<StartRunLobby, LobbyNonceState> LobbyNonces = new();

    static partial void InstallSharing(Harmony harmony)
    {
        // An install left over budget by an earlier version recovers before its first run. Harnesses
        // (TestMode) can run Init against the real data folder; they never compact it.
        if (!TestMode.IsOn) { var storage = Storage; _ = Task.Run(async () => {
            await storage.CompactAsync();
            await UploadQueue.RecoverRetainedAsync(storage, FirstLaunchNotice.ShowStorageCapacity);
        }); }
        foreach (var constructor in AccessTools.GetDeclaredConstructors(typeof(StartRunLobby)))
            harmony.Patch(constructor,
                postfix: new HarmonyMethod(typeof(RecorderMod), nameof(RegisterLobbyNonceHandler)));
        harmony.Patch(AccessTools.Method(typeof(StartRunLobby), "BeginRunForAllPlayers"),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeLobbyBeginRun)));
        harmony.Patch(AccessTools.Method(typeof(StartRunLobby), nameof(StartRunLobby.CleanUp)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(UnregisterLobbyNonceHandler)));
        FirstLaunchNotice.Install(harmony);
        StartupConsent.Enable();
        ConsentWire.Install(harmony);
        Settings.SharingSettings.Changed += OwnConsentChanged;
        RunManager.Instance.RoomEntered += CaptureFloor;
    }

    static partial void BeginRunSharing()
    {
        startingRunConsent = pendingRunConsent;
        pendingRunConsent = null;
        if (startingRunConsent is null) ReleaseRunConsent();
    }
    static partial void ResetRunSharing() { captureLease = null; liveSession = null; }
    static partial void DropLiveSession() => liveSession = null;

    static string? RecordingRejection(RunState state, string? runStartToken)
    {
        string? recordingRejection = StartupConsent.RecordingDeclinedThisSession
            ? "declined at start-up" : state.Players.Count > 1 &&
                runStartToken is null && !TestMode.IsOn ? "multiplayer_run_nonce_unavailable" : null;
        if (state.GameMode != GameMode.Standard || state.IsCustomGame || state.Modifiers.Count != 0)
            recordingRejection ??= "unsupported_run_mode";
        // Training consent and replay saving both off: nothing is recorded, not even locally.
        if (!UploadPolicy.Allowed(Settings.SharingSettings.TrainingConsent, Settings.SharingSettings.ReplaySaving))
            recordingRejection ??= "training consent and replay saving are both off";
        // Everyone must have agreed, or nothing about this party is recorded for upload.
        if (!(startingRunConsent?.Uploadable ?? true))
        {
            recordingRejection ??= "multiplayer party has not all agreed to training data";
            if (!TestMode.IsOn)
                Godot.Callable.From(() => FirstLaunchNotice.ShowShortNote(Settings.UiText.Get("mod_name"),
                    Settings.UiText.Get("mp_upload_skipped_notice"))).CallDeferred();
        }
        return recordingRejection;
    }
    static partial void NotifyExcluded(string rejection)
    {
        if (!TestMode.IsOn) FirstLaunchNotice.ShowExcluded(rejection);
    }
    static LocalJournal NewJournal(string id, bool persist) =>
        new(Path.Combine(DataDirectory, "incomplete"), id, Storage, persist: persist);
    static partial void StartCaptureLease(RunHeader? header, LocalJournal capturing)
    {
        captureLease = new(() => UploadPolicy.Allowed(Settings.SharingSettings.TrainingConsent, Settings.SharingSettings.ReplaySaving),
            () => {
                Godot.GD.Print("[HumanPlayRecorder] capture stopped: training consent and replay saving were switched off");
                header?.Admission.Reject();
                capturing.Invalidate();
                if (ReferenceEquals(capturing, journal)) liveSession?.Discard();
            });
    }
    static partial void StartRecording(RunHeader header, (string id, string version, string dll)[] modFiles)
    {
        BeginLiveSession(header, modFiles);
        if (startingRunConsent is not null) BindRunConsent(startingRunConsent, header, pendingRunService!);
    }
    static partial void RecordLive(LocalJournal destination, ActionRow row, ref bool recorded)
    {
        if (liveSession is not { } live) return;
        live.Record(destination, row);
        recorded = true;
    }
    static partial void VerifyRunSharing(LocalJournal closing)
    {
        if (activeRunConsent is not { } consent) return;
        // Completion re-checks the table that agreed at the start; anything else stays on the PC.
        if (run is not null) consent.Verify(run.Players.Select(p => p.NetId));
        if (!consent.Uploadable) { runHeader?.Admission.Reject(); closing.Invalidate(); }
        ReleaseRunConsent();
    }
    static partial void TakeLiveSession(LocalJournal closed, ref Func<Task>? finish)
    {
        if (liveSession is not { } session || terminalResult is not { } result || terminalAt is not { } endedAt) return;
        var finalAudit = RuntimePatchAudit.Entries();
        finish = async () => {
            session.End(closed, result, endedAt, closed.Complete, finalAudit);
            await session.Completion.ConfigureAwait(false);
        };
    }
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
    static void BeginLiveSession(RunHeader header, (string id, string version, string dll)[] modFiles)
    {
        uploads ??= new UploadQueue(Path.Combine(DataDirectory,
            TestMode.IsOn || !ReleaseReplayVerified ? "uncertified" : "pending"), budget: Storage);
        if (ReleaseReplayVerified && !TestMode.IsOn)
        {
            uploads.StampReceipt ??= ReplayLinks.Stamp;
            uploads.Allowed ??= () => UploadPolicy.Allowed(Settings.SharingSettings.TrainingConsent, Settings.SharingSettings.ReplaySaving);
            uploads.Accepted ??= ReplayLinks.Announce;
            uploads.CapacityWarning ??= FirstLaunchNotice.ShowStorageCapacity;
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
    internal static IReadOnlyList<LocalRun> LocalRuns() => Storage.ListRuns();
    // The local storage window frees this PC only; replays on sts.pqp.me stay (that is DeleteAllMine).
    internal static async Task<int> DeleteLocalRunsAsync(IReadOnlyCollection<string> runIds)
    {
        if (uploads is { } current) return await current.DeleteLocalAsync(runIds);
        await using var queue = new UploadQueue(Path.Combine(DataDirectory, "pending"), budget: Storage);
        return await queue.DeleteLocalAsync(runIds);
    }
    static void OnLiveFailure(Exception error)
    {
        if (error is InvalidOperationException { Message: "queue_full" })
            FirstLaunchNotice.ShowStorageCapacity();
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
