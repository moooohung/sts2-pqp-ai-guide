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
        IReadOnlyList<(object Option, float Probability)>? advice, out AiAdvisor.LiveOption? chosen)
    {
        if (!TryPeek(publication, currentFingerprint, current, advice, out chosen)) return false;
        pending = false;
        return true;
    }

    public bool TryPeek(long publication, long currentFingerprint,
        IReadOnlyList<AiAdvisor.LiveOption> current,
        IReadOnlyList<(object Option, float Probability)>? advice, out AiAdvisor.LiveOption? chosen)
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
        float best = -1;
        foreach (var (value, probability) in advice) {
            if (value is not AiAdvisor.ActionOption joint || !float.IsFinite(probability) ||
                probability is < 0 or > 1) return false;
            int index = Array.FindIndex(options, option => ReferenceEquals(option.Option, joint.Source) &&
                option.Action == joint.Action && ReferenceEquals(option.Target, joint.Target));
            if (index < 0 || !seen.Add(index) || !options[index].Legal) return false;
            total += probability;
            // Stable tie break: original domain order, not dictionary/worker order.
            if (probability > best || probability == best && index < Array.IndexOf(options, chosen)) {
                best = probability;
                chosen = options[index];
            }
        }
        if (Math.Abs(total - 1) > 1e-5) { chosen = null; return false; }
        return chosen is not null;
    }
}
