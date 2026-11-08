using System.Collections.Concurrent;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using HarmonyLib;

namespace HumanPlayRecorder.Ai;

public enum AdvisorStatus { Disabled, Downloading, Ready, Unavailable }
public enum AiFailureKind { None, Offline, CheckpointInvalid, Unsupported, Internal }

/// <summary>
/// The policy's advice for the current pending decisions of the run. The recommendation
/// badges and the AI teammate read only this surface; its implementation (checkpoint
/// download at run start, native inference, state mirroring) lives beside it in Ai/.
/// </summary>
public static class AiAdvisor
{
    /// <summary>Option objects for choices that have no game model of their own.</summary>
    public static class Options
    {
        public static readonly object EndTurn = new Sentinel("end_turn");
        public static readonly object Skip = new Sentinel("skip");
        public static readonly object Proceed = new Sentinel("proceed");
        public static readonly object Confirm = new Sentinel("confirm");
    }

    public static AdvisorStatus Status { get; private set; } = AdvisorStatus.Unavailable;

    /// <summary>Checkpoint id and training step of the policy used for this run, when ready.</summary>
    public static string? CheckpointLabel { get; private set; }
    public static string? LastAdvicePath { get; private set; }

    /// <summary>Raised on the game's main thread when status or any advice changes.</summary>
    public static event Action? Changed;
    public static event Action? ReplayIntegrityLost;
    static int replayIntegrityLost;
    static Action? recordingFailure;
    [ThreadStatic] static Action? workerRecordingFailure;
    public static void BindRecordingIntegrity(Action? reject) { MainThread(); recordingFailure = reject; }
    static string? lastRefusal;
    static AdviceReason lastRefusalReason;
    // Only RefuseDecision knows a category; every other refusal text is a code or an exception message.
    public static string? LastRefusal { get => lastRefusal; private set { lastRefusal = value; lastRefusalReason = AdviceReason.Other; } }
    public static AiFailureKind LastFailureKind { get; private set; }
    public enum DecisionAction
    {
        Select, PlayCard, UsePotion, DiscardPotion, EndTurn, Skip, Proceed,
        BuyCard, BuyRelic, BuyPotion, RemoveCard, EventChoice, RestOption,
        Move, PickCard, PickRelic, PickPotion, OpenReward, Collect, Confirm, Vote
    }
    /// <summary>One executable joint action, retaining the original game object references.</summary>
    public sealed record ActionOption(object Source, DecisionAction Action, object? Target = null);
    public sealed record LiveOption(object Option, string Binding,
        DecisionAction Action = DecisionAction.Select, object? Target = null)
    {
        public bool Legal { get; init; } = true;
        public MegaCrit.Sts2.Core.Rewards.Reward? Reward { get; init; }
        public ActionOption Joint => new(Option, Action, Target);
        // The binding is parsed by several projection stages per capture; parse each string once.
        static readonly System.Runtime.CompilerServices.ConditionalWeakTable<string, RecordedAction> parsed = new();
        public RecordedAction Input => parsed.GetValue(Binding,
            binding => System.Text.Json.JsonSerializer.Deserialize<RecordedAction>(binding, RecordingFormat.Json)!);
    }
    static SynchronizationContext? main;
    static readonly Dictionary<int, LiveOption[]> live = new();
    static readonly Dictionary<int, (ReplayCheckpoint Before, int? Parent)> boundaries = new();
    static readonly Dictionary<int, LiveObservation.Snapshot> liveStates = new();
    static readonly Dictionary<int, AdviceRefusal> unavailable = new();
    /// <summary>Why the local seat has no advice: the category the player sees plus the detail that is only logged.</summary>
    public static AdviceRefusal? Unavailable => unavailable.GetValueOrDefault(localSeat) ??
        (LastFailureKind == AiFailureKind.Unsupported && LastRefusal is { } refusal ? new(lastRefusalReason, refusal) : null);
    public static string? UnavailableReason => Unavailable?.Detail;
    static bool replayUnavailable;
    static readonly Dictionary<int, IReadOnlyList<(object Option, float Probability)>> advice = new();
    // Decision-time search: the option binding it chose per seat, for seats that act on their own.
    static readonly Dictionary<int, string> searchChoices = new();
    // Per searched seat: the decision version whose search result is still awaited, and until when.
    static readonly Dictionary<int, (long Version, long Until)> searchPending = new();
    /// <summary>How long after its decision was captured an AI seat waits for that decision's search
    /// before it takes the policy's choice. Plain advice is published first and a search runs only
    /// while no other work waits, so neither a slow search nor other seats' searches hold a seat past
    /// this; with the 0.35–1.6 s presence think it stays inside the 5 s AI fallback (AiFallback.cs).</summary>
    internal const double SearchWaitSeconds = 2;
    /// <summary>The measured default (2 samples, 64 nodes: p95 about 100 ms per combat decision natively).
    /// In the bridge each search runs on its own thread: a panic, a simulator fault or no result within
    /// its 2 s stall limit reports the search unavailable and the policy's choice stands; after a stall
    /// no search runs again until the game restarts.</summary>
    static readonly object SearchConfig = new Dictionary<string, int> { ["samples"] = 2, ["node_budget"] = 64 };
    /// <summary>Seats whose decisions are searched (the host's AI teammates); called on the main thread.
    /// The human's advice is never searched: the badges show the policy's own probabilities.</summary>
    public static Func<int, bool>? SearchesSeat { get; set; }
    /// <summary>The latest timestamp a seat may still act and beat its fallback (main thread). A
    /// pending search never holds the seat past it, so a search wait ends at the earlier of this
    /// and <see cref="SearchWaitSeconds"/> after capture.</summary>
    public static Func<int, long>? SeatDeadline { get; set; }
    static long WaitEnd(int seat, long until) => SeatDeadline is { } deadline ? Math.Min(until, deadline(seat)) : until;
    /// <summary>Diagnostic lines (search turned off, search unavailable); at most once per run and reason.</summary>
    public static Action<string>? Log { get; set; }
    static bool searchAvailable;
    // A routine refusal (a teammate resolving a card choice) must not hide a later stall or fault.
    static readonly HashSet<string> searchFailuresLogged = new(StringComparer.Ordinal);
    // Per seat: option source -> representative of its indistinguishable group.
    static readonly Dictionary<int, Dictionary<object, object>> equivalent = new();
    static BlockingCollection<Action>? queue;
    static CancellationTokenSource? cancellation;
    internal static Task WorkerCompletion { get; private set; } = Task.CompletedTask;
    [ThreadStatic] static IntPtr native;
    [ThreadStatic] static NativeBridge? workerBridge;
    [ThreadStatic] static CancellationToken workerToken;
    // Searches of decisions whose plain advice is out; the worker runs one only when no job waits.
    [ThreadStatic] static Queue<Action>? workerSearches;
    static int localSeat;
    static long runId, revision;

