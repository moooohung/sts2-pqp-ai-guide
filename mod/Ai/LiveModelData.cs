using System.Reflection;
using System.Runtime.CompilerServices;
using HarmonyLib;
using MegaCrit.Sts2.Core.Combat;
using MegaCrit.Sts2.Core.Entities.Cards;
using MegaCrit.Sts2.Core.Localization.DynamicVars;
using MegaCrit.Sts2.Core.Models;

namespace HumanPlayRecorder.Ai;

// Observe values the game already evaluated; never evaluate an external getter
// or a global card-cost/keyword hook to construct an advisor snapshot.
internal static class LiveModelData
{
    sealed record Value(object? Data, ICombatState? Combat, Task? Refresh);
    sealed class Values { public readonly Dictionary<string, Value> Items = new(); }
    static readonly ConditionalWeakTable<AbstractModel, Values> observed = new();
    static readonly FieldInfo refreshField = AccessTools.Field(typeof(CombatStateTracker), "_combatStateChangedDeferredTask");
    static readonly FieldInfo energyCard = AccessTools.Field(typeof(CardEnergyCost), "_card");
    static readonly string[] cardProperties = [
        "Type", "Rarity", "TargetType", "Pool", "Tags", "CanonicalTags", "CanonicalKeywords",
        "CanonicalEnergyCost", "HasEnergyCostX", "CanonicalStarCost", "CurrentStarCost", "HasStarCostX"
    ];

    static Harmony? installed;

