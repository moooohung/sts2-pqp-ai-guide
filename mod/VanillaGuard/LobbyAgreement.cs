namespace HumanPlayRecorder.VanillaGuard;

public sealed record GuardProposal(string Session, long Revision, bool Enabled, string Fingerprint);
public sealed record GuardAcknowledgement(string Session, long Revision, bool Enabled, string Fingerprint, bool Applied);
public sealed record GuardStartDecision(string Session, long Revision, bool Enabled, string? Reason);

// Transport-independent protocol state. The lobby adapter must send the final
// decision reliably BEFORE the game's begin message, including the OFF fallback.
public sealed class LobbyAgreement
{
    readonly string session;
    readonly string fingerprint;
    readonly Dictionary<ulong, bool> peers = [];
    readonly Dictionary<ulong, GuardAcknowledgement> acknowledgements = [];
    long revision;
    GuardProposal? proposal;
    bool localApplied;
    public LobbyAgreement(string session, string fingerprint)
    {
        this.session = session;
        this.fingerprint = fingerprint;
    }

    // Joining/leaving, ready changes and host setting changes start a new
    // revision, so a late acknowledgement can never authorize a newer run.
    public GuardProposal Propose(bool enabled, IEnumerable<(ulong Id, bool HasMod)> roster,
        Func<bool, bool> apply)
    {
        peers.Clear();
        foreach (var (id, hasMod) in roster) peers.Add(id, hasMod);
        acknowledgements.Clear();
        proposal = new(session, ++revision, enabled, fingerprint);
        localApplied = apply(enabled);
        return proposal;
    }

    public void Acknowledge(ulong sender, GuardAcknowledgement acknowledgement)
    {
        if (proposal is null || !peers.TryGetValue(sender, out bool hasMod) || !hasMod ||
            acknowledgement.Session != session || acknowledgement.Revision != revision) return;
        acknowledgements[sender] = acknowledgement;
    }

    public GuardStartDecision Decide()
    {
        if (proposal is null) return new(session, revision, false, "guard agreement unavailable");
        if (!proposal.Enabled) return new(session, revision, false, null);
        string? reason = !localApplied ? "host could not apply original rules" :
            peers.Any(p => !p.Value) ? "a peer does not have HumanPlayRecorder" :
            peers.Keys.Any(id => !acknowledgements.TryGetValue(id, out var ack) ||
                !ack.Applied || !ack.Enabled || ack.Fingerprint != fingerprint)
                ? "guard state or original-content fingerprint not acknowledged by every peer" : null;
        return new(session, revision, reason is null, reason);
    }
}

public sealed class LobbyAgreementPeer(string fingerprint, Func<bool, bool> apply)
{
    GuardProposal? proposal;
    GuardAcknowledgement? acknowledgement;

    // The adapter authenticates the sender as the current lobby host.
    public GuardAcknowledgement? Receive(GuardProposal value)
    {
        if (proposal is not null && (proposal.Session != value.Session || value.Revision < proposal.Revision))
            return null;
        if (proposal == value) return acknowledgement;
        proposal = value;
        bool compatible = !value.Enabled || value.Fingerprint == fingerprint;
        bool applied = apply(compatible && value.Enabled);
        acknowledgement = new(value.Session, value.Revision, compatible && value.Enabled,
            fingerprint, compatible && applied);
        return acknowledgement;
    }

    public bool ApplyDecision(GuardStartDecision decision)
    {
        if (!decision.Enabled) return apply(false);
        if (proposal is null || acknowledgement is not { Applied: true, Enabled: true } ||
            decision.Session != proposal.Session || decision.Revision != proposal.Revision) return false;
        return true;
    }
}