    /// <summary>Call on the game's main thread, once from RecorderMod.Init.</summary>
    public static void Install(Harmony harmony) => Guard(() => {
        ArgumentNullException.ThrowIfNull(harmony);
        main = SynchronizationContext.Current;
        if (main is null) {
            LastRefusal = "MAIN_THREAD_CONTEXT_UNAVAILABLE";
            LastFailureKind = AiFailureKind.Internal;
        }
    });

    static void Notify()
    {
        if (Changed is not { } changed) return;
        foreach (Action subscriber in changed.GetInvocationList())
            try { subscriber(); } catch (Exception) { /* One UI cannot break another or the recorder. */ }
    }

    static void Guard(Action action)
    {
        try { action(); }
        catch (Exception error) {
            runId++;
            var failed = cancellation;
            cancellation = null;
            try { failed?.Cancel(); } catch (Exception) { }
            try { queue?.CompleteAdding(); } catch (Exception) { }
            queue = null;
            Status = AdvisorStatus.Unavailable;
            LastRefusal = error.Message;
            LastFailureKind = AiFailureKind.Internal;
            advice.Clear();
            equivalent.Clear();
            // Badges and status listeners must learn that the advisor stopped.
            Notify();
        }
    }

    static void Post(Action action)
    {
        try { main?.Post(_ => Guard(action), null); }
        catch (Exception) { /* A disposed game context must not escape the advisor worker. */ }
    }

    static void MainThread()
    {
        if (main is null || SynchronizationContext.Current != main)
            throw new InvalidOperationException("Advisor lifecycle and live objects require the installed main context.");
    }
    // A seat's advice is only as old as that seat's own decision: in a 3-4 player
    // party one teammate's new capture must not discard the others' results.
    // Written on the main thread; the worker reads it to skip searches of superseded decisions.
    static readonly ConcurrentDictionary<int, long> seatVersions = new();
    static readonly Dictionary<int, long> requestedVersions = new();
    static long epoch;
    static void Clear()
    {
        advice.Clear();
        searchChoices.Clear();
        searchPending.Clear();
        equivalent.Clear();
        requestedVersions.Clear();
        LastAdvicePath = null;
        revision++;
        epoch++;
        Notify();
    }
    static void ClearSeat(int seat)
    {
        advice.Remove(seat);
        searchChoices.Remove(seat);
        searchPending.Remove(seat);
        equivalent.Remove(seat);
        requestedVersions.Remove(seat);
        seatVersions[seat] = ++revision;
        Notify();
    }
    static void Worker(Action work)
    {
        if (queue is null || cancellation is null || cancellation.IsCancellationRequested) return;
        if (!queue.TryAdd(work)) {
            Status = AdvisorStatus.Unavailable;
            LastRefusal = "ADVISOR_QUEUE_OVERFLOW";
            LastFailureKind = AiFailureKind.Internal;
            Clear();
        }
    }

    /// <summary>Immutable v2 run-start envelope; never call when loading a save mid-run.</summary>
    public static void BeginRun(string runStartJson, string gameVersion, int seat, string cacheDirectory) =>
        Guard(() => BeginRunCore(runStartJson, gameVersion, seat, ct => PolicyCache.Load(cacheDirectory, gameVersion, ct)));

