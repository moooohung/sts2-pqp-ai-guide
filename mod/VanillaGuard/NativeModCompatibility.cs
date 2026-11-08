using System.Reflection;
using MegaCrit.Sts2.Core.Modding;

namespace HumanPlayRecorder.VanillaGuard;

public static class NativeModCompatibility
{
    public static Mod? Owner(Assembly assembly) =>
        ModManager.GetLoadedMods().FirstOrDefault(mod => mod.assemblies.Contains(assembly));

    // This is the game's manifest declaration, not an inference from patch targets.
    public static bool Preserves(Assembly assembly) => Owner(assembly)?.manifest?.affectsGameplay == false;
}
