using System.IO.Hashing;
using System.Runtime.InteropServices;

namespace HumanPlayRecorder.Ai;

// The fingerprint is only compared with other fingerprints of this process (the
// unsignalled-change check), so it hashes an unambiguous binary encoding of the
// projection instead of serializing it: JSON serialization was a third of every capture.
internal sealed class LiveProjectionFingerprint : IDisposable
{
    readonly XxHash3 hash = new();

    public LiveProjectionFingerprint(string schema, int seat)
    {
        Text(schema);
        Int(seat);
    }

    void Int(int value) => hash.Append(MemoryMarshal.AsBytes(MemoryMarshal.CreateReadOnlySpan(ref value, 1)));
    void Float(float value) => hash.Append(MemoryMarshal.AsBytes(MemoryMarshal.CreateReadOnlySpan(ref value, 1)));
    void Text(string? value)
    {
        Int(value?.Length ?? -1);
        if (value is not null) hash.Append(MemoryMarshal.AsBytes(value.AsSpan()));
    }
    void Model(LiveObservation.Id? id)
    {
        Int(id is null ? 0 : 1);
        if (id is null) return;
        Text(id.@namespace);
        Text(id.name);
    }
    void Map<T>(SortedDictionary<string, T> values, Action<T> write)
    {
        Int(values.Count);
        foreach (var (key, value) in values) { Text(key); write(value); }
    }
    void Flag(bool value) => Int(value ? 1 : 0);

    public void Write(LiveObservation.Element row)
    {
        FrameCaptureBudget.Check();
        Int(1);
        Text(row.kind);
        Model(row.model);
        Model(row.color);
        Int(row.detail.Length);
        foreach (var id in row.detail) Model(id);
        Int(row.owner);
        Int(row.parent);
        Map(row.numbers, Int);
        Map(row.floats, Float);
        Map(row.flags, Flag);
    }

    public void Write(int from, int to, string relation)
    {
        FrameCaptureBudget.Check();
        Int(2);
        Int(from);
        Int(to);
        Text(relation);
    }

    public void Write(LiveObservation.Option option)
    {
        FrameCaptureBudget.Check();
        Int(3);
        Text(option.binding);
        Text(option.action);
        Int(option.source);
        Int(option.target);
        Model(option.model);
        Map(option.numbers, Int);
        Map(option.flags, Flag);
        Flag(option.legal);
    }

    public long Finish()
    {
        FrameCaptureBudget.Check();
        return (long)hash.GetCurrentHashAsUInt64();
    }

    public void Dispose() { }
}