    /// <summary>Offline verification or explicitly offline play: performs no HTTP requests.</summary>
    public static void BeginRunFromCache(string runStartJson, string gameVersion, int seat, string cacheDirectory) =>
        Guard(() => BeginRunCore(runStartJson, gameVersion, seat, ct => PolicyCache.LoadCached(cacheDirectory, gameVersion, ct)));

    static void BeginRunCore(string runStartJson, string gameVersion, int seat, Func<CancellationToken, byte[]> loadWeights)
    {
        MainThread();
        var protectedRecording = recordingFailure;
        EndRun();
        recordingFailure = protectedRecording;
        localSeat = seat;
        long id = runId;
        Status = AdvisorStatus.Downloading;
        LastFailureKind = AiFailureKind.None;
        LastRefusal = null;
        var jobs = new BlockingCollection<Action>(128);
        var lifetime = new CancellationTokenSource();
        var token = lifetime.Token;
        cancellation = lifetime;
        queue = jobs;
        var completion = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        WorkerCompletion = completion.Task;
        var bridge = Bridge;
        var thread = new Thread(() => {
            workerToken = token;
            workerBridge = bridge;
            workerRecordingFailure = protectedRecording;
            var searches = workerSearches = new Queue<Action>();
            try {
                // Every waiting job (a seat's plain advice) runs before the next search, so at
                // most one running search delays another seat's advice.
                while (true) {
                    token.ThrowIfCancellationRequested();
                    if (jobs.TryTake(out var job, searches.Count == 0 ? Timeout.Infinite : 0, token)) {
                        token.ThrowIfCancellationRequested();
                        job();
                    }
                    else if (searches.Count > 0) searches.Dequeue()();
                    else break;
                }
            }
            catch (OperationCanceledException) when (token.IsCancellationRequested) { }
            catch (Exception error) {
                Post(() => {
                    if (id != runId) return;
                    Status = AdvisorStatus.Unavailable;
                    LastFailureKind = AiFailureKind.Internal;
                    LastRefusal = error.Message;
                    advice.Clear();
                    Notify();
                });
            }
            finally {
                try { if (native != IntPtr.Zero) bridge.Destroy(native); }
                catch (Exception) { /* Teardown cannot escape a background thread. */ }
                native = IntPtr.Zero;
                jobs.Dispose();
                lifetime.Dispose();
                completion.TrySetResult();
            }
        }, 16 * 1024 * 1024) { IsBackground = true, Name = "STS2 local advisor" };
        Worker(() => {
            try {
                token.ThrowIfCancellationRequested();
                byte[] weights = loadWeights(token);
                token.ThrowIfCancellationRequested();
                byte[] start = Encoding.UTF8.GetBytes(runStartJson);
                token.ThrowIfCancellationRequested();
                native = bridge.Create(weights, start);
                token.ThrowIfCancellationRequested();
                using var meta = Request("{\"op\":\"meta\"}");
                var value = meta.RootElement.GetProperty("value");
                if (value.GetProperty("game_version").GetString() != gameVersion)
                    throw new InvalidDataException("POLICY_GAME_VERSION_MISMATCH");
                string label = $"{value.GetProperty("checkpoint_id").GetString()} · {value.GetProperty("step").GetUInt64()}";
                // Search discounts with the checkpoint's training gamma; weights exported
                // without it keep every seat on the plain policy.
                bool searchable = value.TryGetProperty("gamma", out var gamma) && gamma.ValueKind == JsonValueKind.Number;
                Post(() => {
                    if (id != runId || token.IsCancellationRequested) return;
                    searchAvailable = searchable;
                    searchFailuresLogged.Clear();
                    if (!searchable)
                        Log?.Invoke($"decision search off: policy {label} lacks the training discount (gamma); AI teammates play the plain policy");
                    CheckpointLabel = label;
                    Status = AdvisorStatus.Ready;
                    LastRefusal = null;
                    LastFailureKind = AiFailureKind.None;
                    Notify();
                    Refresh();
                });
            } catch (OperationCanceledException) when (token.IsCancellationRequested) {
                throw;
            } catch (Exception error) {
                Post(() => {
                    if (id != runId) return;
                    Status = AdvisorStatus.Unavailable;
                    LastRefusal = error.Message;
                    LastFailureKind = error is PolicyLoadException policyError ? policyError.Kind :
                        error is InvalidDataException ? AiFailureKind.CheckpointInvalid : AiFailureKind.Internal;
                    advice.Clear();
                    Notify();
                });
            }
        });
        // Notify before starting so a disable subscriber can cancel even the first job.
        Notify();
        thread.Start();
    }

    public static void EndRun() => Guard(() => {
        MainThread();
        runId++;
        var stopped = cancellation;
        cancellation = null;
        try { stopped?.Cancel(); } catch (Exception) { }
        try { queue?.CompleteAdding(); } catch (ObjectDisposedException) { }
        queue = null;
        live.Clear();
        boundaries.Clear();
        liveStates.Clear();
        unavailable.Clear();
        replayUnavailable = false;
        searchAvailable = false;
        recordingFailure = null;
        Interlocked.Exchange(ref replayIntegrityLost, 0);
        CheckpointLabel = null;
        Status = AdvisorStatus.Unavailable;
        LastFailureKind = AiFailureKind.None;
        LastRefusal = null;
        Clear();
    });

