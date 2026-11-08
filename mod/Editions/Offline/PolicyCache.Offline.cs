namespace HumanPlayRecorder.Ai;

// Offline edition: the policy bundled in the mod folder is the only source. LoadCached applies the
// same checks the online edition applies to its cache: manifest schema and game version, weights
// SHA-256 against current.json, and the safetensors metadata (schema, game version, checkpoint id, step).
internal static partial class PolicyCache
{
    internal static byte[] Load(string directory, string gameVersion, CancellationToken ct = default) =>
        LoadCached(directory, gameVersion, ct);
}
