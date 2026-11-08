using System.Reflection;
using System.Runtime.CompilerServices;
using HarmonyLib;
using MegaCrit.Sts2.Core.Combat;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Nodes.Cards.Holders;

namespace HumanPlayRecorder.Ai;

public static class GameUiPlayability
{
    sealed record Entry(ICombatState State, Task? Refresh, bool Playable);
    sealed class Holder { public volatile Entry? Value; }
    static readonly ConditionalWeakTable<CardModel, Holder> values = new();
    static readonly FieldInfo deferred = AccessTools.Field(typeof(CombatStateTracker), "_combatStateChangedDeferredTask");
    [ThreadStatic] static CardModel? updating;

    internal static void Install(Harmony harmony)
    {
        harmony.Patch(AccessTools.Method(typeof(NHandCardHolder), nameof(NHandCardHolder.UpdateCard)),
            prefix: new HarmonyMethod(typeof(GameUiPlayability), nameof(BeforeUiUpdate)),
            finalizer: new HarmonyMethod(typeof(GameUiPlayability), nameof(AfterUiUpdate)));
        var predicate = typeof(CardModel).GetMethods().Single(method =>
            method.Name == nameof(CardModel.CanPlay) && method.GetParameters().Length == 2);
        harmony.Patch(predicate, postfix: new HarmonyMethod(typeof(GameUiPlayability), nameof(GameResult)));
    }

    static void BeforeUiUpdate(NHandCardHolder __instance, out CardModel? __state)
    {
        __state = updating;
        try {
            updating = __instance.CardNode?.Model;
            if (updating is not null) values.Remove(updating);
        } catch (Exception) { updating = null; }
    }
    static void AfterUiUpdate(Exception? __exception, CardModel? __state)
    {
        if (__exception is not null && updating is not null) values.Remove(updating);
        updating = __state;
    }
    static void GameResult(CardModel __instance, bool __result)
    {
        if (ReferenceEquals(updating, __instance)) ObserveComputed(__instance, __result);
    }

    public static void ObserveComputed(CardModel card, bool playable)
    {
        try {
            if (card.CombatState is not { } state) return;
            var refresh = (Task?)deferred.GetValue(CombatManager.Instance.StateTracker);
            var holder = values.GetValue(card, _ => new Holder());
            var previous = holder.Value;
            holder.Value = new(state, refresh, playable);
            if (previous is null || previous.Playable != playable || !ReferenceEquals(previous.Refresh, refresh))
                RecorderMod.MarkAdvisorDirty();
        } catch (Exception) { values.Remove(card); }
    }

    /// <summary>The hand UI's own result when it is current; otherwise the game's predicate,
    /// evaluated now. Waiting for the UI blocked advice for as long as the hand was not
    /// redrawn after another player's action, which in multiplayer could be a whole turn.</summary>
    internal static bool Read(CardModel card)
    {
        FrameCaptureBudget.Check();
        var refresh = (Task?)deferred.GetValue(CombatManager.Instance.StateTracker);
        if (refresh is not { IsCompletedSuccessfully: false } &&
            values.TryGetValue(card, out var holder) && holder.Value is { } entry &&
            ReferenceEquals(entry.State, card.CombatState) && ReferenceEquals(entry.Refresh, refresh))
            return entry.Playable;
        // A mod's ShouldPlay hook may throw; that refuses this frame's advice, never the game.
        return LiveModelData.Item(card, nameof(CardModel.CanPlay), () => card.CanPlay());
    }
}