    /// <summary>Supply every seat's independently enumerated live options, not simulator output.</summary>
    public static void SetDecision(int seat, IReadOnlyList<LiveOption> options) => Guard(() => {
        MainThread();
        RefuseDecision(seat, "LIVE_BOUNDARY_REQUIRED");
    });

    /// <summary>Bindings are serialized original RecordedActions, validated through
    /// the original-input folder against this independently captured live boundary.</summary>
    public static void SetDecision(int seat, IReadOnlyList<LiveOption> options,
        ReplayCheckpoint before, int? parentSequence = null) => Guard(() => {
        MainThread();
        live[seat] = options.ToArray();
        liveStates.Remove(seat);
        boundaries[seat] = (before, parentSequence);
        unavailable.Remove(seat);
        ClearSeat(seat);
        Refresh();
    });

    public static void SetLiveDecision(int seat, IReadOnlyList<LiveOption> options,
        LiveObservation.Snapshot snapshot, ReplayCheckpoint? before = null, int? parentSequence = null) => Guard(() => {
        MainThread();
        live[seat] = options.ToArray();
        liveStates[seat] = snapshot;
        unavailable.Remove(seat);
        if (before is not null)
            boundaries[seat] = (before, parentSequence);
        else boundaries.Remove(seat);
        ClearSeat(seat);
        Refresh();
    });
    public static void SetReplayDecision(int seat, IReadOnlyList<LiveOption> options,
        ReplayCheckpoint before, int? parentSequence, string missingLiveReason,
        AdviceReason category = AdviceReason.Other)
    {
        SetDecision(seat, options, before, parentSequence);
        unavailable[seat] = new(category, missingLiveReason);
        Notify();
    }

    public static void UseLiveState() => Guard(() => {
        MainThread();
        MarkReplayIntegrityLost();
        replayUnavailable = true;
        InvalidateDecisions();
    });

    public static Task<JsonElement> CompareLiveObservation(LiveObservation.Snapshot snapshot)
    {
        MainThread();
        var completion = new TaskCompletionSource<JsonElement>(TaskCreationOptions.RunContinuationsAsynchronously);
        string json = JsonSerializer.Serialize(new { op = "live_parity", live_state = snapshot }, RecordingFormat.Json);
        Worker(() => {
            try {
                using var response = Request(json);
                completion.TrySetResult(response.RootElement.GetProperty("value").Clone());
            } catch (Exception error) { completion.TrySetException(error); }
        });
        return completion.Task;
    }

    /// <summary>Call immediately on input confirmation to remove stale advice.</summary>
    public static void InvalidateDecisions() => Guard(() => {
        MainThread();
        live.Clear();
        boundaries.Clear();
        liveStates.Clear();
        unavailable.Clear();
        Clear();
    });

    /// <summary>Revokes one seat's decision when that seat acts; other seats keep theirs
    /// and are revalidated against the live state before they act.</summary>
    public static void InvalidateSeat(int seat) => Guard(() => {
        MainThread();
        live.Remove(seat);
        boundaries.Remove(seat);
        liveStates.Remove(seat);
        unavailable.Remove(seat);
        ClearSeat(seat);
    });

    public static void RefuseDecision(int seat, string reason, AdviceReason category = AdviceReason.Other) => Guard(() => {
        MainThread();
        live.Remove(seat);
        boundaries.Remove(seat);
        liveStates.Remove(seat);
        advice.Remove(seat);
        searchChoices.Remove(seat);
        searchPending.Remove(seat);
        equivalent.Remove(seat);
        requestedVersions.Remove(seat);
        seatVersions[seat] = ++revision;
        LastRefusal = reason;
        lastRefusalReason = category;
        unavailable[seat] = new(category, reason);
        LastFailureKind = AiFailureKind.Unsupported;
        Notify();
        Refresh();
    });

    /// <summary>Original ActionRow JSON, in contiguous global input sequence across all seats.</summary>
    public static void ConsumeOriginalInput(string rowJson) => Guard(() => {
        MainThread();
        InvalidateDecisions();
        if (queue is null) return;
        long id = runId;
        Worker(() => {
            try {
                using var row = JsonDocument.Parse(rowJson);
                using var response = Request(JsonSerializer.Serialize(new { op = "consume", row = row.RootElement }));
            } catch (Exception error) {
                Post(() => {
                    if (id != runId) return;
                    MarkReplayIntegrityLost();
                    replayUnavailable = true;
                    Status = AdvisorStatus.Ready;
                    LastRefusal = error.Message;
                    LastFailureKind = AiFailureKind.Unsupported;
                    advice.Clear();
                    Notify();
                });
            }
        });
    });

