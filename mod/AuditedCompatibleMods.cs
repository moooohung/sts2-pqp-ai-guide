using System.Security.Cryptography;
using System.Text.Json;

namespace HumanPlayRecorder;

// Deliberately small: our own source-audited builds, not a general mod allowlist.
public static class AuditedCompatibleMods
{
    public const string BaseCardPoolsOnlySha256 =
        "5c2c0748d07ff9f93a3a845ea7e3c08de0b3d80be900fbe4743f7b45a76d08b0";
    public static bool Matches(string id, string? sha256) =>
        id == "BaseCardPoolsOnly" &&
        string.Equals(sha256, BaseCardPoolsOnlySha256, StringComparison.OrdinalIgnoreCase);

    public static string? ReadCandidateHash(string id, string directory, int assemblyCount)
    {
        if (id != "BaseCardPoolsOnly" || assemblyCount != 1) return null;
        try
        {
            string config = Path.Combine(directory, "BaseCardPoolsOnly.config.json");
            if (File.Exists(config))
            {
                using var json = JsonDocument.Parse(File.ReadAllBytes(config));
                if (!json.RootElement.TryGetProperty("allowed_mod_assemblies", out var allowed) ||
                    allowed.ValueKind != JsonValueKind.Array || allowed.GetArrayLength() != 0) return null;
            }
            using var file = File.OpenRead(Path.Combine(directory, "BaseCardPoolsOnly.dll"));
            return Convert.ToHexStringLower(SHA256.HashData(file));
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or JsonException) { return null; }
    }
}
