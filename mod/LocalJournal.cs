using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text;
using System.Globalization;
using System.Threading.Channels;

namespace HumanPlayRecorder;

// Nonblocking producer. A full channel invalidates the run instead of losing
// actions silently. File creation/flush/serialization happen only on the worker.
public sealed class LocalJournal : IAsyncDisposable
{
    readonly Channel<object> channel = Channel.CreateBounded<object>(
        new BoundedChannelOptions(4096) { SingleReader = true, SingleWriter = false,
            FullMode = BoundedChannelFullMode.Wait });
    readonly Task worker;
    readonly string path;
    readonly bool persist;
    int invalid;
    public bool Complete => Volatile.Read(ref invalid) == 0;
    public Exception? Failure { get; private set; }
    public LocalJournal(string directory, string id, LocalStorageBudget? budget = null, bool persist = true)
    {
        this.persist = persist;
        path = Path.Combine(directory, id + ".jsonl");
        // Advisor-only capture reuses the input lifecycle without any disk or upload work.
        if (!persist) { worker = Task.CompletedTask; return; }
        budget ??= new LocalStorageBudget(directory);
        worker = Task.Run(async () =>
        {
            try
            {
                await budget.WriteAsync(path, [], append: false);
                await foreach (var item in channel.Reader.ReadAllAsync())
                {
                    byte[] bytes = Encoding.UTF8.GetBytes(JsonSerializer.Serialize(item, RecordingFormat.Json) + "\n");
                    await budget.WriteAsync(path, bytes, append: true);
                }
            }
            catch (Exception e) { Failure = e; Invalidate(); }
        });
    }
    public bool Append(object item)
    {
        if (!Complete) return false;
        if (!persist) return true;
        if (channel.Writer.TryWrite(item)) return true;
        Invalidate();
        return false;
    }
    public void Invalidate() => Interlocked.Exchange(ref invalid, 1);
    public async ValueTask DisposeAsync()
    {
        if (!persist) return;
        channel.Writer.TryComplete();
        await worker;
        if (!Complete && File.Exists(path))
        {
            try { File.Move(path, path + ".rejected"); File.SetLastWriteTimeUtc(path + ".rejected", DateTime.UtcNow); }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException) { Failure ??= e; }
        }
    }
}