    static void Refresh()
    {
        if (queue is null || Status != AdvisorStatus.Ready || live.Count == 0) return;
        long id = runId, capturedEpoch = epoch;
        // Only seats whose current decision has no result and no request in flight.
        var pendingSeats = live.Keys.Where(seat => !advice.ContainsKey(seat) &&
            requestedVersions.GetValueOrDefault(seat, -1) != seatVersions.GetValueOrDefault(seat)).ToArray();
        if (pendingSeats.Length == 0) return;
        var versions = pendingSeats.ToDictionary(seat => seat, seat => seatVersions.GetValueOrDefault(seat));
        foreach (var (seat, seatVersion) in versions) requestedVersions[seat] = seatVersion;
        var snapshots = live.Where(p => versions.ContainsKey(p.Key)).ToDictionary(p => p.Key, p => p.Value);
        var capturedBoundaries = boundaries.ToDictionary(p => p.Key, p => p.Value);
        var capturedStates = liveStates.ToDictionary(p => p.Key, p => p.Value);
        bool preferLive = replayUnavailable;
        var searchedSeats = pendingSeats.Where(seat => searchAvailable && SearchesSeat?.Invoke(seat) == true).ToHashSet();
        // Measured from capture: a queue delay counts against the seat's wait for its search.
        long searchUntil = System.Diagnostics.Stopwatch.GetTimestamp() +
            (long)(System.Diagnostics.Stopwatch.Frequency * SearchWaitSeconds);
        string Json(int seat, LiveOption[] options, bool search) => JsonSerializer.Serialize(new {
            op = "checked_pending",
            prefer_live = preferLive,
            search = search ? SearchConfig : null,
            live_state = capturedStates.GetValueOrDefault(seat),
            decisions = new[] { new {
                seat,
                before = capturedBoundaries.TryGetValue(seat, out var boundary) ? boundary.Before : null,
                parent_seq = capturedBoundaries.TryGetValue(seat, out boundary) ? boundary.Parent : null,
                options = options.Select(o => o.Binding).ToArray()
            } }
        }, RecordingFormat.Json);
        Worker(() => {
        // The policy's advice for every seat first; searches follow (SearchSeat).
        var requests = snapshots.Where(p => capturedBoundaries.ContainsKey(p.Key) || capturedStates.ContainsKey(p.Key))
            .Select(p => (Seat: p.Key, Json: Json(p.Key, p.Value, search: false))).ToArray();
            var result = new Dictionary<int, IReadOnlyList<(object Option, float Probability)>>();
            var groups = new Dictionary<int, Dictionary<object, object>>();
            string? selectedPath = null;
            string? refusal = null;
            AiFailureKind failure = AiFailureKind.None;
            foreach (var request in requests) {
              workerToken.ThrowIfCancellationRequested();
              try {
                using var response = Request(request.Json);
                foreach (var seat in response.RootElement.GetProperty("value").EnumerateArray()) {
                    int index = seat.GetProperty("seat").GetInt32();
                    if (index != request.Seat) throw new InvalidDataException("NATIVE_SEAT_MISMATCH");
                    if (!snapshots.TryGetValue(index, out var options) || options.Length == 0) continue;
                    selectedPath = seat.TryGetProperty("path", out var path) ? path.GetString() : "replay";
                    if (selectedPath == "live") MarkReplayIntegrityLost();
                    var nativeOptions = seat.GetProperty("options").EnumerateArray().ToArray();
                    var keys = options.Select(o => o.Binding).ToHashSet(StringComparer.Ordinal);
                    if (keys.Count != options.Length || nativeOptions.Length != keys.Count ||
                        nativeOptions.Select(o => o.GetProperty("binding").GetString()!).Distinct(StringComparer.Ordinal).Count() != keys.Count ||
                        nativeOptions.Any(o => !keys.Contains(o.GetProperty("binding").GetString()!)) ||
                        options.Where((o, i) => options.Take(i).Any(previous => SameAction(previous.Joint, o.Joint))).Any()) {
                        refusal = $"LEGAL_SET_MISMATCH seat={index}";
                        failure = AiFailureKind.Unsupported;
                        continue;
                    }
                    var probabilities = nativeOptions.ToDictionary(o => o.GetProperty("binding").GetString()!,
                        o => o.GetProperty("probability").GetSingle(), StringComparer.Ordinal);
                    if (probabilities.Values.Any(p => !float.IsFinite(p) || p < 0 || p > 1) ||
                        Math.Abs(probabilities.Values.Sum(p => (double)p) - 1) > 1e-5)
                        throw new InvalidDataException("INVALID_POLICY_DISTRIBUTION");
                    result[index] = Array.AsReadOnly(options.Select(o => ((object)o.Joint, probabilities[o.Binding])).ToArray());
                    // Grouping reads only the immutable captured snapshot; keep it off the game thread.
                    if (capturedStates.TryGetValue(index, out var state)) groups[index] = Equivalence(options, state);
                    else groups.Remove(index);
                }
              } catch (OperationCanceledException) when (workerToken.IsCancellationRequested) { throw; }
              catch (Exception error) {
                  result.Remove(request.Seat);
                  refusal = error.Message;
                  failure = error is InvalidDataException ? AiFailureKind.Unsupported : AiFailureKind.Internal;
              }
            }
            workerToken.ThrowIfCancellationRequested();
            Post(() => {
                if (id != runId || capturedEpoch != epoch) return;
                foreach (var pair in result) {
                    // A newer decision for this seat supersedes this result.
                    if (seatVersions.GetValueOrDefault(pair.Key) != versions[pair.Key]) continue;
                    advice[pair.Key] = pair.Value;
                    searchChoices.Remove(pair.Key);
                    if (searchedSeats.Contains(pair.Key)) searchPending[pair.Key] = (versions[pair.Key], searchUntil);
                    else searchPending.Remove(pair.Key);
                    if (groups.TryGetValue(pair.Key, out var grouped)) equivalent[pair.Key] = grouped;
                    else equivalent.Remove(pair.Key);
                }
                LastRefusal = refusal;
                LastFailureKind = failure;
                if (selectedPath is not null) LastAdvicePath = selectedPath;
                Notify();
            });
            foreach (int seat in result.Keys.Where(searchedSeats.Contains)) {
                string json = Json(seat, snapshots[seat], search: true);
                var keys = snapshots[seat].Select(o => o.Binding).ToHashSet(StringComparer.Ordinal);
                workerSearches!.Enqueue(() => SearchSeat(id, capturedEpoch, seat, versions[seat], searchUntil, json, keys));
            }
        });
    }

