using HarmonyLib;
using MegaCrit.Sts2.Core.Modding;
using MegaCrit.Sts2.Core.Models;

namespace HumanPlayRecorder.VanillaGuard;

public static class RegisteredModelCensus
{
    public static string[] Capture()
    {
        var mods = ModManager.GetLoadedMods().ToArray();
        var counts = ModelDb.All.GroupBy(model => model.GetType().Assembly)
            .ToDictionary(group => group.Key, group => group.Count());
        string Clean(string value) => value.Replace('\r', '_').Replace('\n', '_');
        var rows = mods.OrderBy(mod => mod.manifest?.id, StringComparer.Ordinal).Select(mod =>
            $"[HumanPlayRecorder] model census: mod={Clean(mod.manifest?.id ?? "unknown")}; " +
            $"models={mod.assemblies.Distinct().Sum(assembly => counts.GetValueOrDefault(assembly))}; " +
            $"affects_gameplay={mod.manifest?.affectsGameplay ?? true}").ToList();
        foreach (var (assembly, count) in counts.OrderBy(pair => pair.Key.FullName, StringComparer.Ordinal))
            if (assembly != typeof(ModelDb).Assembly && !mods.Any(mod => mod.assemblies.Contains(assembly)))
                rows.Add($"[HumanPlayRecorder] model census: mod=unattributed; assembly={Clean(assembly.GetName().Name ?? "unknown")}; models={count}");
        return rows.ToArray();
    }
    internal static void Install(Harmony harmony) =>
        harmony.Patch(AccessTools.Method(typeof(RuntimePatchAudit), nameof(RuntimePatchAudit.Capture)),
            postfix: new HarmonyMethod(typeof(RegisteredModelCensus), nameof(Append)));
    static void Append(ref string[] __result) => __result = __result.Concat(Capture()).ToArray();
}
