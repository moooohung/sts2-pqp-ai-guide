namespace HumanPlayRecorder;

// When the storage-full note appears: once per session, only after the main menu is up, and a note that
// could not be drawn (no scene tree yet) stays requested instead of being lost for the session.
sealed class StorageNotice
{
    int requested, shown, menuReady;

    // Any thread. True when the caller must schedule Show on the main thread now.
    public bool Request()
    {
        if (Volatile.Read(ref shown) != 0) return false;
        Interlocked.Exchange(ref requested, 1);
        return Volatile.Read(ref menuReady) != 0;
    }

    // Main thread, once the main menu is ready. True when a request is waiting for it.
    public bool MenuReady()
    {
        Interlocked.Exchange(ref menuReady, 1);
        return Volatile.Read(ref requested) != 0 && Volatile.Read(ref shown) == 0;
    }

    // Main thread. draw returns false when nothing could be shown; the request then stays pending.
    public void Show(Func<bool> draw)
    {
        if (Volatile.Read(ref requested) == 0 || Interlocked.Exchange(ref shown, 1) != 0) return;
        bool drawn = false;
        try { drawn = draw(); }
        finally { if (!drawn) Interlocked.Exchange(ref shown, 0); }
    }
}
