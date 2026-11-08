using System.Collections.Concurrent;

namespace HumanPlayRecorder;

// One filesystem budget shared by all journals, queue bodies and receipts.
// The producer never waits for this lock; only journal/queue workers do.
public sealed class LocalStorageBudget
{
    public const long DefaultMaxBytes = 256L * 1024 * 1024;
    public const int DefaultMaxFiles = 1024;
    static readonly ConcurrentDictionary<string, SemaphoreSlim> Gates = new(StringComparer.OrdinalIgnoreCase);
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
    public async Task WriteAsync(string path, byte[] bytes, bool append, CancellationToken ct = default)
    {
        path = Path.GetFullPath(path);
        if (!path.StartsWith(Root + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
            throw new ArgumentException("storage path escapes budget");
        await gate.WaitAsync(ct);
        try
        {
            var files = Directory.Exists(Root) ? Directory.GetFiles(Root, "*", SearchOption.AllDirectories) : [];
            long used = 0; int count = 0;
            foreach (string file in files)
            {
                var info = new FileInfo(file);
                bool quarantine = file.EndsWith(".rejected", StringComparison.Ordinal) ||
                    file.EndsWith(".oversize", StringComparison.Ordinal) || file.EndsWith(".partial", StringComparison.Ordinal);
                if (quarantine && info.LastWriteTimeUtc < DateTime.UtcNow.AddDays(-7))
                { File.Delete(file); continue; }
                count++; used = checked(used + info.Length);
            }
            bool exists = File.Exists(path);
            if (used + bytes.Length > maxBytes || count + (exists ? 0 : 1) > maxFiles)
                throw new InvalidOperationException("queue_full");
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            await using var stream = new FileStream(path, append ? FileMode.Append : FileMode.CreateNew,
                FileAccess.Write, FileShare.Read, 65536, FileOptions.Asynchronous | FileOptions.WriteThrough);
            await stream.WriteAsync(bytes, ct);
            await stream.FlushAsync(ct);
        }
        finally { gate.Release(); }
    }
    public void DeleteAcknowledgedJournal(string runId)
    {
        string path = Path.Combine(Root, "incomplete", runId + ".jsonl");
        if (File.Exists(path)) File.Delete(path);
    }
    public Task ReserveReceiptAsync(string path,CancellationToken ct=default)=>
        WriteAsync(path,new byte[16384],false,ct);
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
