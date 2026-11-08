using System.Reflection;
using HarmonyLib;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Modding;

namespace HumanPlayRecorder;

public sealed partial class PerRunContentGate
{
    readonly VanillaCatalog baseline;
    readonly Action<string> log;
    readonly Dictionary<string,AbstractModel> seen=new(StringComparer.Ordinal);
    public bool Trainable {get;private set;}=true;
    public string Fingerprint=>baseline.fingerprint;
    public string? Failure {get;private set;}
    public PerRunContentGate(VanillaCatalog baseline,Action<string>? log=null)
    {this.baseline=baseline;this.log=log??(s=>Godot.GD.Print(s));}
    public void Observe(AbstractModel model)
    {
        if(!Trainable) return;
        if(model.IsMock) { Reject("mock content "+model.Id); return; }
        if(!VanillaContent.Relevant(model)) return;
        var canonical=ModelDb.All.FirstOrDefault(m=>m.Id==model.Id) ?? model;
        string id=canonical.Id.ToString();
        if(seen.ContainsKey(id)) return;
        seen[id]=canonical;
        if(!baseline.models.TryGetValue(id,out var expected) || expected!=VanillaContent.ModelHash(canonical))
        {
            var owner=ModManager.GetLoadedMods().FirstOrDefault(m=>m.assemblies.Contains(canonical.GetType().Assembly));
            if (owner?.manifest?.id is { } ownerId) RememberOwner(ownerId);
            Reject($"content {id}; mod {owner?.manifest?.id ?? (canonical.GetType().Assembly==typeof(ModelDb).Assembly?"vanilla-modified":"unattributed")}");
        }
    }
    public void Reject(string reason)
    {
        if(!Trainable) return;
        Trainable=false; Failure=reason;
        log("[HumanPlayRecorder] run excluded: "+reason);
    }
    public void Finish()
    {
        foreach(var p in seen.ToArray())
            if(!baseline.models.TryGetValue(p.Key,out var expected) || expected!=VanillaContent.ModelHash(p.Value))
            {
                var owner = ModManager.GetLoadedMods().FirstOrDefault(m => m.assemblies.Contains(p.Value.GetType().Assembly));
                if (owner?.manifest?.id is { } ownerId) RememberOwner(ownerId);
                Reject("content changed at run end "+p.Key);
            }
    }
    // Online edition: remembered for the start-up recording consent (PerRunContentGate.Sharing.cs).
    static partial void RememberOwner(string id);
    public static PerRunContentGate? Active;
    public static void Seen(AbstractModel model)=>ObserveSafe(model);
    [ThreadStatic] static bool inspecting;
    static void Cloning(AbstractModel __instance)=>ObserveSafe(__instance);
    static void ModelUse(AbstractModel __instance)
    {
        if(__instance.IsMutable) ObserveSafe(__instance);
    }
    static void ObserveSafe(AbstractModel m)
    {
        if(inspecting || Active is null) return;
        inspecting=true;
        try { Active.Observe(m); }
        catch(Exception e) { Active.Reject("content validation unavailable "+e.GetType().Name); }
        finally { inspecting=false; }
    }
    public static void Install(Harmony h)
    {
        h.Patch(typeof(AbstractModel).GetMethod(nameof(AbstractModel.MutableClone))!,
            prefix:new HarmonyMethod(typeof(PerRunContentGate),nameof(Cloning)));
        // Mutable offers/encounters are observed on clone and subsequent gameplay
        // use. Pure registry/library enumeration is deliberately not run content.
        foreach(var type in new[]{typeof(AbstractModel),typeof(CardModel),typeof(RelicModel),typeof(PotionModel),
            typeof(PowerModel),typeof(EventModel),typeof(MonsterModel),typeof(EncounterModel),
            typeof(AfflictionModel),typeof(EnchantmentModel),typeof(OrbModel),typeof(ModifierModel)})
        foreach(var m in type.GetMethods(BindingFlags.Instance|BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.DeclaredOnly)
            .Where(m=>!m.IsAbstract&&!m.ContainsGenericParameters&&m.GetMethodBody() is not null&&RuntimeGameplayPatchClassifier.IsGameplay(m)))
        {
            if(m.Name is "get_Id" or "get_IsMutable" or "get_IsCanonical" || m.Name.StartsWith("Assert",StringComparison.Ordinal))continue;
            h.Patch(m,prefix:new HarmonyMethod(typeof(PerRunContentGate),nameof(ModelUse)));
        }
    }
}
