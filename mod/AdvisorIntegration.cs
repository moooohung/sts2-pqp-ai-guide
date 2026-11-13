using System.Security.Cryptography;
using System.Text.Json;
using HarmonyLib;
using HumanPlayRecorder.Ai;
using HumanPlayRecorder.Settings;
using MegaCrit.Sts2.Core.Runs;
using MegaCrit.Sts2.Core.TestSupport;

namespace HumanPlayRecorder;

public static partial class RecorderMod
{
    static SynchronizationContext? advisorMain;
    static long advisorEpoch;
    static bool advisorActive, advisorLoaded;
    static bool advisorReplayable;
    static int advisorSequence;
    static RunHeader? advisorRecordingHeader;
    static Task? terminalAdvisorDrain;
    static long terminalAdvisorEpoch;
    static readonly TimeSpan TerminalAdvisorDrainTimeout = TimeSpan.FromSeconds(2);
    static readonly SortedDictionary<int, string> advisorInputs = new();
    static bool AdvisorRequiresLiveContent =>
        !VanillaGuard.ContentPools.IsNeutralized && VanillaGuard.ContentPools.NonNeutralizationReason is not null;

    static void InstallAdvisorIntegration(Harmony harmony) => AdvisorBoundary(() => {
        // Original-DLL verification owns its own advisor and never starts HTTP.
        if (!GameBuild.PolicyEnabled || TestMode.IsOn) return;
        advisorMain = SynchronizationContext.Current;
        // The game loads mods into its own load context, whose native probing does not
        // reach the mod folder; bind the bridge to the copy shipped beside this assembly.
        string bridge = Path.Combine(Path.GetDirectoryName(typeof(AiAdvisor).Assembly.Location)!, "sts2_advisor_bridge.dll");
        System.Runtime.InteropServices.NativeLibrary.SetDllImportResolver(typeof(AiAdvisor).Assembly, (name, _, _) =>
            name == "sts2_advisor_bridge" ? System.Runtime.InteropServices.NativeLibrary.Load(bridge) : IntPtr.Zero);
        AiAdvisor.Install(harmony);
        // Decision-time search plays the host's AI teammates; the human's badges stay the policy's.
        AiAdvisor.SearchesSeat = seat => run is { } state && seat < state.Players.Count && AiOwned(state.Players[seat]);
        AiAdvisor.SeatDeadline = seat => run is { } state && seat < state.Players.Count
            ? aiFallbackDeadline.ActBy(state.Players[seat], System.Diagnostics.Stopwatch.GetTimestamp(), AiFallbackDelay,
                (long)(System.Diagnostics.Stopwatch.Frequency * AiActMarginSeconds))
            : long.MaxValue;
        AiAdvisor.Log = line => AdvisorLog(line);
        AiAdvisor.Changed += () => {
            AdvisorLog($"status={AiAdvisor.Status} failure={AiAdvisor.LastFailureKind} " +
                $"refused={AiAdvisor.LastRefusal is not null} unavailable={AiAdvisor.UnavailableReason is not null} " +
                $"advice={AiAdvisor.AdviceForSeat(runLocalSeat)?.Count ?? -1} path={AiAdvisor.LastAdvicePath ?? "-"} " +
                $"active={advisorActive} loaded={advisorLoaded}");
            // Several refusal paths (budget overruns, native legal-set or bridge errors) leave no
            // other trace; log each new reason once so a silent teammate fallback is diagnosable.
            if (AiAdvisor.LastRefusal is { } refusal && refusal != loggedRefusal)
                AdvisorTrace("refusal " + (refusal.Length > 300 ? refusal[..300] : refusal));
            loggedRefusal = AiAdvisor.LastRefusal;
        };
        Recommend.Recommendations.Install(harmony);
        RecommendationSettings.ShowRecommendationsChanged += () => AdvisorBoundary(() => {
            if (!RecommendationSettings.ShowRecommendations && !MultiplayerAi.HostsAi) StopAdvisor();
            else EnsureLiveAdvisor();
        });
        InstallAdvisorDecisions(harmony);
        harmony.Patch(AccessTools.Method(typeof(RunManager), nameof(RunManager.CleanUp)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(StopAdvisor)));
        harmony.Patch(AccessTools.Method(typeof(LocalJournal), nameof(LocalJournal.Invalidate)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AdvisorCaptureFailed)));
    });

    static readonly Dictionary<string, long> advisorLogged = new(StringComparer.Ordinal);
    // Per-frame paths alternate between states (idle/capturing, refused/ready), so each
    // distinct line prints at most once per interval rather than on every transition.
    static void AdvisorLog(string line)
    {
        long now = Environment.TickCount64;
        lock (advisorLogged) {
            if (advisorLogged.TryGetValue(line, out long last) && now - last < 30_000) return;
            if (advisorLogged.Count >= 256) advisorLogged.Clear();
            advisorLogged[line] = now;
        }
        Godot.GD.Print("[HumanPlayRecorder] advisor " + line);
    }
    static void AdvisorTrace(string message) => AdvisorLog("trace: " + message);
    static string? loggedRefusal;

    static void AdvisorBoundary(Action action)
    {
        try { action(); }
        catch (Exception error) {
            AdvisorTrace("boundary " + RunStartDiagnostics.Describe(error));
            advisorActive = advisorLoaded = false;
            try { AiAdvisor.EndRun(); } catch (Exception) { }
        }
    }

    static void AdvisorMain(Action action)
    {
        if (advisorMain is null) return;
        AdvisorBoundary(() => {
            if (SynchronizationContext.Current == advisorMain) AdvisorBoundary(action);
            else advisorMain.Post(_ => AdvisorBoundary(action), null);
        });
    }

    static void StopAdvisor() => AdvisorMain(() => {
        if (terminalSummary is not null || terminalAdvisorEpoch == advisorEpoch && terminalAdvisorDrain is not null) return;
        advisorEpoch++;
        advisorActive = advisorLoaded = false;
        advisorReplayable = false;
        advisorInputs.Clear();
        ClearAdvisorDecisions();
        advisorSequence = 0;
        AiAdvisor.EndRun();
    });

    static void RefuseAdvisor(string reason) => AdvisorMain(() => {
        StopAdvisor();
        AiAdvisor.RefuseDecision(0, reason);
    });

    static void AdvisorCaptureFailed(LocalJournal __instance) => AdvisorMain(() => {
        if (ReferenceEquals(__instance, journal)) SwitchAdvisorToLive();
    });

    static void SwitchAdvisorToLive() => AdvisorMain(() => {
        advisorReplayable = false;
        advisorInputs.Clear();
        advisorCaptured.Clear();
        MarkAdvisorDirty();
        AiAdvisor.UseLiveState();
    });

    static void EnsureLiveAdvisor()
    {
        if (!GameBuild.PolicyEnabled || advisorActive || advisorMain is null ||
            !RecommendationSettings.ShowRecommendations && !MultiplayerAi.HostsAi) return;
        var state = RunManager.Instance.DebugOnlyGetState();
        // SetUpSavedMultiplayer sets State, then awaits before InitializeShared sets NetService.
        if (state is null || RunManager.Instance.NetService is not { } net ||
            state.Players.Count is < 1 or > 4) return;
        int seat = ResolveLocalSeat(state, net.NetId);
        if (seat < 0) { AdvisorTrace("local seat unresolved"); return; }
        AdvisorTrace("starting live advisor seat=" + seat);
        if (!ReferenceEquals(run, state)) {
            if (journal is { } previous) { previous.Invalidate(); _ = previous.DisposeAsync(); }
            journal = null; DropLiveSession();
            terminalSummary = null; terminalResult = null; terminalAt = null;
            sequence = 0;
            pendingInputs.Clear(); executingInputs.Clear(); cancelledInputs.Clear();
            ResetInventoryAdapterState();
            PerRunContentGate.Active = null;
        }
        run = state;
        runLocalSeat = seat;
        journal ??= NewJournal(Guid.NewGuid().ToString(), persist: false);
        StartAdvisor(null, state.AscensionLevel, typeof(RunState).Assembly.Location, state, seat);
    }

    static void StartAdvisor(RunHeader? header, int ascension, string gameDll,
        RunState? liveState = null, int liveSeat = 0)
    {
        if (!GameBuild.PolicyEnabled || advisorMain is null) return;
        AdvisorMain(() => {
            if (!RecommendationSettings.ShowRecommendations && !MultiplayerAi.HostsAi) return;
            if (header is not null) advisorRecordingHeader = header;
            var protectedHeader = advisorRecordingHeader;
            var protectedJournal = journal;
            advisorActive = true;
            advisorReplayable = header is not null && !AdvisorRequiresLiveContent;
            int seat = header?.local_seat ?? liveSeat;
            var seed = header?.seed ?? liveState!.Rng.Seed.ToString(System.Globalization.CultureInfo.InvariantCulture);
            var party = header?.party ?? liveState!.Players.Select((p, index) =>
                new PartySeat(index, p.Character.Id.Entry)).ToArray();
            var initial = header?.initial_state;
            var setup = header?.setup;
            long epoch = ++advisorEpoch;
            // Hash the actual installed assembly off-thread, never assume that
            // the currently running game matches the bridge's external ABI pin.
            _ = Task.Run(() => {
                using var stream = File.OpenRead(gameDll);
                return Convert.ToHexStringLower(SHA256.HashData(stream));
            }).ContinueWith(hash => AdvisorMain(() => {
                if (epoch != advisorEpoch || !advisorActive) return;
                if (!hash.IsCompletedSuccessfully) {
                    StopAdvisor();
                    AiAdvisor.RefuseDecision(seat, "GAME_HASH_UNAVAILABLE");
                    return;
                }
                string start = JsonSerializer.Serialize(new {
                    schema = "sts2-human-run-v2", action_schema = RecordingFormat.ActionSchema,
                    game_dll_sha256 = hash.Result, ascension, seed, input_seed = seed,
                    party_size = party.Length, party, local_seat = seat, initial_state = initial, setup,
                    live_advice = true
                }, RecordingFormat.Json);
                AiAdvisor.BindRecordingIntegrity(() => {
                    bool first = protectedHeader?.Admission.Reject() ?? protectedJournal?.Complete == true;
                    if (first && protectedJournal is { Complete: true }) protectedJournal.Invalidate();
                });
                AiAdvisor.BeginRun(start, "public-beta+" + hash.Result[..8],
                    seat, PolicyDirectory);
                if (!advisorReplayable) AiAdvisor.UseLiveState();
                advisorLoaded = true;
                MarkAdvisorDirty();
                FlushAdvisorInputs();
            }), TaskScheduler.Default);
        });
    }

    static void FlushAdvisorInputs()
    {
        if (!advisorLoaded) return;
        while (advisorInputs.Remove(advisorSequence, out var json)) {
            AiAdvisor.ConsumeOriginalInput(json);
            advisorSequence++;
        }
    }

    static void AdvisorStamped(RecordedAction action)
    {
        if (advisorMain is null) return;
        AdvisorMain(() => {
            if (!advisorActive) return;
            LiveObservation.InvalidatePreviews();
            AiAdvisor.InvalidateDecisions();
            advisorCaptured.Clear();
            MarkAdvisorDirty();
            if (!advisorReplayable) return;
            string json = JsonSerializer.Serialize(new {
                seq = action.InputSequence, parent_seq = action.ParentSequence,
                before = action.ReplayBefore, action
            }, RecordingFormat.Json);
            // Selectors reserve their sequence before the player chooses.
            // Their complete original input arrives through Observe instead.
            if (action.kind is "Choose" or "PickCardBundle") return;
            advisorInputs[action.InputSequence!.Value] = json;
            FlushAdvisorInputs();
        });
    }

    static void AdvisorCompleted(ActionRow row)
    {
        if (advisorMain is null) return;
        AdvisorMain(() => {
            if (!advisorActive) return;
            MarkAdvisorDirty();
            if (!advisorReplayable) {
                AiAdvisor.InvalidateDecisions();
                advisorCaptured.Clear();
                return;
            }
            if (row.seq < advisorSequence) return;
            string json = JsonSerializer.Serialize(row, RecordingFormat.Json);
            advisorInputs[row.seq] = json;
            FlushAdvisorInputs();
        });
    }
}
