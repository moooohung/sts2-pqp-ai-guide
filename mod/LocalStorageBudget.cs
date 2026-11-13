using System.Collections.Concurrent;
using System.Text.Json;

namespace HumanPlayRecorder;

// Pending: queued for upload, or a staged handoff the next launch queues. Retained: a kept journal with no
// payload in any queue (none was queued, or the server refused it); it cannot upload and only the player removes it.
public enum LocalRunState { Pending, Uncertified, Oversize, Retained }
public sealed record LocalRun(string RunId, LocalRunState State, long Bytes, DateTime UpdatedUtc,
    string? Character, int? Ascension, int? PartySize);

// One filesystem budget shared by all journals, queue bodies and receipts.
// The producer never waits for this lock; only journal/queue workers do.
// Journals of runs that never reached a queue (crashed, abandoned, uncertified) are
// stale: they expire after StaleDays and are evicted oldest-first whenever the budget is
// exceeded. An open run journal and the upload schedule are essential: budget pressure never
// blocks them, so an install that is already over budget still records its next run.
public sealed class LocalStorageBudget
{
    internal static readonly byte[] RetentionFooter = System.Text.Encoding.UTF8.GetBytes("{\"kind\":\"handoff_retained\"}\n");
    internal static bool HasRetentionFooter(string path)
    {
        using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
        if (stream.Length < RetentionFooter.Length) return false;
        stream.Seek(-RetentionFooter.Length, SeekOrigin.End);
        var tail = new byte[RetentionFooter.Length];
        stream.ReadExactly(tail);
        return tail.AsSpan().SequenceEqual(RetentionFooter);
    }
    public const long DefaultMaxBytes = 256L * 1024 * 1024;
    public const int DefaultMaxFiles = 1024;
    public const int StaleDays = 7;
    static readonly ConcurrentDictionary<string, SemaphoreSlim> Gates = new(StringComparer.OrdinalIgnoreCase);
    static readonly ConcurrentDictionary<string, FileStream> OpenJournals = new(StringComparer.OrdinalIgnoreCase);
    readonly SemaphoreSlim gate;
    readonly long maxBytes;
    readonly int maxFiles;
    public string Root { get; }
    public LocalStorageBudget(string root, long maxBytes = DefaultMaxBytes, int maxFiles = DefaultMaxFiles)
    {
        Root = Path.GetFullPath(root);
        this.maxBytes = maxBytes; this.maxFiles = maxFiles;
        gate = Gates.GetOrAdd(Root, _ => new SemaphoreSlim(1, 1));
    }
    static FileStream OwnJournal(string path)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        return new FileStream(path + ".lock", FileMode.OpenOrCreate, FileAccess.ReadWrite,
            FileShare.None, 1, FileOptions.DeleteOnClose);
    }
    public void OpenJournal(string path)
    {
        path = Path.GetFullPath(path);
        var owner = OwnJournal(path);
        if (!OpenJournals.TryAdd(path, owner))
        {
            owner.Dispose();
            throw new IOException("journal already owned");
        }
    }
    public void CloseJournal(string path)
    {
        path = Path.GetFullPath(path);
        if (OpenJournals.TryRemove(path, out var owner))
        {
            try { ApplyJournalDeletion(path); }
            finally { owner.Dispose(); }
        }
    }
    static void ApplyJournalDeletion(string path)
    {
        if (!File.Exists(path + ".delete")) return;
        File.Delete(path);
        File.Delete(path + ".retained");
        File.Delete(path + ".rejected");
        File.Delete(path + ServerRejectedSuffix);
        File.Delete(path + ".payload");
        File.Delete(path + ".payload.partial");
        foreach (var payload in Directory.GetFiles(Path.GetDirectoryName(path)!, Path.GetFileName(path) + ".*.payload*"))
            File.Delete(payload);
        File.Delete(path + ".delete");
    }
    public async Task DeleteAllJournalsAsync()
    {
        await gate.WaitAsync();
        try
        {
            string incomplete = Path.Combine(Root, "incomplete");
            if (!Directory.Exists(incomplete)) return;
            var paths = Directory.GetFiles(incomplete, "*.jsonl*").Select(p =>
                p[..(p.IndexOf(".jsonl", StringComparison.Ordinal) + 6)]).Distinct(StringComparer.OrdinalIgnoreCase);
            foreach (string path in paths)
            {
                using (var marker = new FileStream(path + ".delete", FileMode.Create,
                    FileAccess.Write, FileShare.Read, 1, FileOptions.WriteThrough)) marker.Flush(true);
                try
                {
                    using var owner = OwnJournal(path);
                    ApplyJournalDeletion(path);
                }
                catch (IOException) { /* The active owner's close consumes the durable deletion request. */ }
            }
        }
        finally { gate.Release(); }
    }
    public async Task WriteAsync(string path, byte[] bytes, bool append, CancellationToken ct = default, bool essential = false)
    {
        path = Path.GetFullPath(path);
        if (!path.StartsWith(Root + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
            throw new ArgumentException("storage path escapes budget");
        await gate.WaitAsync(ct);
        try
        {
            if (path.Contains(Path.DirectorySeparatorChar + "incomplete" + Path.DirectorySeparatorChar) &&
                File.Exists(path + ".delete")) throw new InvalidOperationException("capture_deleted");
            if (!Reclaim(path, bytes.Length) && !essential && !OpenJournals.ContainsKey(path))
                throw new InvalidOperationException("queue_full");
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            await using var stream = new FileStream(path, append ? FileMode.Append : FileMode.CreateNew,
                FileAccess.Write, FileShare.Read, 65536, FileOptions.Asynchronous | FileOptions.WriteThrough);
            await stream.WriteAsync(bytes, ct);
            await stream.FlushAsync(ct);
        }
        finally { gate.Release(); }
    }
    // Brings an install back within budget without writing (first launch of a new version).
    public async Task CompactAsync(CancellationToken ct = default)
    {
        await gate.WaitAsync(ct);
        try { Reclaim(null, 0); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        { Console.Error.WriteLine("[HumanPlayRecorder] storage compaction failed: " + e.GetType().Name); }
        finally { gate.Release(); }
    }
    // Under the gate: expires rejected/partial files and stale journals, then evicts stale journals oldest-first.
    // Oversize captures and retained terminal journals are never automatic eviction candidates.
    // until `target` gains `adding` bytes within budget. True when it fits.
    bool Reclaim(string? target, long adding)
    {
        string incomplete = Path.Combine(Root, "incomplete");
        if (Directory.Exists(incomplete))
            foreach (var marker in Directory.GetFiles(incomplete, "*.jsonl.delete"))
            {
                string path = marker[..^7];
                try { using var owner = OwnJournal(path); ApplyJournalDeletion(path); }
                catch (IOException) { }
            }
        var files = Directory.Exists(Root) ? Directory.GetFiles(Root, "*", SearchOption.AllDirectories) : [];
        string journals = Path.Combine(Root, "incomplete");
        var queued = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (string file in files)
        {
            if (string.Equals(Path.GetDirectoryName(file), journals, StringComparison.OrdinalIgnoreCase)) continue;
            string name = Path.GetFileName(file);
            if (name.EndsWith(".json", StringComparison.Ordinal)) queued.Add(name[..^5]);
            else if (name.EndsWith(".json.receipt", StringComparison.Ordinal)) queued.Add(name[..^13]);
            else if (name.EndsWith(".json.oversize", StringComparison.Ordinal)) queued.Add(name[..^14]);
        }
        var expiry = DateTime.UtcNow.AddDays(-StaleDays);
        var stale = new List<FileInfo>();
        long used = 0; int count = 0;
        foreach (string file in files)
        {
            var info = new FileInfo(file);
            if (!info.Exists) continue;
            bool quarantine = file.EndsWith(".rejected", StringComparison.Ordinal) ||
                file.EndsWith(".partial", StringComparison.Ordinal);
            bool staleJournal = file.EndsWith(".jsonl", StringComparison.Ordinal) &&
                string.Equals(Path.GetDirectoryName(file), journals, StringComparison.OrdinalIgnoreCase) &&
                !string.Equals(file, target, StringComparison.OrdinalIgnoreCase) && !OpenJournals.ContainsKey(file) &&
                !queued.Contains(Path.GetFileNameWithoutExtension(file)) && !HasRetentionFooter(file);
            if ((quarantine || staleJournal) && info.LastWriteTimeUtc < expiry && TryDelete(info, staleJournal)) continue;
            if (staleJournal) stale.Add(info);
            count++; used = checked(used + info.Length);
        }
        int adds = target is null || File.Exists(target) ? 0 : 1;
        foreach (var old in stale.OrderBy(f => f.LastWriteTimeUtc).ThenBy(f => f.Name, StringComparer.Ordinal))
        {
            if (used + adding <= maxBytes && count + adds <= maxFiles) break;
            long length = old.Length;
            if (TryDelete(old, journal: true)) { used -= length; count--; }
        }
        return used + adding <= maxBytes && count + adds <= maxFiles;
    }
    // Hold the same ownership lock through deletion, not just while checking it: the data
    // stream is deliberately closed between appends, and another process can own it then.
    static bool TryDelete(FileInfo file, bool journal = false)
    {
        try
        {
            using var owner = journal ? OwnJournal(file.FullName) : null;
            file.Delete();
            return true;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { return false; }
    }
    internal static bool IsRunId(string id) => Guid.TryParseExact(id, "D", out var g) && g.ToString() == id;
    // Every data file of a run on this PC: its journal family in incomplete/ and its files in the queue
    // folders. Ownership locks are not data; other folders' files (policy cache) never match a run id.
    IEnumerable<(string Id, string Path)> RunFiles()
    {
        if (!Directory.Exists(Root)) yield break;
        string journals = Path.Combine(Root, "incomplete");
        foreach (string dir in Directory.GetDirectories(Root))
        {
            string mark = string.Equals(dir, journals, StringComparison.OrdinalIgnoreCase) ? ".jsonl" : ".json";
            foreach (string file in Directory.GetFiles(dir))
            {
                string name = Path.GetFileName(file);
                if (name.Length < 36 || !IsRunId(name[..36])) continue;
                string rest = name[36..];
                if ((rest == mark || rest.StartsWith(mark + ".", StringComparison.Ordinal)) &&
                    !rest.EndsWith(".lock", StringComparison.Ordinal))
                    yield return (name[..36], file);
            }
        }
    }
    static bool Retained(FileInfo file)
    {
        if (file.Name.EndsWith(".jsonl.retained", StringComparison.Ordinal)) return true;
        try { return file.Name.EndsWith(".jsonl", StringComparison.Ordinal) && HasRetentionFooter(file.FullName); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException) { return false; }
    }
    // The local storage window's list, newest first. Plain journals (a run being recorded, or a stale one the
    // budget reclaims by itself) and quarantined rejects are not listed.
    public IReadOnlyList<LocalRun> ListRuns()
    {
        var runs = new List<LocalRun>();
        foreach (var group in RunFiles().GroupBy(f => f.Id, StringComparer.Ordinal))
        {
            var files = group.Select(f => new FileInfo(f.Path)).Where(f => f.Exists).ToList();
            FileInfo? Find(string suffix) => files.FirstOrDefault(f => f.Name.EndsWith(suffix, StringComparison.Ordinal));
            var oversize = Find(".json.oversize");
            var body = Find(".json");
            var staged = Find(".payload");
            var journal = files.FirstOrDefault(Retained);
            LocalRunState? state =
                oversize is not null ? LocalRunState.Oversize :
                body is not null ? (string.Equals(body.Directory!.Name, "uncertified", StringComparison.OrdinalIgnoreCase)
                    ? LocalRunState.Uncertified : LocalRunState.Pending) :
                staged is not null ? (staged.Name.EndsWith(".uncertified.payload", StringComparison.Ordinal)
                    ? LocalRunState.Uncertified : LocalRunState.Pending) :
                journal is not null ? LocalRunState.Retained : null;
            if (state is null) continue;
            var about = Peek(journal ?? body ?? oversize ?? staged!);
            runs.Add(new(group.Key, state.Value, files.Sum(f => f.Length), files.Max(f => f.LastWriteTimeUtc),
                about.Character, about.Ascension, about.PartySize));
        }
        return runs.OrderByDescending(r => r.UpdatedUtc).ThenBy(r => r.RunId, StringComparer.Ordinal).ToList();
    }
    // Character, ascension and party size from the first top-level fields (the journal's entry row or the
    // payload's head); a body is never read whole.
    static (string? Character, int? Ascension, int? PartySize) Peek(FileInfo file)
    {
        string? character = null; int? ascension = null, party = null;
        try
        {
            byte[] head = new byte[8192];
            int read;
            using (var stream = new FileStream(file.FullName, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete))
                read = stream.ReadAtLeast(head, head.Length, throwOnEndOfStream: false);
            var reader = new Utf8JsonReader(head.AsSpan(0, read), isFinalBlock: false, default);
            while (reader.Read())
            {
                if (reader.TokenType == JsonTokenType.EndObject && reader.CurrentDepth == 0) break;
                if (reader.TokenType != JsonTokenType.PropertyName || reader.CurrentDepth != 1) continue;
                string name = reader.GetString()!;
                if (!reader.Read()) break;
                if (name == "character" && reader.TokenType == JsonTokenType.String) character = reader.GetString();
                else if (name == "ascension" && reader.TryGetInt32(out int a)) ascension = a;
                else if (name == "party_size" && reader.TryGetInt32(out int p)) party = p;
            }
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or JsonException or InvalidOperationException) { }
        return (character, ascension, party);
    }
    // A retained journal with no payload left in any queue cannot upload and is never reclaimed automatically.
    // A journal still owned (its handoff is running) is not one yet. One whose upload the server refused (its
    // marker, or a rejected copy left by a build without the marker) is a handled outcome, not a storage problem.
    public bool HasUnqueuedRetainedJournal()
    {
        var refused = RunFiles().Where(f => f.Path.EndsWith(ServerRejectedSuffix, StringComparison.Ordinal) ||
            f.Path.EndsWith(".json.rejected", StringComparison.Ordinal)).Select(f => f.Id).ToHashSet(StringComparer.Ordinal);
        foreach (var run in ListRuns().Where(r => r.State == LocalRunState.Retained && !refused.Contains(r.RunId)))
        {
            try { using var owner = OwnJournal(Path.Combine(Root, "incomplete", run.RunId + ".jsonl")); }
            catch (IOException) { continue; }
            return true;
        }
        return false;
    }
    // The local storage window's removal: every file of the run on this PC (queued body, receipt, oversize or
    // rejected copy, staged payload, journal), under the budget lock and the run's journal ownership. Nothing
    // on the server changes. False while the journal is owned elsewhere (being recorded or handed off).
    public async Task<bool> DeleteRunAsync(string runId)
    {
        if (!IsRunId(runId)) return false;
        await gate.WaitAsync();
        try
        {
            FileStream owner;
            try { owner = OwnJournal(Path.Combine(Root, "incomplete", runId + ".jsonl")); }
            catch (IOException) { return false; }
            using (owner)
                foreach (var (id, path) in RunFiles().ToList())
                    if (id == runId) File.Delete(path);
            return true;
        }
        finally { gate.Release(); }
    }
    // The server terminally refused this run's upload (400/413/422). Kept beside the retained journal so the refusal
    // outlives the 7-day quarantine of the rejected copies; not ".rejected", which that quarantine expires.
    internal const string ServerRejectedSuffix = ".server_rejected";
    internal void MarkServerRejected(string runId)
    {
        string journal = Path.Combine(Root, "incomplete", runId + ".jsonl");
        if (File.Exists(journal) || File.Exists(journal + ".retained"))
            File.WriteAllBytes(journal + ServerRejectedSuffix, []);
    }
    public void DeleteAcknowledgedJournal(string runId)
    {
        string path = Path.Combine(Root, "incomplete", runId + ".jsonl");
        if (File.Exists(path)) File.Delete(path);
        if (File.Exists(path + ".retained")) File.Delete(path + ".retained");
        if (File.Exists(path + ".payload")) File.Delete(path + ".payload");
        if (Directory.Exists(Path.GetDirectoryName(path)))
            foreach (var payload in Directory.GetFiles(Path.GetDirectoryName(path)!, Path.GetFileName(path) + ".*.payload*"))
                File.Delete(payload);
    }
    public Task ReserveReceiptAsync(string path,CancellationToken ct=default)=>
        // Draining an existing body may temporarily exceed the budget; refusing its crash-safe
        // receipt slot would prevent the upload that frees both that body and its journal.
        WriteAsync(path,new byte[16384],false,ct,essential:
            path.EndsWith(".json.receipt",StringComparison.Ordinal) && File.Exists(path[..^8]));
    public async Task FillReceiptAsync(string path,byte[] receipt,CancellationToken ct=default)
    {
        if(receipt.Length>16384)throw new InvalidOperationException("receipt_limit");
        await gate.WaitAsync(ct);
        try {
            var padded=Enumerable.Repeat((byte)' ',16384).ToArray();
            receipt.CopyTo(padded,0);
            using var file=new FileStream(path,FileMode.Open,FileAccess.Write,FileShare.None,16384,FileOptions.WriteThrough);
            if(file.Length!=16384)throw new IOException("receipt reservation missing");
            await file.WriteAsync(padded,ct);file.Flush(true);
        } finally {gate.Release();}
    }
}
