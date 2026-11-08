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
    public void Progress(object seat) => waiting.Remove(seat);
    public void Clear() => waiting.Clear();
}
