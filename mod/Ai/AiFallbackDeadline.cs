namespace HumanPlayRecorder.Ai;

public sealed class AiFallbackDeadline
{
    readonly Dictionary<object, (object Decision, long Start)> waiting = new();
    public bool Due(object seat, long now, long delay)
        => Due(seat, seat, now, delay);
    public bool Due(object seat, object? decision, long now, long delay)
    {
        if (decision is null) { waiting.Remove(seat); return false; }
        if (!waiting.TryGetValue(seat, out var entry) || !Equals(entry.Decision, decision)) {
            waiting[seat] = (decision, now);
            return false;
        }
        return now - entry.Start >= delay;
    }
    /// <summary>The latest time the seat may act and still beat the fallback for its pending
    /// decision (a decision not yet waiting starts its clock now), less a safety margin.</summary>
    public long ActBy(object seat, long now, long delay, long margin)
        => (waiting.TryGetValue(seat, out var entry) ? entry.Start : now) + delay - margin;
    public void Progress(object seat) => waiting.Remove(seat);
    public void Clear() => waiting.Clear();
}
