using System.Diagnostics;

namespace HumanPlayRecorder.Ai;

public sealed class FrameCaptureBudget : IDisposable
{
    [ThreadStatic] static FrameCaptureBudget? current;
    [ThreadStatic] static bool pooled;
    [ThreadStatic] static long spent;
    readonly FrameCaptureBudget? previous;
    readonly Func<long> clock;
    readonly long deadline;
    readonly long started;
    readonly bool chargeFrame;
    [ThreadStatic] static bool exhausted;
    // Measured in the live game at 4K: a three-option event projection takes
    // 1.0-1.1 ms warm, so a 1 ms cap refused on alternate frames. Captures run
    // only when the decision changes or on sampled verification frames.
    public const double Milliseconds = 4;
    public static bool Active => current is not null;
    /// <summary>True once a budget deadline was hit in this frame, even if the
    /// refusal was wrapped into another exception on the way out.</summary>
    public static bool Exhausted => exhausted;
    [ThreadStatic] static double frameMilliseconds;
    static double FrameMilliseconds => frameMilliseconds > 0 ? frameMilliseconds : Milliseconds;
    public static void BeginFrame(double milliseconds = Milliseconds)
    {
        pooled = true; spent = 0; exhausted = false; frameMilliseconds = milliseconds;
    }
    /// <summary>Starts an independently budgeted pass later in the same frame.</summary>
    public static void BeginPass() => exhausted = false;
    public FrameCaptureBudget(Func<long>? clock = null, long? ticks = null)
    {
        previous = current;
        this.clock = clock ?? Stopwatch.GetTimestamp;
        chargeFrame = clock is null && pooled && previous is null;
        started = this.clock();
        long allowed = ticks ?? (long)(Stopwatch.Frequency * FrameMilliseconds / 1000);
        if (chargeFrame) allowed = Math.Min(allowed, Math.Max(0, (long)(Stopwatch.Frequency * FrameMilliseconds / 1000) - spent));
        deadline = started + allowed;
        current = this;
    }
    public static void Check()
    {
        for (var budget = current; budget is not null; budget = budget.previous)
            if (budget.clock() >= budget.deadline) {
                exhausted = true;
                throw new AdviceUnavailableException(AdviceReason.BudgetExceeded, $"추천 불가: 화면 캡처가 작업 예산({FrameMilliseconds}ms)을 초과했습니다.");
            }
    }
    public void Dispose()
    {
        if (chargeFrame) spent += clock() - started;
        current = previous;
    }
}

public sealed class DecisionCaptureVersions
{
    readonly Dictionary<int, long> seen = new();
    public bool Changed(int seat, long signature)
    {
        if (seen.TryGetValue(seat, out long previous) && previous == signature) return false;
        return true;
    }
    public void Commit(int seat, long signature) => seen[seat] = signature;
    public void Clear() => seen.Clear();
}
