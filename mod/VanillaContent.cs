using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using MegaCrit.Sts2.Core.Localization.DynamicVars;
using MegaCrit.Sts2.Core.Models;

namespace HumanPlayRecorder;

public sealed record VanillaCatalog(string game_hash, string fingerprint, SortedDictionary<string, string> models);

public static class VanillaContent
{
    public static bool Relevant(AbstractModel m) => !m.IsMock && m is (CardModel or RelicModel or PotionModel or PowerModel or
        EventModel or MonsterModel or EncounterModel or CharacterModel or ActModel or CardPoolModel or RelicPoolModel or PotionPoolModel or
        AfflictionModel or EnchantmentModel or OrbModel or ModifierModel);
    // Audited canonical gameplay properties, not localized strings, art or mutable run state.
    static readonly string[] Properties = [
        "Type","Rarity","MultiplayerConstraint","CanonicalEnergyCost","HasEnergyCostX","CanonicalStarCost",
        "HasStarCostX","TargetType","CanonicalKeywords","CanonicalTags","MaxUpgradeLevel","CanBeGeneratedInCombat",
        "CanBeGeneratedByModifiers","GainsBlock","IsAllowedInShops","HasUponPickupEffect","IsStackable",
        "SpawnsPets","AddsPet","MerchantCost","Usage","InstanceType","StackType","IsDeterministic","IsShared",
        "StartingHp","StartingGold","MaxEnergy","BaseOrbSlotCount","StartingDeck","StartingRelics","StartingPotions",
        "MinInitialHp","MaxInitialHp","RoomType","IsWeak","ShouldGiveRewards","MinGoldReward","MaxGoldReward",
        "AllPossibleMonsters","Tags","AllCards","AllRelics","AllPotions","Index","IsDefault","BaseNumberOfRooms",
        "NumberOfWeakEncounters","AllEncounters","AllAncients","AllEvents","IsColorless"
    ];
    static PropertyInfo? Find(Type type, string name)
    {
        for (Type? t=type; t is not null; t=t.BaseType)
        {
            var p=t.GetProperty(name,BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.Instance|BindingFlags.DeclaredOnly);
            if(p is not null) return p;
        }
        return null;
    }
    static object? Value(object? v) => v switch {
        null => null, AbstractModel m => m.Id.ToString(), ModelId id => id.ToString(),
        string s => s, Enum e => e.ToString(), bool or byte or int or long or float or double or decimal => v,
        IEnumerable list => list.Cast<object?>().Select(Value).ToArray(),
        _ => throw new InvalidOperationException("unsupported canonical value: "+v.GetType().FullName)
    };
    public static string ModelHash(AbstractModel model)
    {
        var data=new SortedDictionary<string,object?>(StringComparer.Ordinal) {
            ["id"]=model.Id.ToString(),["type"]=model.GetType().FullName
        };
        foreach(string name in Properties)
        {
            var p=Find(model.GetType(),name);
            if(p?.GetMethod is null || p.GetIndexParameters().Length!=0) continue;
            data[name]=Value(p.GetValue(model));
        }
        var canonical=Find(model.GetType(),"CanonicalVars")?.GetValue(model) as IEnumerable<DynamicVar>;
        if(canonical is not null) data["vars"]=canonical.Select(v=>new {
            name=v.Name,type=v.GetType().FullName,value=v.BaseValue.ToString(CultureInfo.InvariantCulture)
        }).OrderBy(v=>v.name,StringComparer.Ordinal).ToArray();
        // Method bytecode seals model-specific rules as well as exposed constants.
        data["rules"]=model.GetType().GetMethods(BindingFlags.Public|BindingFlags.NonPublic|BindingFlags.Instance|BindingFlags.DeclaredOnly)
            .Where(m=>RuntimeGameplayPatchClassifier.IsGameplay(m) && m.GetMethodBody() is not null)
            .OrderBy(m=>m.ToString(),StringComparer.Ordinal)
            .Select(m=>m.ToString()+":"+Convert.ToHexString(SHA256.HashData(m.GetMethodBody()!.GetILAsByteArray()!))).ToArray();
        return Hash(JsonSerializer.SerializeToUtf8Bytes(data));
    }
    public static string Hash(byte[] bytes)=>Convert.ToHexStringLower(SHA256.HashData(bytes));
    public static VanillaCatalog Generate()
    {
        var models=new SortedDictionary<string,string>(StringComparer.Ordinal);
        foreach(var m in ModelDb.All.Where(Relevant).OrderBy(m=>m.Id.ToString(),StringComparer.Ordinal))
            models.Add(m.Id.ToString(),ModelHash(m));
        string game=Hash(File.ReadAllBytes(typeof(ModelDb).Assembly.Location));
        return new(game,Hash(Encoding.UTF8.GetBytes(string.Join("\n",models.Select(p=>p.Key+"="+p.Value)))),models);
    }
    public static VanillaCatalog Load()
    {
        using var s=typeof(VanillaContent).Assembly.GetManifestResourceStream("HumanPlayRecorder.vanilla-content.json")
            ?? throw new InvalidOperationException("vanilla catalog missing");
        return JsonSerializer.Deserialize<VanillaCatalog>(s)!;
    }
}
