using System.Reflection;
using System.Runtime.CompilerServices;
using HarmonyLib;
using MegaCrit.Sts2.Core.Modding;
using MegaCrit.Sts2.Core.Models;

namespace HumanPlayRecorder.VanillaGuard;

public static class PatchSuspension
{
    // Hard ABI pin: the game's Harmony 2.4.2 PatchInfo/UpdateWrapper contract.
    // Re-adding HarmonyMethods loses patch indices and inner-patch metadata.
    static readonly Assembly Game = typeof(ModelDb).Assembly;
    static readonly Assembly Self = typeof(PatchSuspension).Assembly;
    static readonly Type Shared = typeof(Harmony).Assembly.GetType("HarmonyLib.HarmonySharedState", true)!;
    static readonly MethodInfo Get = AccessTools.Method(Shared, "GetPatchInfo");
    static readonly MethodInfo Set = AccessTools.Method(Shared, "UpdatePatchInfo");
    static readonly MethodInfo Update = AccessTools.Method(
        typeof(Harmony).Assembly.GetType("HarmonyLib.PatchFunctions", true)!, "UpdateWrapper");
    static readonly object Sync = AccessTools.Field(typeof(PatchProcessor), "locker").GetValue(null)!;
    static readonly FieldInfo[] Lists = typeof(PatchInfo).GetFields().Where(f => f.FieldType == typeof(Patch[])).ToArray();
    // Shared state serializes only module/token, losing constructed generic
    // arguments. Keep the actual Patch objects published with each live wrapper.
    static readonly Dictionary<MethodBase, PatchInfo> published = [];
    static readonly Dictionary<MethodBase, Dictionary<FieldInfo, List<Patch>>> removed = [];
    static List<PatchAuditEntry> audit = [];
    static readonly ConditionalWeakTable<LiveRunSession, List<PatchAuditEntry>> sessionAudits = new();
    static bool active, changing, runPhase;

    public static PatchAuditEntry[] Audit { get { lock (Sync) return audit.ToArray(); } }
    internal static bool InRun => runPhase;

    // The registry and its ID aliases belong to the process, not the run.
    // Presentation consumers still enumerate mod characters and resolve their
    // pools here. Filter generation candidates instead of removing registration.
    static bool IsRegistry(MethodBase method) => method.DeclaringType == typeof(ModelDb) &&
        method.Name is "Get" or "GetId" or "GetEntry" or "GetCategory" or "GetCategoryType" or
            "GetById" or "GetByIdOrNull" or "Contains" or "Inject" or "Remove" or "Init" or "InitIds" or
            "get_AllCharacters" or "get_All";

    // Once a guarded run starts, a gameplay mod's Node patches are suspended too:
    // they can mutate run state (HextechRunes reattaches its modifier from
    // NEventRoom.Proceed). Menus before launch keep them so lobby UI stays intact.
    static bool IsRunTarget(MethodBase method) => IsLogicTarget(method) ||
        (runPhase && method.DeclaringType?.Assembly == Game && !IsRegistry(method));

    public static bool IsLogicTarget(MethodBase method)
    {
        if (WireInterop.IsSchemaTarget(method)) return true;
        if (MenuLifecycle.RunEntrypoints.Contains(method)) return true;
        var type = method.DeclaringType;
        if (type is null || type.Assembly != Game) return false;
        if (IsRegistry(method)) return false;
        // Generated closures/async state machines inherit their enclosing type's
        // domain rather than being mistaken for logic because they derive Object.
        while (type.DeclaringType is { } enclosing) type = enclosing;
        if (typeof(Godot.GodotObject).IsAssignableFrom(type)) return false;
        var nodes = typeof(MegaCrit.Sts2.Core.Nodes.NGame).Namespace!;
        if (type.Namespace == nodes || type.Namespace?.StartsWith(nodes + ".", StringComparison.Ordinal) == true)
            return false;
        if (typeof(AbstractModel).IsAssignableFrom(type)) return true;
        return RuntimeGameplayPatchClassifier.IsGameplay(method);
    }

    static bool Foreign(Patch patch, MethodBase target)
    {
        var assembly = patch.PatchMethod.Module.Assembly;
        return assembly != Self && assembly != Game &&
            (WireInterop.IsSchemaTarget(target) || !NativeModCompatibility.Preserves(assembly));
    }
    public static bool HasForeignPatches(MethodBase target)
    {
        var info = (PatchInfo?)Get.Invoke(null, [target]);
        return info is not null && Lists.Any(f => ((Patch[])f.GetValue(info)!).Any(p => Foreign(p, target)));
    }

