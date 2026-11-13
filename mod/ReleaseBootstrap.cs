using System.Reflection;
using System.Runtime.CompilerServices;
using System.Security.Cryptography;
using MegaCrit.Sts2.Core.Modding;

namespace HumanPlayRecorder;

// Keep the loader boundary free of game-state types. Unsupported builds never
// initialize RecorderMod's static fields or install any Harmony patches.
[ModInitializer(nameof(Init))]
public static class ReleaseBootstrap
{
    public static string SupportedGameHash => GameBuild.Sha256;
    public static void Init()
    {
        if (Edition.StandDownReason() is { } standDown)
        {
            Console.WriteLine($"[{Edition.ModId}] disabled: {standDown}.");
            return;
        }
        string hash;
        try
        {
            using var file = File.OpenRead(Assembly.Load("sts2").Location);
            hash = Convert.ToHexStringLower(SHA256.HashData(file));
        }
        catch (Exception)
        {
            Console.WriteLine("[HumanPlayRecorder] disabled: game build could not be identified.");
            return;
        }
        if (hash != SupportedGameHash)
        {
            Console.WriteLine($"[HumanPlayRecorder] disabled: unsupported game build {hash}.");
            return;
        }
        InstallSupportedBuild();
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    static void InstallSupportedBuild() => RecorderMod.Init();
}
