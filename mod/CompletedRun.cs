using System.Text.Json.Serialization;

namespace HumanPlayRecorder;

// Runs on the journal/queue worker, never on the game thread. A partially
// recorded/cancelled run must remain local even when a terminal result exists.
public static class CompletedRun
{
    public static RunPayload Assemble(RunHeader header, IReadOnlyList<ActionRow> completed,
        IReadOnlyList<FloorDigest> floors, RunResult result, string endedAt)
    {
        if (header.ai_teammate_seats is { Length: > 0 })
            throw new InvalidOperationException("AI teammate runs cannot upload as human runs");
        if (!header.Admission.Clean || !header.Admission.AdviceDrained.IsCompletedSuccessfully)
            throw new InvalidOperationException("advisor replay integrity is not clean");
        if(header.input_seed?.StartsWith("old",StringComparison.Ordinal)==true)throw new ArgumentException("legacy seed excluded");
        UploadMetadata.Validate(header.environment_mods??header.active_mods.Select(m=>new EnvironmentMod(m.id,m.version)).Distinct().ToArray(),header.patch_audit);
        var actions = completed.OrderBy(row => row.seq).ToArray();
        if (actions.Length is < 1 or > 100_000 || floors.Count is < 1 or > 1000)
            throw new ArgumentException("run limits");
        for (int i = 0; i < actions.Length; i++)
        {
            var row = actions[i];
            if (row.seq != i || row.parent_seq is { } parent && (parent < 0 || parent >= i))
                throw new ArgumentException("incomplete input order");
            RecordingFormat.Validate(row.action);
            if (row.state_sha256 != RecordingFormat.Hash(row.after))
                throw new ArgumentException("action digest mismatch");
        }
        int previousFloor = -1;
        foreach (var floor in floors)
        {
            if (floor.act != floor.state.act || floor.floor != floor.state.floor ||
                floor.floor <= previousFloor || floor.after_action < -1 ||
                floor.after_action >= actions.Length ||
                floor.sha256 != RecordingFormat.Hash(floor.state))
                throw new ArgumentException("invalid floor digest");
            previousFloor = floor.floor;
        }
        if (!DateTimeOffset.TryParse(header.started_at, out var start) ||
            !DateTimeOffset.TryParse(endedAt, out var end) || end < start ||
            !header.started_at.EndsWith('Z') || !endedAt.EndsWith('Z'))
            throw new ArgumentException("timestamps");
        if (result.act is < 1 or > 10 || result.floor is < 0 or > 1000 || result.hp < 0)
            throw new ArgumentException("result");
        if (header.party_size is < 1 or > 4 || header.party.Length != header.party_size ||
            header.local_seat < 0 || header.local_seat >= header.party_size ||
            header.mode != (header.party_size == 1 ? "solo" : "multiplayer") ||
            header.party.Where((p, i) => p.seat != i).Any() ||
            header.character != header.party[header.local_seat].character)
            throw new ArgumentException("party");
        if (header.ascension is < 0 or > 10)
            throw new ArgumentException("ascension");
        return new("sts2-human-run-v2", RecordingFormat.ActionSchema, header.run_id,
            header.mod_version, header.game_version, header.game_dll_sha256,
            header.character, header.ascension, header.seed, header.active_mods, actions,
            floors.ToArray(), result, header.started_at, endedAt,
            header.mode, header.party_size, header.local_seat, header.party, header.shared_run_id) {
                training=header.training,environment_mods=header.environment_mods,
                patch_audit=header.patch_audit,replay_contract=header.replay_contract,input_seed=UploadMetadata.Seed(header.seed),
                setup=header.setup,initial_state=header.initial_state};
    }
    public static async Task QueueAsync(UploadQueue queue, RunHeader header,
        IReadOnlyList<ActionRow> actions, IReadOnlyList<FloorDigest> floors,
        RunResult result, string endedAt, bool completeCapture, CancellationToken ct = default,
        Action<string>? log = null, LocalJournal? journal = null)
    {
        // This flag is local only, never added to RunPayload.
        await header.Admission.AdviceDrained.ConfigureAwait(false);
        if (!completeCapture || !header.Admission.Clean)
            throw new InvalidOperationException("incomplete capture cannot upload");
        var payload = Assemble(header, actions, floors, result, endedAt);
        string? durable = journal is null ? null : await journal.StageHandoffAsync(payload, queue);
        await queue.EnqueueDurableAsync(payload, durable, ct);
        (log ?? Console.WriteLine)($"[HumanPlayRecorder] record finalized/queued: actions {actions.Count}, floors {floors.Count}, party_size {header.party_size}");
    }
}
