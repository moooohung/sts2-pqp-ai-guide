using System.Runtime.CompilerServices;
using MegaCrit.Sts2.Core.Entities.Cards;
using MegaCrit.Sts2.Core.Entities.Creatures;
using MegaCrit.Sts2.Core.Localization.DynamicVars;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.TestSupport;

namespace HumanPlayRecorder.Ai;

public static partial class LiveObservation
{
    sealed record DisplayValue(string Name, decimal PreviewValue, decimal EnchantedValue, bool Damage, bool Block);
    sealed class PreviewEntries
    {
        public readonly Dictionary<object, DisplayValue[]> Targets = new(ReferenceEqualityComparer.Instance);
    }
    static readonly object noTarget = new();
    static readonly ConditionalWeakTable<CardModel, PreviewEntries> previews = new();
    internal static void InvalidatePreviews() => previews.Clear();
    internal static bool ObserveEnginePreview(CardModel card, Creature? target, DynamicVarSet variables)
    {
        try {
            using var budget = new FrameCaptureBudget();
            var values = DisplayValues(variables);
            FrameCaptureBudget.Check();
            var cache = previews.GetValue(card, _ => new PreviewEntries());
            object key = target ?? noTarget;
            if (cache.Targets.TryGetValue(key, out var previous) && previous.SequenceEqual(values)) return false;
            cache.Targets[key] = values;
            return true;
        } catch (Exception) { return false; } // A UI preview failure must never escape into game code.
    }
    static DisplayValue[] DisplayValues(DynamicVarSet variables)
    {
        var result = new List<DisplayValue>();
        foreach (var variable in variables.Values) {
            FrameCaptureBudget.Check();
            if (IsDisplayVariable(variable))
                result.Add(new(variable.Name, variable.PreviewValue, variable.EnchantedValue,
                    variable is DamageVar or OstyDamageVar or CalculatedDamageVar, variable is BlockVar or CalculatedBlockVar));
        }
        return result.ToArray();
    }
    static IReadOnlyList<DisplayValue> Preview(CardModel card, Creature? target)
    {
        FrameCaptureBudget.Check();
        if (FrameCaptureBudget.Active) {
            if (previews.TryGetValue(card, out var cached) &&
                cached.Targets.TryGetValue(target ?? noTarget, out var values)) return values;
            var variables = LiveModelData.Variables(card).Values;
            if (card.Pile?.IsCombatPile != true && card.Enchantment is null &&
                !variables.Any(v => v is CalculatedVar) &&
                variables.Where(IsDisplayVariable).All(v => v.GetType().GetMethod(nameof(DynamicVar.UpdateCardPreview))!
                    .DeclaringType!.Assembly == typeof(DynamicVar).Assembly)) {
                var intrinsic = new List<DisplayValue>();
                foreach (var variable in variables) {
                    FrameCaptureBudget.Check();
                    if (IsDisplayVariable(variable)) intrinsic.Add(new(variable.Name, variable.BaseValue,
                        variable.BaseValue, variable is DamageVar, variable is BlockVar));
                }
                return intrinsic;
            }
            // The game publishes previews only for rendered cards (the hand). Otherwise
            // run the engine's own preview, including mod preview code, on a clone.
        }
        else if (!TestMode.IsOn)
            throw new AdviceUnavailableException(AdviceReason.PreviewUnavailable, "추천 불가: 프레임 예산 밖의 미리보기 계산은 허용되지 않습니다.");
        // The original-DLL offline oracle is not a running game's frame.
        var vars = card.DynamicVars.Clone(card);
        foreach (var variable in vars.Values.ToArray()) {
            FrameCaptureBudget.Check();
            try { variable.UpdateCardPreview(card, CardPreviewMode.Normal, target, card.Pile?.IsCombatPile == true); }
            catch (Exception) { throw new AdviceUnavailableException(AdviceReason.PreviewUnavailable, "추천 불가: 카드 미리보기를 계산할 수 없습니다."); }
        }
        return DisplayValues(vars);
    }
}
