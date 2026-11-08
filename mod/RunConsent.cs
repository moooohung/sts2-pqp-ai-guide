namespace HumanPlayRecorder;

// The consent state of one multiplayer run, from the lobby through completion. It only ever gets
// stricter: a withdrawal, or any change in who is playing (leave, join, reconnect, continuing a
// saved run), makes the run permanently non-uploadable. A later "yes" never restores it.
public sealed class RunConsent
{
    readonly ulong host;
    readonly Func<ulong, bool> isAi;
    readonly HashSet<ulong> players;
    readonly List<ulong> humans;
    public string? BrokenBy { get; private set; }
    // Raised once, the first time the run stops being uploadable after it started.
    public event Action<string>? Broken;

    public RunConsent(ulong host, IEnumerable<ulong> players, Func<ulong, bool> isAi,
        IReadOnlyDictionary<ulong, bool> agreedAtStart)
    {
        this.host = host;
        this.isAi = isAi;
        this.players = [.. players];
        humans = this.players.Where(id => !isAi(id)).ToList();
        if (!PartyConsent.AllAgreed(this.players, host, isAi, agreedAtStart))
            BrokenBy = "not every player agreed to training data";
    }

    public bool Uploadable => BrokenBy is null;

    void Break(string why)
    {
        if (BrokenBy is not null) return;
        BrokenBy = why;
        Broken?.Invoke(why);
    }

    // An answer from this player's own connection. Only a "no" matters once the run has started.
    public void Observe(ulong player, bool agreed)
    {
        if (!agreed && (player == host || humans.Contains(player)))
            Break("a player withdrew training consent");
    }

    public void MembershipChanged(string why) => Break(why);

    // At completion: the table must still be the one that agreed at the start.
    public void Verify(IEnumerable<ulong> currentPlayers)
    {
        if (!players.SetEquals(currentPlayers)) Break("the players of the run changed");
    }
}