    /// <summary>On the worker: one seat's search after its plain advice is out. Search is advisory to
    /// the AI seat only: a missing, failed or late search leaves the policy's choice.</summary>
    static void SearchSeat(long id, long capturedEpoch, int seat, long version, long until, string json, HashSet<string> keys)
    {
        // Superseded, or the seat has stopped waiting for it: nothing would use the result.
        if (Volatile.Read(ref epoch) != capturedEpoch || seatVersions.GetValueOrDefault(seat) != version ||
            System.Diagnostics.Stopwatch.GetTimestamp() >= until) return;
        string? choice = null, searchFailure = null;
        try {
            using var response = Request(json);
            foreach (var entry in response.RootElement.GetProperty("value").EnumerateArray()) {
                if (entry.GetProperty("seat").GetInt32() != seat) throw new InvalidDataException("NATIVE_SEAT_MISMATCH");
                if (!entry.TryGetProperty("search", out var search) || search.ValueKind != JsonValueKind.Object) continue;
                string? reason = search.TryGetProperty("reason", out var why) ? why.GetString() : null;
                if (reason == "searched" && search.TryGetProperty("binding", out var chosen) &&
                    chosen.ValueKind == JsonValueKind.String && keys.Contains(chosen.GetString()!))
                    choice = chosen.GetString()!;
                else if (reason == "unavailable")
                    searchFailure = search.TryGetProperty("error", out var error) ? error.ToString() : "unknown";
            }
        } catch (OperationCanceledException) when (workerToken.IsCancellationRequested) { throw; }
        catch (Exception error) { searchFailure = error.Message; }
        Post(() => {
            if (id != runId || capturedEpoch != epoch || seatVersions.GetValueOrDefault(seat) != version) return;
            // Once the seat stopped waiting it may already be acting on the policy's choice.
            if (!searchPending.Remove(seat, out var pending)) return;
            // Past the wait the seat may act on the policy's choice before AwaitingSearch polls.
            if (System.Diagnostics.Stopwatch.GetTimestamp() >= WaitEnd(seat, pending.Until)) {
                SearchWaitExpired();
                Notify();
                return;
            }
            if (choice is not null) searchChoices[seat] = choice;
            if (searchFailure is not null && searchFailuresLogged.Add(searchFailure))
                Log?.Invoke("decision search unavailable (" + searchFailure + "); the AI teammate keeps the policy's choice");
            Notify();
        });
    }

    static JsonDocument Request(string json)
    {
        workerToken.ThrowIfCancellationRequested();
        if (native == IntPtr.Zero) throw new InvalidOperationException("NATIVE_INITIALIZATION_FAILED");
        byte[] request = Encoding.UTF8.GetBytes(json);
        var response = JsonDocument.Parse(workerBridge!.Request(native, request) ?? throw new InvalidDataException("NATIVE_RESPONSE_MISSING"));
        if (response.RootElement.TryGetProperty("replay_clean", out var clean) && clean.ValueKind == JsonValueKind.False)
            MarkReplayIntegrityLost();
        if (workerToken.IsCancellationRequested) {
            response.Dispose();
            workerToken.ThrowIfCancellationRequested();
        }
        if (response.RootElement.GetProperty("ok").GetBoolean()) return response;
        string error = response.RootElement.GetProperty("error").GetRawText();
        response.Dispose();
        throw new InvalidDataException(error);
    }

    static void MarkReplayIntegrityLost()
    {
        try { (workerRecordingFailure ?? recordingFailure)?.Invoke(); } catch (Exception) { }
        if (Interlocked.Exchange(ref replayIntegrityLost, 1) != 0) return;
        foreach (var subscriber in ReplayIntegrityLost?.GetInvocationList() ?? [])
            try { ((Action)subscriber)(); } catch (Exception) { }
    }