    public static void Install(Harmony harmony)
    {
        // Both individual and class/assembly patch processors publish here.
        harmony.Patch(Set,
            postfix: new HarmonyMethod(typeof(PatchSuspension), nameof(AfterPatch)));
        harmony.Patch(AccessTools.Method(typeof(RecorderMod), "CreateRunHeader"),
            postfix: new HarmonyMethod(typeof(PatchSuspension), nameof(Header)));
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

    static void Header(ref RunHeader __result) => __result = Stamp(__result);
    public static RunHeader Stamp(RunHeader header) => !active ? header : header with {
        patch_audit = (header.patch_audit ?? []).Concat(Audit).Distinct()
            .OrderBy(p => p.mod_id, StringComparer.Ordinal).ThenBy(p => p.method, StringComparer.Ordinal)
            .ThenBy(p => p.kind, StringComparer.Ordinal).ToArray()
    };

    static PatchInfo Copy(PatchInfo info)
    {
        var copy = new PatchInfo { VersionCount = info.VersionCount };
        foreach (var field in Lists) field.SetValue(copy, ((Patch[])field.GetValue(info)!).ToArray());
        return copy;
    }

    static PatchInfo? Current(MethodBase target)
    {
        var current = (PatchInfo?)Get.Invoke(null, [target]);
        return current is not null && published.TryGetValue(target, out var captured) &&
            captured.VersionCount == current.VersionCount ? Copy(captured) : current;
    }

    static void AfterPatch(MethodBase __0, PatchInfo __2)
    {
        try
        {
            if (!IsLogicTarget(__0) && (__0.DeclaringType?.Assembly != Game || IsRegistry(__0))) return;
            lock (Sync)
            {
                // Capture while inactive too, and retain our own publications so
                // the next run does not deserialize the restored patches again.
                published[__0] = Copy(__2);
                if (active && !changing && IsRunTarget(__0)) Suspend(__0);
            }
        }
        catch (Exception e) { Abort(e); }
    }

    static void Abort(Exception error)
    {
        ContentPools.SuspensionFailed(error);
        try { ContentPools.End(); }
        catch (Exception restoreError) { ContentPools.SuspensionFailed(restoreError); }
    }

    public static bool Begin()
    {
        lock (Sync)
        {
            try
            {
                if (active || removed.Count != 0)
                    throw new InvalidOperationException("original-rules patch scope active or restoration incomplete");
                audit = [];
                active = true;
                foreach (var target in Harmony.GetAllPatchedMethods().Where(IsLogicTarget).ToArray())
                    Suspend(target);
                return true;
            }
            catch (Exception e)
            {
                Abort(e);
                return false;
            }
        }
    }

    public static void EnterRun()
    {
        lock (Sync)
        {
            if (!active || runPhase) return;
            try
            {
                runPhase = true;
                foreach (var target in Harmony.GetAllPatchedMethods()
                    .Where(m => !IsLogicTarget(m) && IsRunTarget(m)).ToArray())
                    Suspend(target);
            }
            catch (Exception e) { Abort(e); }
        }
    }

    static string Signature(MethodBase method) => (method.DeclaringType?.FullName ?? "") + "." + method.Name +
        "(" + string.Join(",", method.GetParameters().Select(p => p.ParameterType.FullName)) + ")";

    static void Suspend(MethodBase target)
    {
        var current = Current(target);
        if (current is null) return;
        var mods = ModManager.GetLoadedMods().ToArray();
        var original = Copy(current);
        var pending = new Dictionary<FieldInfo, List<Patch>>();
        var annotations = new List<PatchAuditEntry>();
        bool changed = false;
        foreach (var field in Lists)
        {
            var patches = (Patch[])field.GetValue(current)!;
            var foreign = patches.Where(p => Foreign(p, target)).ToArray();
            if (foreign.Length == 0) continue;
            pending.Add(field, foreign.ToList());
            foreach (var patch in foreign)
            {
                if (patch.PatchMethod.ContainsGenericParameters)
                    throw new InvalidOperationException("cannot suspend " + Signature(target) +
                        ": concrete patch method was not captured at publication: " + patch.PatchMethod);
                string id = mods.FirstOrDefault(m => m.assemblies.Contains(patch.PatchMethod.Module.Assembly))
                    ?.manifest?.id ?? "unattributed";
                string kind = field.Name switch {
                    "prefixes" or "innerprefixes" => "prefix",
                    "postfixes" or "innerpostfixes" => "postfix",
                    "transpilers" => "transpiler", _ => "finalizer"
                };
                // Preserve the existing receipt schema. This explicit method
                // annotation distinguishes suspended patches from live patches.
                annotations.Add(new(id, "suspended:" + Signature(target) + "; patch=" +
                    Signature(patch.PatchMethod) + "; owner=" + patch.owner + "; slot=" + field.Name, kind));
            }
            field.SetValue(current, patches.Where(p => !Foreign(p, target)).ToArray());
            changed = true;
        }
        if (!changed) return;
        try { Apply(target, current); }
        catch { Apply(target, original); throw; }
        if (!removed.TryGetValue(target, out var saved)) removed.Add(target, saved = []);
        foreach (var (field, patches) in pending)
        {
            if (!saved.TryGetValue(field, out var entries)) saved.Add(field, entries = []);
            entries.AddRange(patches);
        }
        audit.AddRange(annotations);
    }

    static void Apply(MethodBase target, PatchInfo info)
    {
        changing = true;
        try
        {
            var replacement = (MethodInfo)Update.Invoke(null, [target, info])!;
            Set.Invoke(null, [target, replacement, info]);
        }
        finally { changing = false; }
    }

    public static void End()
    {
        lock (Sync)
        {
            active = runPhase = false;
            var failures = new List<Exception>();
            foreach (var (target, fields) in removed.ToArray())
            {
                try
                {
                    var current = Current(target) ?? new();
                    foreach (var (field, saved) in fields)
                    {
                        var live = (Patch[])field.GetValue(current)!;
                        field.SetValue(current, live.Concat(saved.Where(s => !live.Any(p =>
                            p.index == s.index && p.owner == s.owner && p.PatchMethod == s.PatchMethod)))
                            .OrderBy(p => p.index).ToArray());
                    }
                    Apply(target, current);
                    removed.Remove(target);
                }
                catch (Exception e)
                {
                    var cause = e is TargetInvocationException { InnerException: { } inner } ? inner : e;
                    failures.Add(new InvalidOperationException(
                        Signature(target) + ": " + cause.GetType().Name + ": " + cause.Message, cause));
                }
            }
            if (failures.Count != 0) throw new AggregateException("gameplay patch restoration failed", failures);
        }
    }
}
