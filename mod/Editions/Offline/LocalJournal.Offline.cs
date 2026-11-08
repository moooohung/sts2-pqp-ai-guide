namespace HumanPlayRecorder;

// Offline edition: the run's input lifecycle still drives the advisor, but nothing is written
// anywhere. Complete/Invalidate keep the advisor's capture-integrity signal (AdvisorIntegration
// patches Invalidate to fall back to live state).
public sealed class LocalJournal : IAsyncDisposable
{
    int invalid;
    public bool Complete => Volatile.Read(ref invalid) == 0;
    public bool Append(object item) => Complete;
    public void Invalidate() => Interlocked.Exchange(ref invalid, 1);
    public ValueTask DisposeAsync() => ValueTask.CompletedTask;
}
