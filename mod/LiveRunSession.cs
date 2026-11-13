using System.Security.Cryptography;
using System.Reflection;

namespace HumanPlayRecorder;

// The game thread supplies only immutable DTO snapshots. All hashing, file
// persistence, validation and HTTP handoff are performed on a worker.
internal sealed class LiveRunSession
{
    readonly Task<RunHeader> header;
    readonly List<ActionRow> actions = new();
    readonly List<FloorDigest> floors = new();
    readonly UploadQueue queue;
    readonly Action<Exception>? failureSink;
    readonly Action<string>? log;
    bool terminal, discarded;
    public bool Discarded => discarded;
    public Task Completion { get; private set; } = Task.CompletedTask;
    public Exception? Failure { get; private set; }
    public LiveRunSession(RunHeader provisional, string gameDll,
        (string id, string version, string dll)[] mods, UploadQueue queue,
        Action<Exception>? failureSink = null, Action<string>? log = null)
    {
        this.queue = queue;
        this.failureSink = failureSink;
        this.log = log;
        header = Task.Run(() => provisional with {
            game_dll_sha256 = HashFile(gameDll),
            active_mods = mods.Select(m => new ModRef(m.id, m.version, HashFile(m.dll))).ToArray()
        });
    }
    static string HashFile(string path)
    {
        using var stream = File.OpenRead(path);
        return Convert.ToHexStringLower(SHA256.HashData(stream));
    }
    public void Add(ActionRow row)
    {
        if (discarded) return;
        if (terminal) throw new InvalidOperationException("action after terminal capture");
        actions.Add(row);
    }
    // The journal accepts a row only while the capture is valid; when it refuses (sharing was switched off,
    // a hole, a failed write) the in-memory recording is dropped instead of growing on without its journal.
    public bool Record(LocalJournal journal, ActionRow row)
    {
        if (discarded) return false;
        if (!journal.Append(row)) { Discard(); return false; }
        Add(row);
        return true;
    }
    // Stops and forgets this recording for good: nothing captured so far is kept or queued, and later rows
    // are ignored. (What the AI guide observes for its advice is separate and never goes through here.)
    public void Discard()
    {
        discarded = true;
        actions.Clear();
        floors.Clear();
    }
    public void Floor(StateDigest state, int afterAction)
    {
        if (terminal || discarded) return;
        if (floors.Count != 0 && floors[^1].floor == state.floor) return;
        floors.Add(new(state.act, state.floor, afterAction, state, RecordingFormat.Hash(state)));
    }
    public void End(LocalJournal journal, RunResult result, string endedAt, bool complete,PatchAuditEntry[]? finalAudit=null)
    {
        if (terminal) return;
        terminal = true;
        if (discarded)
        {
            Completion = journal.DisposeAsync().AsTask();
            return;
        }
        var captured = actions.ToArray();
        var capturedFloors = floors.ToArray();
        Completion = Task.Run(async () =>
        {
            try
            {
                await journal.FinishForHandoffAsync();
                // Frozen snapshot plus journal completion: channel overflow,
                // disk errors and unsupported adapters must never upload.
                if (!complete || !journal.Complete || journal.Failure is not null)
                    throw new InvalidOperationException("incomplete capture retained locally");
                var completedHeader=await header;
                if(finalAudit is not null)completedHeader=completedHeader with {
                    patch_audit=(completedHeader.patch_audit??[]).Concat(finalAudit).Distinct()
                        .OrderBy(p=>p.mod_id,StringComparer.Ordinal).ThenBy(p=>p.method,StringComparer.Ordinal)
                        .ThenBy(p=>p.kind,StringComparer.Ordinal).ToArray()
                };
                await CompletedRun.QueueAsync(queue, completedHeader, captured,
                    capturedFloors, result, endedAt, completeCapture: true, log: log, journal: journal);
            }
            catch (Exception e)
            {
                Failure = e;
                Console.Error.WriteLine("[HumanPlayRecorder] run retained locally: " + e.Message);
                failureSink?.Invoke(e);
            }
            finally { await journal.DisposeAsync(); }
        });
    }
}
