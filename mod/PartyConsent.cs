namespace HumanPlayRecorder;

// A multiplayer run is recorded and uploaded (for training and for replay) only when every human
// player runs this mod and has agreed to training data. A player who runs a vanilla game, declined
// or has not answered never sends an agreement, so the whole run stays on the PC. An AI teammate
// seat belongs to the host and counts as the host.
public static class PartyConsent
{
    public static bool AllAgreed(IEnumerable<ulong> players, ulong host, Func<ulong, bool> isAi,
        IReadOnlyDictionary<ulong, bool> agreed)
    {
        foreach (ulong id in players)
            if (!(agreed.TryGetValue(isAi(id) ? host : id, out bool yes) && yes)) return false;
        return true;
    }
}
