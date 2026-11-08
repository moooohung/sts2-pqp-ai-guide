namespace HumanPlayRecorder.VanillaGuard;

// The transport adapter must reserve before starting a connection (not after
// IsConnected becomes true), and release only after disconnect/queue drainage.
// WireTransportBoundary owns these reservations for live game services.
public sealed class WireCacheBoundary
{
    readonly object sync = new();
    int connections, operations;
    bool changing;

    public IDisposable ReserveConnection()
    {
        lock (sync)
        {
            if (changing) throw new InvalidOperationException("wire cache is changing");
            connections++;
            return new Reservation(this, true);
        }
    }

    public IDisposable EnterPacketOperation()
    {
        lock (sync)
        {
            if (changing) throw new InvalidOperationException("wire cache is changing");
            operations++;
            return new Reservation(this, false);
        }
    }

    public void AtDisconnectedBoundary(Action change)
    {
        lock (sync)
        {
            if (changing || connections != 0 || operations != 0)
                throw new InvalidOperationException("wire cache requires disconnected transports and no in-flight operations");
            changing = true;
            try { change(); }
            finally { changing = false; }
        }
    }

    sealed class Reservation(WireCacheBoundary owner, bool connection) : IDisposable
    {
        bool disposed;
        public void Dispose()
        {
            lock (owner.sync)
            {
                if (disposed) return;
                disposed = true;
                if (connection) owner.connections--;
                else owner.operations--;
            }
        }
    }
}
