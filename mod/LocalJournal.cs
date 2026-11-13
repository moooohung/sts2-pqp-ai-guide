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
    readonly LocalStorageBudget? budget;
    bool ownsJournal;
    bool retaining, retentionDurable;
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
        this.budget = budget;
        worker = Task.Run(async () =>
        {
            try
            {
                budget.OpenJournal(path);
                ownsJournal = true;
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
    public async Task FinishForHandoffAsync()
    {
        if (!persist) return;
        channel.Writer.TryComplete();
        await worker;
    }
    // Only a run about to be queued is retained, before ownership is released or any queue allocation is tried:
    // a failed handoff must survive both budget reclamation and the next process launch. A run that can never
    // upload (excluded, incomplete) stays an ordinary journal that expires like any other.
    internal async Task RetainAsync()
    {
        await FinishForHandoffAsync();
        if (!Complete || Failure is not null) throw new InvalidOperationException("incomplete capture cannot upload");
        retaining = true;
        await budget!.WriteAsync(path, LocalStorageBudget.RetentionFooter, append: true);
        retentionDurable = true;
        try { File.Move(path, path + ".retained"); }
        catch (IOException) { /* The durable footer protects the original name too. */ }
        catch (UnauthorizedAccessException) { /* Retention does not depend on a rename. */ }
    }
    internal async Task<string?> StageHandoffAsync(RunPayload payload, UploadQueue queue)
    {
        if (!persist) return null;
        await RetainAsync();
        return await queue.StageHandoffAsync(payload);
    }
    public async ValueTask DisposeAsync()
    {
        if (!persist) return;
        channel.Writer.TryComplete();
        await worker;
        if (!ownsJournal) return;
        // If even the durable retention append failed, never release this lifecycle's
        // ownership as though its completed journal had been safely handed off.
        if (retaining && !retentionDurable && !File.Exists(path + ".delete")) return;
        if (!Complete && File.Exists(path))
        {
            try { File.Move(path, path + ".rejected"); File.SetLastWriteTimeUtc(path + ".rejected", DateTime.UtcNow); }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException) { Failure ??= e; }
        }
        budget?.CloseJournal(path);
        ownsJournal = false;
    }
}
