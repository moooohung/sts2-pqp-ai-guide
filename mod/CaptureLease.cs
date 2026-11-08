namespace HumanPlayRecorder;

// A run's capture lasts only while sharing stays allowed. Switching training consent and replay saving
// both off ends it for good: switching one back on mid-run must not yield a recording that covers the
// stretch in which the player had asked for nothing to be recorded.
public sealed class CaptureLease(Func<bool> allowed, Action end)
{
    int ended;
    public bool Ended => Volatile.Read(ref ended) != 0;

    // Called whenever the sharing settings change.
    public void Check()
    {
        if (!allowed() && Interlocked.Exchange(ref ended, 1) == 0) end();
    }
}