    internal static void Install(Harmony harmony)
    {
        installed = harmony;
        // Mod subtypes are enumerable only after ModManager finishes; mods
        // initialize inside it, so observe them once the game builds ModelDb.
        harmony.Patch(AccessTools.Method(typeof(ModelDb), nameof(ModelDb.Init)),
            postfix: new HarmonyMethod(typeof(LiveModelData), nameof(ObserveModTypes)));
        harmony.Patch(AccessTools.Method(typeof(ModelDb), nameof(ModelDb.Inject)),
            postfix: new HarmonyMethod(typeof(LiveModelData), nameof(Injected)));
        harmony.Patch(AccessTools.Method(typeof(CardEnergyCost), nameof(CardEnergyCost.GetWithModifiers)),
            postfix: new HarmonyMethod(typeof(LiveModelData), nameof(EnergyComputed)));
        harmony.Patch(AccessTools.Method(typeof(CardModel), nameof(CardModel.GetStarCostWithModifiers)),
            postfix: new HarmonyMethod(typeof(LiveModelData), nameof(StarsComputed)));
        harmony.Patch(AccessTools.Method(typeof(CardModel), nameof(CardModel.GetKeywordsWithSources)),
            postfix: new HarmonyMethod(typeof(LiveModelData), nameof(KeywordsComputed)));
    }
    static void ObserveModTypes()
    {
        try { foreach (var type in ModelDb.AllAbstractModelSubtypes) ObserveType(installed!, type); }
        catch (Exception e) { Godot.GD.Print("[HumanPlayRecorder] advisor mod data observation failed " + RunStartDiagnostics.Describe(e)); }
    }
    static void Injected(Type type)
    {
        try { ObserveType(new Harmony(Edition.ModDataHarmonyId), type); }
        catch (Exception) { } // Optional observation must not break mod registration.
    }
    static void ObserveType(Harmony harmony, Type type)
    {
        if (type.Assembly == typeof(CardModel).Assembly) return;
        var names = typeof(CardModel).IsAssignableFrom(type) ? cardProperties :
            typeof(CharacterModel).IsAssignableFrom(type) ? new[] { "CardPool" } : [];
        foreach (string name in names) {
            var getter = AccessTools.DeclaredProperty(type, name)?.GetMethod;
            if (getter is null || getter.IsAbstract ||
                Harmony.GetPatchInfo(getter)?.Postfixes.Any(p => p.PatchMethod.DeclaringType == typeof(LiveModelData)) == true) continue;
            harmony.Patch(getter, postfix: new HarmonyMethod(typeof(LiveModelData)
                .GetMethod(nameof(PropertyComputed), BindingFlags.Static | BindingFlags.NonPublic)!
                .MakeGenericMethod(getter.ReturnType)));
        }
    }
    static Task? Refresh() => (Task?)refreshField.GetValue(CombatManager.Instance.StateTracker);
    static void Remember(AbstractModel model, string key, object? value)
    {
        try {
            FrameCaptureBudget.Check();
            var combat = (model as CardModel)?.CombatState;
            observed.GetValue(model, _ => new Values()).Items[key] = new(value, combat, combat is null ? null : Refresh());
        } catch (Exception) { } // A failed observer cannot affect the original operation.
    }
    static void PropertyComputed<T>(AbstractModel __instance, MethodBase __originalMethod, T __result) =>
        Remember(__instance, __originalMethod.Name[4..], __result);
    static void EnergyComputed(CardEnergyCost __instance, CostModifiers modifiers, int __result)
    {
        try {
            if (modifiers == CostModifiers.All && energyCard.GetValue(__instance) is CardModel card)
                Remember(card, "energy_cost", __result);
        } catch (Exception) { }
    }
    static void StarsComputed(CardModel __instance, int __result) => Remember(__instance, "star_cost", __result);
    static void KeywordsComputed(CardModel __instance, KeywordSources sources, IReadOnlySet<CardKeyword> __result)
    {
        if (sources == KeywordSources.All) Remember(__instance, "keywords", __result);
    }
    static bool TryObserved<T>(AbstractModel model, string key, out T result)
    {
        if (observed.TryGetValue(model, out var values) && values.Items.TryGetValue(key, out var value) &&
            (model is not CardModel card || card.CombatState is null ||
                ReferenceEquals(value.Combat, card.CombatState) &&
                ReferenceEquals(value.Refresh, Refresh()) && value.Refresh is not { IsCompletedSuccessfully: false }) &&
            value.Data is T typed) {
            result = typed;
            return true;
        }
        result = default!;
        return false;
    }
    internal static AdviceUnavailableException Missing(AbstractModel model, string field) =>
        new(AdviceReason.StateNotExposed, $"추천 불가: {model.Id.Entry}.{field}: 게임이 현재 데이터 값을 제공하지 않았습니다.");
    // Reflection lookups are fixed per type once ModelDb is built; resolving them per
    // card read was a tenth of every capture.
    sealed record Getter(PropertyInfo Property, bool GameDeclared, FieldInfo? Backing);
    static readonly System.Collections.Concurrent.ConcurrentDictionary<(Type, string), Getter> getters = new();
    static readonly System.Collections.Concurrent.ConcurrentDictionary<string, FieldInfo> cardFields = new();
    static Getter Resolve(Type type, string name) => getters.GetOrAdd((type, name), key => {
        var property = AccessTools.Property(key.Item1, key.Item2);
        var getter = property.GetMethod!;
        bool game = getter.DeclaringType!.Assembly == typeof(CardModel).Assembly;
        // Auto-properties already contain data, even if declared by a mod.
        var backing = game ? null : getter.DeclaringType.GetField("<" + key.Item2 + ">k__BackingField",
            BindingFlags.Instance | BindingFlags.NonPublic);
        return new(property, game, backing?.IsDefined(typeof(CompilerGeneratedAttribute)) == true ? backing : null);
    });
    internal static T Property<T>(AbstractModel model, string name, Func<T> gameRead)
    {
        FrameCaptureBudget.Check();
        if (!FrameCaptureBudget.Active) return gameRead();
        var getter = Resolve(model.GetType(), name);
        if (getter.GameDeclared) return gameRead();
        if (getter.Backing?.GetValue(model) is T stored) return stored;
        if (TryObserved<T>(model, name, out var observedValue)) return observedValue;
        // This may run another mod's getter. The game itself calls the same model getters during
        // play, so reading them here adds no network path of this mod's own (offline gate scope).
        return gameRead();
    }
    static FieldInfo CardField(string name) => cardFields.GetOrAdd(name, n => AccessTools.Field(typeof(CardModel), n));
    static T? Field<T>(CardModel card, string name) where T : class => CardField(name).GetValue(card) as T;
    static T Protected<T>(CardModel card, string name) =>
        Property(card, name, () => (T)Resolve(card.GetType(), name).Property.GetValue(card)!);
    internal static DynamicVarSet Variables(CardModel card)
    {
        if (!FrameCaptureBudget.Active) return card.DynamicVars;
        return Field<DynamicVarSet>(card, "_dynamicVars") ?? card.DynamicVars;
    }
    internal static CardEnergyCost Energy(CardModel card)
    {
        if (!FrameCaptureBudget.Active) return card.EnergyCost;
        if (Field<CardEnergyCost>(card, "_energyCost") is { } cost) return cost;
        // The lazy getter would dispatch CanonicalEnergyCost/HasEnergyCostX.
        int canonical = Protected<int>(card, "CanonicalEnergyCost");
        bool x = Protected<bool>(card, "HasEnergyCostX");
        return new CardEnergyCost(card, canonical, x);
    }
    // The game evaluates costs, keywords and previews only for rendered cards; for the
    // rest run the engine's own evaluation, including mod code (user decision 2026-11-06:
    // advice everywhere, mod code may run when the game has not published a value).
    internal static int EnergyCost(CardModel card)
    {
        FrameCaptureBudget.Check();
        if (FrameCaptureBudget.Active && card.CombatState is not null)
            return TryObserved<int>(card, "energy_cost", out var value) ? value
                : card.EnergyCost.GetWithModifiers(CostModifiers.All);
        return Energy(card).GetWithModifiers(CostModifiers.Local);
    }
    internal static int StarCost(CardModel card)
    {
        FrameCaptureBudget.Check();
        if (FrameCaptureBudget.Active && card.CombatState is not null)
            return TryObserved<int>(card, "star_cost", out var value) ? value
                : card.GetStarCostWithModifiers();
        if (!FrameCaptureBudget.Active) return card.CurrentStarCost;
        return Property(card, "CurrentStarCost", () => {
            int baseCost = (bool)CardField("_starCostSet").GetValue(card)!
                ? (int)CardField("_baseStarCost").GetValue(card)!
                : Protected<int>(card, "CanonicalStarCost");
            int? temporary = card.TemporaryStarCost?.Cost;
            return temporary is not { } cost || cost == 0 && baseCost < 0 ? baseCost : cost;
        });
    }
    internal static IReadOnlySet<CardKeyword> Keywords(CardModel card)
    {
        FrameCaptureBudget.Check();
        if (FrameCaptureBudget.Active && card.CombatState is not null) {
            if (TryObserved<IReadOnlySet<CardKeyword>>(card, "keywords", out var value) &&
                value.GetType() == typeof(HashSet<CardKeyword>)) return value;
            return card.GetKeywordsWithSources(KeywordSources.All);
        }
        if (!FrameCaptureBudget.Active) return card.Keywords;
        if (Field<HashSet<CardKeyword>>(card, "_keywords") is { } keywords) return keywords;
        return Protected<IEnumerable<CardKeyword>>(card, "CanonicalKeywords").ToHashSet();
    }
    internal static IEnumerable<CardTag> Tags(CardModel card)
    {
        if (!FrameCaptureBudget.Active) return card.Tags;
        return Resolve(card.GetType(), "Tags").Property.GetMethod!.DeclaringType == typeof(CardModel)
            ? Field<HashSet<CardTag>>(card, "_tags") ?? Protected<HashSet<CardTag>>(card, "CanonicalTags")
            : Property(card, "Tags", () => card.Tags).ToArray();
    }
    internal static bool Retained(CardModel card, IReadOnlySet<CardKeyword> keywords) =>
        keywords.Contains(CardKeyword.Retain) ||
        (bool)CardField("_hasSingleTurnRetain").GetValue(card)!;
    internal static T Item<T>(AbstractModel model, string field, Func<T> read)
    {
        try {
            FrameCaptureBudget.Check();
            T value = read();
            FrameCaptureBudget.Check();
            return value;
        } catch (Exception error) {
            // A budget refusal inside the read keeps its category; any other failure is a value the game did not give.
            throw new AdviceUnavailableException(AdviceUnavailableException.ReasonOf(error, AdviceReason.StateNotExposed),
                $"추천 불가: {model.Id.Entry}.{field}: {error.Message}");
        }
    }
}
