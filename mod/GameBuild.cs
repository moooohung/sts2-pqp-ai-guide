using System.Text.Json;

namespace HumanPlayRecorder;

public static class GameBuild
{
    static readonly JsonDocument Build = JsonDocument.Parse(
        typeof(GameBuild).Assembly.GetManifestResourceStream("HumanPlayRecorder.game-build.json")
        ?? throw new InvalidOperationException("game build pin missing"));
    public static string Version => Build.RootElement.GetProperty("version").GetString()!;
    public static string Sha256 => Build.RootElement.GetProperty("sha256").GetString()!;
    public static string PolicyTag => "public-beta+" + Sha256[..8];
    // Policy and simulator compatibility are separate from recorder compatibility. The guide and AI teammates stay
    // off for this pin until a policy is retrained on it; tests/PackagingCheck reads the same flag from the DLL.
    public static bool PolicyEnabled => Build.RootElement.GetProperty("policy_enabled").GetBoolean();
    public static string VanillaFingerprint
    {
        get
        {
            using var stream = typeof(GameBuild).Assembly.GetManifestResourceStream("HumanPlayRecorder.vanilla-content.json")
                ?? throw new InvalidOperationException("vanilla catalog missing");
            using var catalog = JsonDocument.Parse(stream);
            return catalog.RootElement.GetProperty("fingerprint").GetString()!;
        }
    }
}
