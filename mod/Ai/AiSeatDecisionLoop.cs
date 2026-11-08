namespace HumanPlayRecorder.Ai;

// One publication is executable once. The caller must independently re-project
// the live domain before handing it to this gate.
public sealed class AiSeatDecisionLoop
{
    long generation;
    long fingerprint;
    bool pending;
    AiAdvisor.LiveOption[] options = [];

    public long Publish(long stateFingerprint, IReadOnlyList<AiAdvisor.LiveOption> domain)
    {
        fingerprint = stateFingerprint;
        options = domain.ToArray();
        pending = options.Length > 0;
        return ++generation;
    }

    public void Invalidate()
    {
        generation++;
        pending = false;
        options = [];
    }

    public bool TryTake(long publication, long currentFingerprint,
        IReadOnlyList<AiAdvisor.LiveOption> current,
        IReadOnlyList<(object Option, float Probability)>? advice, out AiAdvisor.LiveOption? chosen,
        string? searched = null, Func<AiAdvisor.LiveOption, bool>? blocked = null)
    {
        if (!TryPeek(publication, currentFingerprint, current, advice, out chosen, searched, blocked)) return false;
        pending = false;
        return true;
    }

    /// <param name="searched">Binding the decision-time search chose for this decision, if any;
    /// it replaces the policy's most probable option when it is one of this domain's.</param>
    /// <param name="blocked">The reward-reopen guard: options the seat must not take now
    /// (a card reward it closed without taking a card on this rewards screen; reopening is
    /// legal but returns to the same screen, so a greedy choice would cycle forever). The
    /// best remaining option is taken instead; the advice's probabilities are not changed.</param>
    public bool TryPeek(long publication, long currentFingerprint,
        IReadOnlyList<AiAdvisor.LiveOption> current,
        IReadOnlyList<(object Option, float Probability)>? advice, out AiAdvisor.LiveOption? chosen,
        string? searched = null, Func<AiAdvisor.LiveOption, bool>? blocked = null)
    {
        chosen = null;
        if (!pending || publication != generation || currentFingerprint != fingerprint ||
            advice is null || advice.Count != options.Length || current.Count != options.Length) return false;
        for (int i = 0; i < options.Length; i++)
            if (!ReferenceEquals(options[i].Option, current[i].Option) || options[i].Action != current[i].Action ||
                !ReferenceEquals(options[i].Target, current[i].Target) || options[i].Binding != current[i].Binding ||
                !current[i].Legal) return false;
        var seen = new HashSet<int>();
        double total = 0;
        int best = -1, any = -1;
        var probabilities = new float[options.Length];
        foreach (var (value, probability) in advice) {
            if (value is not AiAdvisor.ActionOption joint || !float.IsFinite(probability) ||
                probability is < 0 or > 1) return false;
            int index = Array.FindIndex(options, option => ReferenceEquals(option.Option, joint.Source) &&
                option.Action == joint.Action && ReferenceEquals(option.Target, joint.Target));
            if (index < 0 || !seen.Add(index) || !options[index].Legal) return false;
            total += probability;
            probabilities[index] = probability;
        }
        if (Math.Abs(total - 1) > 1e-5) return false;
        // Stable tie break: original domain order, not dictionary/worker order.
        for (int index = 0; index < options.Length; index++) {
            if (!seen.Contains(index)) continue;
            if (any < 0 || probabilities[index] > probabilities[any]) any = index;
            if (blocked?.Invoke(options[index]) != true && (best < 0 || probabilities[index] > probabilities[best])) best = index;
        }
        chosen = best >= 0 ? options[best] : any >= 0 ? options[any] : null;
        if (searched is not null && Array.Find(options, option => option.Binding == searched) is { } pick &&
            seen.Contains(Array.IndexOf(options, pick)) && blocked?.Invoke(pick) != true) chosen = pick;
        return chosen is not null;
    }
}
