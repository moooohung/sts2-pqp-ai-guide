using System.Text.Json.Serialization;

namespace HumanPlayRecorder;

// The run's start context. The advisor replays from it in both editions; the online edition
// also uploads it with the recording.
public sealed class RecordingAdmission
{
    int rejected;
    public bool Clean => Volatile.Read(ref rejected) == 0;
    public bool Reject() => Interlocked.Exchange(ref rejected, 1) == 0;
    public Task AdviceDrained { get; set; } = Task.CompletedTask;
}

public sealed record RunHeader(string run_id, string mod_version, string game_version,
    string game_dll_sha256, string character, string seed, ModRef[] active_mods, string started_at,
    string mode, int party_size, int local_seat, PartySeat[] party, string shared_run_id)
{
    public int ascension {get;init;}
    public TrainingDecision? training {get;init;}
    public EnvironmentMod[]? environment_mods {get;init;}
    public PatchAuditEntry[]? patch_audit {get;init;}
    public string? replay_contract {get;init;}
    public string? input_seed {get;init;}
    public ReplaySetup? setup {get;init;}
    public StateDigest? initial_state {get;init;}
    public int[]? ai_teammate_seats {get;init;}
    [JsonIgnore] public RecordingAdmission Admission { get; } = new();
}
