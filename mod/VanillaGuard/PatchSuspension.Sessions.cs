using System.Runtime.CompilerServices;
using HarmonyLib;

namespace HumanPlayRecorder.VanillaGuard;

// Online edition: suspended patches are added to the uploaded run's final audit.
public static partial class PatchSuspension
{
    static readonly ConditionalWeakTable<LiveRunSession, List<PatchAuditEntry>> sessionAudits = new();

    static partial void InstallSessionAudit(Harmony harmony)
    {
        harmony.Patch(AccessTools.GetDeclaredConstructors(typeof(LiveRunSession)).Single(),
            postfix: new HarmonyMethod(typeof(PatchSuspension), nameof(Session)));
        harmony.Patch(AccessTools.Method(typeof(LiveRunSession), nameof(LiveRunSession.End)),
            prefix: new HarmonyMethod(typeof(PatchSuspension), nameof(FinalAudit)));
    }
    static void Session(LiveRunSession __instance) { if (active) sessionAudits.Add(__instance, audit); }
    static void FinalAudit(LiveRunSession __instance, ref PatchAuditEntry[]? finalAudit)
    {
        if (sessionAudits.TryGetValue(__instance, out var captured))
            lock (Sync) finalAudit = (finalAudit ?? []).Concat(captured).Distinct().ToArray();
    }
}
