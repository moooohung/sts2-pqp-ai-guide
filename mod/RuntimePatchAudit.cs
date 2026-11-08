using System.Reflection;
using HarmonyLib;
using MegaCrit.Sts2.Core.Localization;
using MegaCrit.Sts2.Core.Modding;
using MegaCrit.Sts2.Core.Models;

namespace HumanPlayRecorder;

static class RuntimeGameplayPatchClassifier
{
    public static bool IsGameplay(MethodBase method)
    {
        var type = method.DeclaringType;
        bool model = type is not null && typeof(AbstractModel).IsAssignableFrom(type);
        bool visual = method is MethodInfo info && method.Name.StartsWith("get_", StringComparison.Ordinal) &&
            (typeof(Godot.Resource).IsAssignableFrom(info.ReturnType) || info.ReturnType == typeof(LocString));
        return GameplayPatchClassifier.IsGameplay(new(type?.FullName ?? "unknown", method.Name, model, visual));
    }
}

public static class RuntimePatchAudit
{
    public static PatchAuditEntry[] Entries()
    {
        var mods=ModManager.GetLoadedMods().ToArray();
        var result=new List<PatchAuditEntry>();
        foreach(var original in Harmony.GetAllPatchedMethods().ToArray())
        {
            var info=Harmony.GetPatchInfo(original);if(info is null)continue;
            void Add(IEnumerable<Patch> patches,string kind)
            {
                foreach(var p in patches)
                {
                    var assembly=p.PatchMethod.Module.Assembly;
                    if(assembly==typeof(RecorderMod).Assembly)continue;
                    string id=mods.FirstOrDefault(m=>m.assemblies.Contains(assembly))?.manifest?.id??"unattributed";
                    string signature=(original.DeclaringType?.FullName??"unknown")+"."+original.Name+"("+
                        string.Join(",",original.GetParameters().Select(p=>p.ParameterType.FullName??p.ParameterType.Name))+")";
                    result.Add(new(id,signature,kind));
                }
            }
            Add(info.Prefixes,"prefix");Add(info.Postfixes,"postfix");
            Add(info.Transpilers,"transpiler");Add(info.Finalizers,"finalizer");
        }
        return result.Distinct().OrderBy(p=>p.mod_id,StringComparer.Ordinal)
            .ThenBy(p=>p.method,StringComparer.Ordinal).ThenBy(p=>p.kind,StringComparer.Ordinal).ToArray();
    }
    static int scheduled;
    public static void Schedule()
    {
        if (Interlocked.Exchange(ref scheduled, 1) != 0) return;
        _ = Task.Run(() => {
            try { foreach (string row in Capture()) Godot.GD.Print(row); }
            catch (Exception e) { Godot.GD.Print("[HumanPlayRecorder] mod audit unavailable: " + e.GetType().Name); }
        });
    }
    public static string[] Capture()
    {
        var mods = ModManager.GetLoadedMods().ToArray();
        var offenders = mods.ToDictionary(m => m, _ => new SortedSet<string>(StringComparer.Ordinal));
        var unknown = new SortedSet<string>(StringComparer.Ordinal);
        var self = typeof(RecorderMod).Assembly;
        foreach (var original in Harmony.GetAllPatchedMethods().ToArray())
        {
            if (!RuntimeGameplayPatchClassifier.IsGameplay(original)) continue;
            var info = Harmony.GetPatchInfo(original);
            if (info is null) continue;
            foreach (var patch in info.Prefixes.Concat(info.Postfixes).Concat(info.Transpilers).Concat(info.Finalizers))
            {
                var assembly = patch.PatchMethod.DeclaringType?.Assembly ?? patch.PatchMethod.Module.Assembly;
                if (assembly == self) continue;
                var owners = mods.Where(m => m.assemblies.Contains(assembly)).ToArray();
                string target = (original.DeclaringType?.FullName ?? "unknown") + "." + original.Name;
                if (owners.Length == 0) { unknown.Add(target); continue; }
                foreach (var owner in owners) offenders[owner].Add(target);
            }
        }
        var rows = new List<string>();
        foreach (var mod in mods.OrderBy(m => m.manifest?.id, StringComparer.Ordinal))
        {
            string id = (mod.manifest?.id ?? "unknown_mod").Replace('\r', '_').Replace('\n', '_');
            string? hash = AuditedCompatibleMods.ReadCandidateHash(id, mod.path, mod.assemblies.Count);
            string category = AuditedCompatibleMods.Matches(id, hash) ? "audited-compatible" :
                offenders[mod].Count == 0 ? "no gameplay patches" : "patches gameplay methods (replay decides)";
            string evidence = offenders[mod].Count == 0 ? "none observed" :
                string.Join(";", offenders[mod].Take(20));
            rows.Add($"[HumanPlayRecorder] mod audit: id={id}; category={category}; gameplay_targets={offenders[mod].Count}; evidence={evidence}");
        }
        foreach (string target in unknown)
            rows.Add("[HumanPlayRecorder] mod audit: unattributed gameplay patch; target=" + target);
        return rows.ToArray();
    }
}
