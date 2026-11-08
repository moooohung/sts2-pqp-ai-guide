using System.Security.Cryptography;
using System.Text.Json;

namespace HumanPlayRecorder.Ai;

internal sealed class PolicyLoadException(AiFailureKind kind, Exception error)
    : Exception(error.Message, error)
{
    internal AiFailureKind Kind { get; } = kind;
}

// Loading and validating a checkpoint from a local folder, shared by both editions. The online
// edition downloads into that folder first (PolicyCache.Download.cs); the offline edition reads
// the copy bundled beside the DLL (Editions/Offline/PolicyCache.Offline.cs).
internal static partial class PolicyCache
{
    internal static byte[] LoadCached(string directory, string gameVersion, CancellationToken ct = default)
    {
        ct.ThrowIfCancellationRequested();
        try {
            using var cached = JsonDocument.Parse(File.ReadAllBytesAsync(Path.Combine(directory, "current.json"), ct).GetAwaiter().GetResult());
            Validate(cached.RootElement, gameVersion);
            string hash = cached.RootElement.GetProperty("weights_sha256").GetString()!;
            byte[] bytes = File.ReadAllBytesAsync(Path.Combine(directory, hash + ".safetensors"), ct).GetAwaiter().GetResult();
            ct.ThrowIfCancellationRequested();
            if (Hash(bytes) != hash) throw new InvalidDataException("CACHED_CHECKPOINT_HASH_MISMATCH");
            ValidateWeights(bytes, cached.RootElement, gameVersion);
            return bytes;
        } catch (Exception error) when (!ct.IsCancellationRequested) {
            throw new PolicyLoadException(error is IOException
                and not FileNotFoundException and not DirectoryNotFoundException
                ? AiFailureKind.Internal : AiFailureKind.CheckpointInvalid, error);
        }
    }
    static string Hash(byte[] bytes) => Convert.ToHexStringLower(SHA256.HashData(bytes));
    static void Validate(JsonElement manifest, string version)
    {
        string hash = manifest.GetProperty("weights_sha256").GetString()!;
        if (manifest.GetProperty("schema").GetString() != "sts2-policy-web-v2" ||
            manifest.GetProperty("game_version").GetString() != version ||
            hash.Length != 64 || hash.Any(c => !char.IsAsciiHexDigitLower(c)))
            throw new InvalidDataException("INCOMPATIBLE_CHECKPOINT");
    }
    static void ValidateWeights(byte[] bytes, JsonElement manifest, string version)
    {
        if (bytes.Length < 8) throw new InvalidDataException("TRUNCATED_CHECKPOINT");
        ulong length = System.Buffers.Binary.BinaryPrimitives.ReadUInt64LittleEndian(bytes);
        if (length > 1024 * 1024 || length > (ulong)bytes.Length - 8) throw new InvalidDataException("INVALID_CHECKPOINT_HEADER");
        using var header = JsonDocument.Parse(bytes.AsMemory(8, (int)length));
        var metadata = header.RootElement.GetProperty("__metadata__");
        if (metadata.GetProperty("schema").GetString() != "sts2-policy-web-v2" ||
            metadata.GetProperty("game_version").GetString() != version ||
            metadata.GetProperty("checkpoint_id").GetString() != manifest.GetProperty("checkpoint_id").GetString() ||
            metadata.GetProperty("step").GetString() != manifest.GetProperty("step").GetUInt64().ToString(System.Globalization.CultureInfo.InvariantCulture))
            throw new InvalidDataException("CHECKPOINT_METADATA_MISMATCH");
    }
}
