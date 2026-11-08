using System.Runtime.CompilerServices;
using HarmonyLib;
using HumanPlayRecorder.Ai;
using MegaCrit.Sts2.Core.Commands;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.Entities.RestSite;
using MegaCrit.Sts2.Core.Events;
using MegaCrit.Sts2.Core.GameActions.Multiplayer;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Models.Monsters;

namespace HumanPlayRecorder;

public static partial class RecorderMod
{
    static readonly AsyncLocal<AbstractModel?> selectionSource = new();
    static readonly ConditionalWeakTable<EventOption, EventModel> optionEvents = new();
    sealed class UnsupportedChoice(string reason)
    {
        public string Reason { get; } = reason;
        public Task? Task;
    }
    static readonly ConditionalWeakTable<Player, UnsupportedChoice> unsupportedChoices = new();

    static void InstallChoiceContexts(Harmony harmony)
    {
        harmony.Patch(AccessTools.Method(typeof(RelicCmd), nameof(RelicCmd.Obtain),
            [typeof(RelicModel), typeof(Player), typeof(int)]),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeObtainingChoiceRelic)),
            finalizer: new HarmonyMethod(typeof(RecorderMod), nameof(RestoreChoiceSource)));
        foreach (var constructor in AccessTools.GetDeclaredConstructors(typeof(EventOption))) {
            string hook = constructor.GetParameters().Any(p => p.ParameterType == typeof(EventModel))
                ? nameof(AfterChoiceEventOption) : nameof(AfterChoiceEventOptionCopy);
            harmony.Patch(constructor, postfix: new HarmonyMethod(typeof(RecorderMod), hook));
        }
        // Harmony shares __state between patches declared by the same type on one
        // method; the recorder's EventOption.Chosen hooks keep a RecordedAction.
        harmony.Patch(AccessTools.Method(typeof(EventOption), nameof(EventOption.Chosen)),
            prefix: new HarmonyMethod(typeof(EventChoiceSourceHooks), nameof(EventChoiceSourceHooks.Before)),
            finalizer: new HarmonyMethod(typeof(EventChoiceSourceHooks), nameof(EventChoiceSourceHooks.Restore)));
        foreach (var method in new[] {
            AccessTools.Method(typeof(EventModel), "SelectCardsToAddToDeckFromGrid"),
            AccessTools.Method(typeof(KnowledgeDemon), "ChooseCurse")
        })
            harmony.Patch(method, prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeModelChoice)),
                finalizer: new HarmonyMethod(typeof(RecorderMod), nameof(RestoreChoiceSource)));
        harmony.Patch(AccessTools.Method(typeof(RelicSelectCmd), nameof(RelicSelectCmd.FromChooseARelicScreen)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeStandaloneRelicChoice)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterUnsupportedChoice)));
        harmony.Patch(AccessTools.Method(typeof(MendRestSiteOption), nameof(MendRestSiteOption.OnSelect)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(BeforeMendChoice)),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AfterUnsupportedChoice)));
    }

    static AbstractModel? ChoiceSource(PlayerChoiceContext? context, AbstractModel? source = null) =>
        source ?? context?.LastInvolvedModel ?? (context as HookPlayerChoiceContext)?.Source ?? selectionSource.Value;

    static void BeforeObtainingChoiceRelic(RelicModel relic, out AbstractModel? __state)
    {
        __state = selectionSource.Value;
        selectionSource.Value = relic;
    }
    static void BeforeModelChoice(AbstractModel __instance, out AbstractModel? __state)
    {
        __state = selectionSource.Value;
        selectionSource.Value = __instance;
    }
    // The original async method captures this scope across awaits. Restore the
    // caller immediately, including synchronous faults, without replacing its task.
    static void RestoreChoiceSource(AbstractModel? __state) => selectionSource.Value = __state;
    static void AfterChoiceEventOption(EventOption __instance, EventModel eventModel)
    {
        optionEvents.Remove(__instance);
        optionEvents.Add(__instance, eventModel);
    }
    static void AfterChoiceEventOptionCopy(EventOption __instance, EventOption eventOption)
    {
        if (optionEvents.TryGetValue(eventOption, out var model)) {
            optionEvents.Remove(__instance);
            optionEvents.Add(__instance, model);
        }
    }
    static class EventChoiceSourceHooks
    {
        internal static void Before(EventOption __instance, out AbstractModel? __state)
        {
            __state = selectionSource.Value;
            selectionSource.Value = optionEvents.TryGetValue(__instance, out var model) ? model : null;
        }
        internal static void Restore(AbstractModel? __state) => RestoreChoiceSource(__state);
    }

    static UnsupportedChoice RememberUnsupportedChoice(Player player, string reason)
    {
        var choice = new UnsupportedChoice(reason);
        unsupportedChoices.Remove(player);
        unsupportedChoices.Add(player, choice);
        AdvisorMain(() => {
            MarkAdvisorDirty();
            if (advisorActive && run is not null) AiAdvisor.RefuseDecision(run.GetPlayerSlotIndex(player), reason, AdviceReason.UnsupportedScreen);
        });
        return choice;
    }
    static void BeforeStandaloneRelicChoice(Player player, out UnsupportedChoice __state) =>
        __state = RememberUnsupportedChoice(player,
            "추천 불가: 독립 유물 선택 화면은 원본 입력 계약에서 지원되지 않습니다. 상자 유물 선택과는 다른 화면입니다.");
    static void BeforeMendChoice(MendRestSiteOption __instance, out UnsupportedChoice __state) =>
        __state = RememberUnsupportedChoice((Player)AccessTools.Property(typeof(RestSiteOption), "Owner").GetValue(__instance)!,
            "추천 불가: 치료 대상 플레이어 선택은 배지의 대상 선택 계약에서 지원되지 않습니다.");
    static void AfterUnsupportedChoice(Task __result, UnsupportedChoice __state) => __state.Task = __result;
    internal static void CheckUnsupportedChoice(Player player)
    {
        if (!unsupportedChoices.TryGetValue(player, out var choice)) return;
        if (choice.Task?.IsCompleted == true) { unsupportedChoices.Remove(player); return; }
        throw new AdviceUnavailableException(AdviceReason.UnsupportedScreen, choice.Reason);
    }
}