    public static Task DrainAsync()
    {
        MainThread();
        if (queue is null) return Task.CompletedTask;
        var done = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var token = cancellation!.Token;
        var registration = token.Register(() => done.TrySetCanceled(token));
        _ = done.Task.ContinueWith(_ => registration.Dispose(), TaskScheduler.Default);
        Worker(() => Post(() => done.TrySetResult()));
        return done.Task;
    }

    /// <summary>The bridge's entry points; verification substitutes a scripted bridge. Each run's
    /// worker keeps the bridge it started with.</summary>
    internal sealed record NativeBridge(Func<byte[], byte[], IntPtr> Create, Func<IntPtr, byte[], string?> Request,
        Action<IntPtr> Destroy);
    internal static NativeBridge Bridge { get; set; } = new(
        (weights, start) => Native.Create(weights, (nuint)weights.Length, start, (nuint)start.Length),
        (handle, request) => Marshal.PtrToStringUTF8(Native.Request(handle, request, (nuint)request.Length)),
        Native.Destroy);

    static class Native
    {
        [DllImport("sts2_advisor_bridge", EntryPoint = "sts2_advisor_create", CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr Create(byte[] weights, nuint length, byte[] start, nuint startLength);
        [DllImport("sts2_advisor_bridge", EntryPoint = "sts2_advisor_request", CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr Request(IntPtr handle, byte[] request, nuint length);
        [DllImport("sts2_advisor_bridge", EntryPoint = "sts2_advisor_destroy", CallingConvention = CallingConvention.Cdecl)]
        internal static extern void Destroy(IntPtr handle);
    }

    /// <summary>
    /// The policy's probability (0..1) for <paramref name="option"/> in the local player's
    /// current decision. <paramref name="option"/> is the game object the game itself
    /// passes when that option is confirmed (CardModel, RelicModel, PotionModel, MapPoint,
    /// event option, rest site option, …) or one of <see cref="Options"/>. Null when there
    /// is no advice: the policy is not ready, no options are offered, or a live
    /// decision snapshot could not be captured.
    /// </summary>
    /// <remarks>Copies the observation cannot tell apart (identical cards that differ only
    /// in hand position) are one choice to the player; the policy splits its mass across
    /// them arbitrarily, so their probabilities are summed and shown on every copy.</remarks>
    public static float? ProbabilityFor(object option)
    {
        var choices = AdviceForSeat(localSeat);
        if (choices is null) return null;
        if (option is ActionOption joint)
            return ExactProbability(choices, joint.Source, joint.Action, joint.Target);
        object group = Group(option);
        double sum = 0;
        bool found = false;
        foreach (var (candidate, probability) in choices)
            if (candidate is ActionOption action && ReferenceEquals(Group(action.Source), group)) {
                found = true;
                sum += probability;
            }
        return found ? (float)sum : null;
    }

    static object Group(object source) =>
        equivalent.TryGetValue(localSeat, out var groups) && groups.TryGetValue(source, out var group) ? group : source;

    static readonly HashSet<string> PositionalNumbers = ["hand_order", "slot"];

    /// <summary>Groups sources whose observed rows, child rows and candidates are identical
    /// apart from position, i.e. exactly what the policy itself can distinguish.</summary>
    static Dictionary<object, object> Equivalence(LiveOption[] options, LiveObservation.Snapshot snapshot)
    {
        var groups = new Dictionary<object, object>(ReferenceEqualityComparer.Instance);
        var byBinding = new Dictionary<string, LiveObservation.Option>(StringComparer.Ordinal);
        foreach (var option in snapshot.options) byBinding.TryAdd(option.binding, option);
        // A row that is the source of several different options (the player's row for end
        // turn and every potion) stands for distinct actions, not copies of one choice.
        var sourceOf = new Dictionary<int, object>();
        var shared = new HashSet<int>();
        foreach (var live in options)
            if (byBinding.TryGetValue(live.Binding, out var option) &&
                !sourceOf.TryAdd(option.source, live.Option) && !ReferenceEquals(sourceOf[option.source], live.Option))
                shared.Add(option.source);
        var childRows = snapshot.rows.Select((row, index) => (row.parent, handle: index + 1))
            .Where(c => c.parent != c.handle).ToLookup(c => c.parent, c => c.handle);
        var candidateRows = snapshot.options.ToLookup(o => o.source);
        var keys = new Dictionary<int, string>();
        var representatives = new Dictionary<string, object>(StringComparer.Ordinal);
        // A child's parent is the row being keyed itself; only its content distinguishes.
        string Row(int handle, bool nested = false)
        {
            if (!nested && keys.TryGetValue(handle, out var known)) return known;
            var row = snapshot.rows[handle - 1];
            var children = childRows[handle].Select(child => Row(child, true))
                .OrderBy(k => k, StringComparer.Ordinal).ToArray();
            var candidates = candidateRows[handle]
                .Select(o => JsonSerializer.Serialize(new { o.action, o.target, o.model, o.numbers, o.flags, o.legal }))
                .OrderBy(k => k, StringComparer.Ordinal).ToArray();
            string key = JsonSerializer.Serialize(new {
                row.kind, row.model, row.color, row.detail, row.owner, parent = nested ? 0 : row.parent,
                numbers = row.numbers.Where(n => !PositionalNumbers.Contains(n.Key)), row.floats, row.flags,
                children, candidates
            });
            if (!nested) keys[handle] = key;
            return key;
        }
        foreach (var live in options) {
            if (!byBinding.TryGetValue(live.Binding, out var option) || option.source <= 0 ||
                option.source > snapshot.rows.Count || shared.Contains(option.source)) continue;
            string key = Row(option.source);
            if (!representatives.TryGetValue(key, out var representative))
                representatives[key] = representative = live.Option;
            groups.TryAdd(live.Option, representative);
        }
        return groups;
    }

    /// <summary>Exact joint probability, not a target-conditional renormalization.
    /// Null if the source/target pair identifies more than one action verb.</summary>
    public static float? ProbabilityFor(object option, object? target) =>
        AdviceForSeat(localSeat) is { } choices ? ExactProbability(choices, option, null, target) : null;

    /// <summary>Disambiguates verbs such as using versus discarding the same potion.</summary>
    public static float? ProbabilityFor(object option, DecisionAction action, object? target) =>
        AdviceForSeat(localSeat) is { } choices ? ExactProbability(choices, option, action, target) : null;

    static float? ExactProbability(IReadOnlyList<(object Option, float Probability)> choices,
        object source, DecisionAction? verb, object? target)
    {
        // Ambiguity is per copy (one source matching several verbs); copies are summed.
        object group = Group(source);
        var perSource = new Dictionary<object, int>(ReferenceEqualityComparer.Instance);
        float? result = null;
        foreach (var (candidate, probability) in choices)
            if (candidate is ActionOption action && ReferenceEquals(Group(action.Source), group) &&
                ReferenceEquals(action.Target, target) && (verb is null || action.Action == verb)) {
                int seen = perSource.GetValueOrDefault(action.Source) + 1;
                if (seen > 1) return null;
                perSource[action.Source] = seen;
                result = (result ?? 0) + probability;
            }
        return result;
    }

    static bool SameAction(ActionOption left, ActionOption right) =>
        ReferenceEquals(left.Source, right.Source) && left.Action == right.Action &&
        ReferenceEquals(left.Target, right.Target);

    /// <summary>True for the single option with the highest probability in the current decision.</summary>
    public static bool IsTopPick(object option)
    {
        if (AdviceForSeat(localSeat) is not { Count: > 0 } choices) return false;
        if (option is ActionOption joint) {
            // Rank joint actions with indistinguishable copies merged.
            var top = choices.Select(p => (ActionOption)p.Option)
                .GroupBy(a => (Group: Group(a.Source), a.Action, a.Target))
                .OrderByDescending(g => choices.Where(p => g.Contains((ActionOption)p.Option)).Sum(p => (double)p.Probability))
                .First().Key;
            return ReferenceEquals(top.Group, Group(joint.Source)) && top.Action == joint.Action &&
                ReferenceEquals(top.Target, joint.Target);
        }
        var groups = choices.Select(p => Group(((ActionOption)p.Option).Source))
            .Distinct(ReferenceEqualityComparer.Instance);
        return ReferenceEquals(groups.OrderByDescending(group => ProbabilityFor(group)).First(), Group(option));
    }

    /// <summary>Exact joint advice for any seat. Each Option is an ActionOption;
    /// Source and Target are the live game objects used for execution.</summary>
    public static IReadOnlyList<(object Option, float Probability)>? AdviceForSeat(int seat) =>
        Status == AdvisorStatus.Ready && advice.TryGetValue(seat, out var choices) ? choices : null;

    /// <summary>The option binding decision-time search chose for the seat's current advice
    /// (combat decisions of <see cref="SearchesSeat"/> seats only); null otherwise.</summary>
    public static string? SearchChoiceForSeat(int seat) =>
        AdviceForSeat(seat) is not null ? searchChoices.GetValueOrDefault(seat) : null;

    /// <summary>True while the seat's current decision still waits for its search: the policy's
    /// advice is out, the search result is not, and <see cref="SearchWaitSeconds"/> since capture
    /// have not passed. Afterwards the seat takes the policy's choice and a late result is dropped.</summary>
    /// <see cref="SeatDeadline"/> ends the wait earlier, both here and when the result is applied.
    public static bool AwaitingSearch(int seat)
    {
        if (!searchPending.TryGetValue(seat, out var pending)) return false;
        if (System.Diagnostics.Stopwatch.GetTimestamp() < WaitEnd(seat, pending.Until)) return true;
        searchPending.Remove(seat);
        SearchWaitExpired();
        return false;
    }
    static void SearchWaitExpired()
    {
        if (searchFailuresLogged.Add("SEARCH_WAIT"))
            Log?.Invoke($"decision search unavailable (no result within the AI seat's {SearchWaitSeconds} s wait); the AI teammate keeps the policy's choice");
    }

    sealed class Sentinel(string name)
    {
        public override string ToString() => name;
    }
}
