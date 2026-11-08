using System.Reflection;
using HarmonyLib;
using HumanPlayRecorder.Settings;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Multiplayer.Game.Lobby;
using MegaCrit.Sts2.Core.Runs;
using MegaCrit.Sts2.Core.Saves;
using MegaCrit.Sts2.Core.Unlocks;

namespace HumanPlayRecorder.VanillaGuard;

// Content and Harmony guards share one lifetime. The existing content/replay
// gates remain authoritative for effects outside these interception points.
public static class ContentPools
{
    static readonly HashSet<string> BaseCharacters = ["IRONCLAD", "SILENT", "DEFECT", "REGENT", "NECROBINDER"];
    static readonly HashSet<string> PreservedModelDbCaches = ["_allCharacterCardPools", "_allCardPools"];
    static readonly List<(FieldInfo Field, object? Owner, object? Value)> caches = [];
    static HashSet<string>? originalIds;
    static bool installed;
    static bool terminalPending;
    static string? failure;
    static string noticeKey = "original_rules_standard_only";
    [ThreadStatic] static bool inBeginRunLocally;
    public static bool Active { get; private set; }
    public static bool IsNeutralized => Active && failure is null;
    public static string? NonNeutralizationReason => failure;
    public static string? LastNoticeKey { get; private set; }
    public static int NoticeCount { get; private set; }
    public static void ResetNoticeState() { LastNoticeKey = null; NoticeCount = 0; }
    public static bool IsOriginalCharacter(CharacterModel? character) =>
        character is not null && BaseCharacters.Contains(character.Id.Entry);
    public static bool IsOriginalId(string id) =>
        (originalIds ??= VanillaContent.Load().models.Keys.ToHashSet(StringComparer.Ordinal)).Contains(id);

    public static void Install(Harmony harmony)
    {
        if (installed) return;
        PatchSuspension.Install(harmony);
        RegisteredModelCensus.Install(harmony);
        WireTransportBoundary.Install(harmony);
        WireInterop.Install(harmony);
        MenuLifecycle.Install(harmony);
        harmony.Patch(AccessTools.Method(typeof(MegaCrit.Sts2.Core.Nodes.NGame), "StartNewSingleplayerRun"),
            prefix: new HarmonyMethod(typeof(ContentPools), nameof(PrepareSolo)) { priority = Priority.First },
            postfix: new HarmonyMethod(typeof(ContentPools), nameof(WatchStart)),
            finalizer: new HarmonyMethod(typeof(ContentPools), nameof(StartFailed)));
        harmony.Patch(AccessTools.Method(typeof(MegaCrit.Sts2.Core.Nodes.NGame), "StartNewMultiplayerRun"),
            prefix: new HarmonyMethod(typeof(ContentPools), nameof(PrepareMultiplayer)) { priority = Priority.First },
            postfix: new HarmonyMethod(typeof(ContentPools), nameof(WatchStart)),
            finalizer: new HarmonyMethod(typeof(ContentPools), nameof(StartFailed)));
        harmony.Patch(AccessTools.Method(typeof(MegaCrit.Sts2.Core.Nodes.NGame), "LoadRun"),
            prefix: new HarmonyMethod(typeof(ContentPools), nameof(EnterLoadRun)) { priority = Priority.First },
            postfix: new HarmonyMethod(typeof(ContentPools), nameof(WatchTask)),
            finalizer: new HarmonyMethod(typeof(ContentPools), nameof(StartFailed)));
        // A finalizer runs after every postfix, including mod-loader postfixes
        // that append their characters after ours.
        harmony.Patch(AccessTools.PropertyGetter(typeof(ModelDb), nameof(ModelDb.AllCharacters)),
            finalizer: new HarmonyMethod(typeof(ContentPools), nameof(FilterLobbyRandomCharacters)) { priority = Priority.Last });
        harmony.Patch(AccessTools.Method(typeof(StartRunLobby), "BeginRunLocally"),
            prefix: new HarmonyMethod(typeof(ContentPools), nameof(BeforeBeginRunLocally)) { priority = Priority.First },
            postfix: new HarmonyMethod(typeof(ContentPools), nameof(AfterBeginRunLocally)) { priority = Priority.Last },
            finalizer: new HarmonyMethod(typeof(ContentPools), nameof(ExitBeginRunLocally)));
        var types = typeof(ModelDb).Assembly.GetTypes().Where(t =>
            t == typeof(ModelDb) || t == typeof(UnlockState) ||
            typeof(CardPoolModel).IsAssignableFrom(t) || typeof(RelicPoolModel).IsAssignableFrom(t) ||
            typeof(PotionPoolModel).IsAssignableFrom(t) || typeof(ActModel).IsAssignableFrom(t));
        var names = new HashSet<string>(StringComparer.Ordinal) {
            "get_AllCards", "get_AllRelics", "get_AllPotions",
            "GetUnlockedCards", "GetUnlockedRelics", "GetUnlockedPotions",
            "get_Characters", "get_CharacterCardPools", "get_CardPools", "get_Cards", "get_Relics", "get_Potions",
            "get_SharedAncients", "get_AllSharedCardPools",
            "get_AllPotionPools", "get_AllCharacterPotionPools", "get_AllSharedPotionPools",
            "get_AllRelicPools", "get_AllCharacterRelicPools", "get_CharacterRelicPools", "get_AllSharedRelicPools",
            "get_AllSharedEvents", "get_AllSharedAncients",
            "get_AllEncounters", "get_AllWeakEncounters", "get_AllRegularEncounters",
            "get_AllEliteEncounters", "get_AllBossEncounters", "get_BossDiscoveryOrder", "GenerateAllEncounters",
            "get_AllEvents", "get_AllAncients", "GetUnlockedAncients",
            "get_EventEncounters", "get_AllPossibleMonsters", "get_AllMonsters", "get_Monsters",
            "get_Acts", "GetRandomList"
        };
        foreach (var method in types.SelectMany(t => t.GetMethods(
            BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.Static | BindingFlags.DeclaredOnly))
            .Where(m => names.Contains(m.Name) && !m.IsAbstract && !m.ContainsGenericParameters))
        {
            var result = method.ReturnType;
            if (!result.IsGenericType || result.GetGenericTypeDefinition() != typeof(IEnumerable<>)) continue;
            var element = result.GetGenericArguments()[0];
            if (!typeof(AbstractModel).IsAssignableFrom(element)) continue;
            var filter = typeof(ContentPools).GetMethods(BindingFlags.NonPublic | BindingFlags.Static)
                .Single(m => m.Name == "Filter" + element.Name);
            // A finalizer, not a postfix: content libraries append at the same
            // Priority.Last after us (RitsuLib shared events), and act rooms are
            // generated before run-phase suspension removes their postfixes.
            harmony.Patch(method, finalizer: new HarmonyMethod(filter)
                { priority = Priority.Last });
        }
        harmony.Patch(AccessTools.Method(typeof(RunManager), nameof(RunManager.SetUpNewSingleplayer)),
            prefix: new HarmonyMethod(typeof(ContentPools), nameof(Start)) { priority = Priority.First },
            postfix: new HarmonyMethod(typeof(ContentPools), nameof(CheckFailure)) { priority = Priority.Last },
            finalizer: new HarmonyMethod(typeof(ContentPools), nameof(StartFailed)));
        harmony.Patch(AccessTools.Method(typeof(RunManager), nameof(RunManager.SetUpNewMultiplayer)),
            prefix: new HarmonyMethod(typeof(ContentPools), nameof(Start)) { priority = Priority.First },
            postfix: new HarmonyMethod(typeof(ContentPools), nameof(CheckFailure)) { priority = Priority.Last },
            finalizer: new HarmonyMethod(typeof(ContentPools), nameof(StartFailed)));
        harmony.Patch(AccessTools.Method(typeof(RunManager), nameof(RunManager.SetUpSavedSingleplayer)),
            prefix: new HarmonyMethod(typeof(ContentPools), nameof(StartSaved)) { priority = Priority.First },
            postfix: new HarmonyMethod(typeof(ContentPools), nameof(WatchTask)),
            finalizer: new HarmonyMethod(typeof(ContentPools), nameof(StartFailed)));
        harmony.Patch(AccessTools.Method(typeof(RunManager), nameof(RunManager.SetUpSavedMultiplayer)),
            prefix: new HarmonyMethod(typeof(ContentPools), nameof(StartSaved)) { priority = Priority.First },
            postfix: new HarmonyMethod(typeof(ContentPools), nameof(WatchTask)),
            finalizer: new HarmonyMethod(typeof(ContentPools), nameof(StartFailed)));
        harmony.Patch(AccessTools.Method(typeof(RunManager), nameof(RunManager.CleanUp)),
            finalizer: new HarmonyMethod(typeof(ContentPools), nameof(Stop)));
        harmony.Patch(AccessTools.Method(typeof(RunManager), "OnEnded"),
            finalizer: new HarmonyMethod(typeof(ContentPools), nameof(RunEnded)));
        harmony.Patch(AccessTools.Method(typeof(RecorderMod), "FinishJournalIfReady"),
            postfix: new HarmonyMethod(typeof(ContentPools), nameof(AfterJournalFinished)));
        harmony.Patch(AccessTools.Method(typeof(MegaCrit.Sts2.Core.Nodes.NGame), "Quit"),
            prefix: new HarmonyMethod(typeof(ContentPools), nameof(Stop)));
        installed = true;
    }

    static void BeforeBeginRunLocally(StartRunLobby __instance, List<ModifierModel> modifiers)
    {
        MenuLifecycle.BeginLobbyLaunch();
        if (!OriginalRulesSettings.Enabled && !Active) return;
        if (__instance.GameMode != GameMode.Standard || __instance.DailyTime is not null || modifiers.Count != 0)
        {
            UnsupportedMode();
            return;
        }
        inBeginRunLocally = Active;
    }
    static void AfterBeginRunLocally(StartRunLobby __instance)
    {
        if (!OriginalRulesSettings.Enabled && !Active) return;
        if (__instance.Players.Count == 0 || __instance.Players.Any(p => !IsOriginalCharacter(p.character)))
            UnsupportedCharacter();
    }
    static void ExitBeginRunLocally() => inBeginRunLocally = false;
    // In a guarded run, gameplay that counts characters must see the vanilla
    // roster: Kaleidoscope.IsAllowedAtNeow compares it with the unlocked pools,
    // and a mismatch reshuffles every player's Neow options against vanilla peers.
    static void FilterLobbyRandomCharacters(ref IEnumerable<CharacterModel> __result)
    {
        if ((inBeginRunLocally || PatchSuspension.InRun) && Active)
            __result = __result.Where(IsOriginalCharacter).ToArray();
    }

    static void PrepareSolo(CharacterModel character, IReadOnlyList<ModifierModel> modifiers, GameMode gameMode, MethodBase __originalMethod)
    {
        MenuLifecycle.EnteredGameStart();
        try
        {
            if (!OriginalRulesSettings.Enabled && !Active) return;
            if (gameMode != GameMode.Standard || modifiers.Count != 0) { UnsupportedMode(); return; }
            if (!BaseCharacters.Contains(character.Id.Entry)) { UnsupportedCharacter(); return; }
            ValidateStart(__originalMethod);
        }
        catch (Exception e) { Fail(e); Stop(); }
    }
    static void PrepareMultiplayer(StartRunLobby lobby, IReadOnlyList<ModifierModel> modifiers, DateTimeOffset? dailyTime, MethodBase __originalMethod)
    {
        MenuLifecycle.EnteredGameStart();
        try
        {
            if (!OriginalRulesSettings.Enabled && !Active) return;
            if (lobby.GameMode != GameMode.Standard || dailyTime is not null ||
                modifiers.Count != 0 || lobby.Modifiers.Count != 0) { UnsupportedMode(); return; }
            if (lobby.Players.Count == 0 || lobby.Players.Any(p => !IsOriginalCharacter(p.character)))
            {
                UnsupportedCharacter();
                return;
            }
            ValidateStart(__originalMethod);
        }
        catch (Exception e) { Fail(e); Stop(); }
    }
    static void EnterLoadRun() => MenuLifecycle.EnteredGameStart();

    static void Refuse(string reason, string key)
    {
        End();
        failure = reason;
        noticeKey = key;
        LastNoticeKey = key;
        NoticeCount++;
        CheckFailure();
        ShowModeNotice();
    }
    internal static void UnsupportedMode() =>
        Refuse("original rules unavailable: only standard singleplayer runs are supported", "original_rules_standard_only");
    internal static void UnsupportedCharacter() =>
        Refuse("original rules unavailable: unsupported character", "original_rules_unsupported_character");
    internal static void UnsupportedSave() =>
        Refuse("original rules unavailable: saved run contains modded content", "original_rules_unavailable");
    static void ShowModeNotice() =>
            UiGuard.Run("guard unsupported mode", () => {
                if (MegaCrit.Sts2.Core.Nodes.NGame.Instance is null ||
                    MegaCrit.Sts2.Core.TestSupport.TestMode.IsOn) return;
                Godot.Callable.From(() =>
                    FirstLaunchNotice.ShowShortNote(UiText.Get("settings_original_rules"),
                        OriginalRulesSettings.NoticeText(noticeKey))).CallDeferred();
            });
    internal static void BeginMenuScope(bool enabled)
    {
        if (enabled && Active && failure is null) return;
        failure = null;
        try { End(); if (enabled) BeginScope(); }
        catch (Exception e) { Fail(e); Stop(); }
    }
    static void ValidateStart(MethodBase original)
    {
        if (PatchSuspension.HasForeignPatches(original) || !Active ||
            MenuLifecycle.RunEntrypoints.Any(PatchSuspension.HasForeignPatches))
        {
            Refuse(failure ?? "original rules unavailable: guard was not established before launch",
                "original_rules_unavailable");
        }
    }
    static void WatchStart(ref Task<RunState> __result) => __result = FinishStart(__result);
    static void WatchTask(ref Task __result) => __result = FinishTask(__result);
    static async Task<RunState> FinishStart(Task<RunState> task)
    {
        try { return await task; }
        catch { Stop(); throw; }
    }
    static async Task FinishTask(Task task)
    {
        try { await task; }
        catch { Stop(); throw; }
    }
    static void Start(RunState state, MethodBase __originalMethod)
    {
        try
        {
            if (!OriginalRulesSettings.Enabled && !Active) return;
            if (state.GameMode != GameMode.Standard || state.Modifiers.Count != 0) { UnsupportedMode(); return; }
            if (state.Players.Count == 0 || state.Players.Any(p => !IsOriginalCharacter(p.Character)))
            {
                UnsupportedCharacter();
                return;
            }
            ValidateStart(__originalMethod);
            PatchSuspension.EnterRun();
        }
        catch (Exception e) { Fail(e); Stop(); }
    }
    static void StartSaved(RunState state, MethodBase __originalMethod)
    {
        MenuLifecycle.EnteredGameStart();
        try
        {
            if (!OriginalRulesSettings.Enabled && !Active) return;
            if (state.GameMode != GameMode.Standard || state.Modifiers.Count != 0) { UnsupportedMode(); return; }
            if (state.Players.Count == 0 || state.Players.Any(p => !IsOriginalCharacter(p.Character)))
            {
                UnsupportedCharacter();
                return;
            }
            if (HasNonOriginalContent(state)) { UnsupportedSave(); return; }
            ValidateStart(__originalMethod);
            PatchSuspension.EnterRun();
        }
        catch (Exception e) { Fail(e); Stop(); }
    }
    internal static bool ValidateSave(SerializableRun save)
    {
        if (save.GameMode != GameMode.Standard || save.DailyTime is not null || (save.Modifiers?.Count ?? 0) != 0)
        {
            UnsupportedMode();
            return false;
        }
        if (save.Players is null || save.Players.Count == 0 ||
            save.Players.Any(p => p.CharacterId is null || !BaseCharacters.Contains(p.CharacterId.Entry)))
        {
            UnsupportedCharacter();
            return false;
        }
        if ((save.Acts is not null && save.Acts.Any(a => a.Id is null || !IsOriginalId(a.Id.ToString()) ||
                (a.SerializableRooms?.EventIds is { } evs && evs.Any(id => !IsOriginalId(id.ToString()))) ||
                (a.SerializableRooms?.NormalEncounterIds is { } nes && nes.Any(id => !IsOriginalId(id.ToString()))) ||
                (a.SerializableRooms?.EliteEncounterIds is { } ees && ees.Any(id => !IsOriginalId(id.ToString()))))) ||
            (save.EventsSeen is not null && save.EventsSeen.Any(id => !IsOriginalId(id.ToString()))) ||
            save.Players.Any(p =>
                (p.Deck is not null && p.Deck.Any(c => c.Id is null || !IsOriginalId(c.Id.ToString()))) ||
                (p.Relics is not null && p.Relics.Any(r => r.Id is null || !IsOriginalId(r.Id.ToString()))) ||
                (p.Potions is not null && p.Potions.Any(po => po.Id is null || !IsOriginalId(po.Id.ToString())))))
        {
            UnsupportedSave();
            return false;
        }
        return true;
    }
    internal static bool HasNonOriginalContent(RunState state)
    {
        if (state.Acts.Any(a => !IsOriginalId(a.Id.ToString())) ||
            state.VisitedEventIds.Any(id => !IsOriginalId(id.ToString())))
            return true;
        foreach (var player in state.Players)
        {
            if (!IsOriginalCharacter(player.Character) ||
                player.Deck.Cards.Any(c => !IsOriginalId(c.Id.ToString())) ||
                player.Relics.Any(r => !IsOriginalId(r.Id.ToString())) ||
                player.PotionSlots.Where(p => p is not null).Any(p => !IsOriginalId(p!.Id.ToString())))
                return true;
        }
        return false;
    }
    static void CheckFailure() { if (failure is not null) PerRunContentGate.Active?.Reject(failure); }
    internal static void SuspensionFailed(Exception e)
    {
        failure = "original rules unavailable: patch suspension " + e.GetType().Name;
        noticeKey = "original_rules_unavailable";
        LastNoticeKey = noticeKey;
        NoticeCount++;
        try { CheckFailure(); ShowModeNotice(); }
        catch (Exception logError) { UiGuard.Report("guard rejection", logError); }
    }
    static void Fail(Exception e)
    {
        failure = "original content filtering unavailable " + e.GetType().Name;
        noticeKey = "original_rules_unavailable";
        LastNoticeKey = noticeKey;
        NoticeCount++;
        CheckFailure();
        ShowModeNotice();
        UiGuard.Report("original content filtering", e);
    }
    static void StartFailed(Exception? __exception) { if (__exception is not null) Stop(); }
    static void Stop() { try { End(); } catch (Exception e) { Fail(e); } }
    static void RunEnded(Exception? __exception)
    {
        if (__exception is not null) { Stop(); return; }
        // OnEnded may be inside the last awaited action. Do not restore foreign
        // model getters before that action's final content/hash validation.
        terminalPending = true;
        AfterJournalFinished();
    }
    static void AfterJournalFinished()
    {
        if (terminalPending && PerRunContentGate.Active is null or { Trainable: false }) Stop();
    }

    public static void BeginSolo(RunState state, bool enabled)
    {
        failure = null;
        End();
        if (!enabled || state.Players.Count != 1 ||
            !BaseCharacters.Contains(state.Players[0].Character.Id.Entry)) return;
        BeginScope();
    }

    public static void BeginMultiplayer(RunState state, bool enabled)
    {
        failure = null;
        End();
        if (!enabled) return;
        if (state.GameMode != GameMode.Standard || state.Modifiers.Count != 0) { UnsupportedMode(); return; }
        if (state.Players.Count == 0 || state.Players.Any(p => !IsOriginalCharacter(p.Character)))
        {
            UnsupportedCharacter();
            return;
        }
        if (HasNonOriginalContent(state)) { UnsupportedSave(); return; }
        BeginScope();
    }

    static void BeginScope()
    {
        if (!PatchSuspension.Begin()) return;
        try
        {
            originalIds ??= VanillaContent.Load().models.Keys.ToHashSet(StringComparer.Ordinal);
            // Registry getters memoize derived lists. Restore their pre-run caches so
            // a filtered list created during this run cannot constrain later modded runs.
            void Capture(Type type, object? owner, IEnumerable<string>? names = null, ISet<string>? clearNames = null)
            {
                foreach (var field in type.GetFields(BindingFlags.Public | BindingFlags.NonPublic |
                    BindingFlags.DeclaredOnly | (owner is null ? BindingFlags.Static : BindingFlags.Instance)))
                    if (!field.IsInitOnly && (names?.Contains(field.Name) ??
                        typeof(IEnumerable<AbstractModel>).IsAssignableFrom(field.FieldType)))
                    {
                        caches.Add((field, owner, field.GetValue(owner)));
                        if (clearNames?.Contains(field.Name) == true)
                            field.SetValue(owner, null);
                    }
            }
            var canonical = ModelDb.All.ToArray();
            var clearModelDb = typeof(ModelDb).GetFields(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.DeclaredOnly)
                .Where(f => !f.IsInitOnly && typeof(IEnumerable<AbstractModel>).IsAssignableFrom(f.FieldType) && !PreservedModelDbCaches.Contains(f.Name))
                .Select(f => f.Name).Concat(["_acts", "_actsByIndex"]).ToHashSet(StringComparer.Ordinal);
            Capture(typeof(ModelDb), null, clearModelDb.Concat(PreservedModelDbCaches), clearModelDb);
            foreach (var model in canonical)
            {
                if (model is CardPoolModel) Capture(typeof(CardPoolModel), model, ["_allCards", "_allCardIds"], new HashSet<string> { "_allCardIds" });
                if (model is RelicPoolModel) Capture(typeof(RelicPoolModel), model, ["_relics", "_allRelicIds"], new HashSet<string> { "_allRelicIds" });
                if (model is PotionPoolModel) Capture(typeof(PotionPoolModel), model, ["_allPotions", "_allPotionIds"], new HashSet<string> { "_allPotionIds" });
                if (model is ActModel) Capture(typeof(ActModel), model,
                    ["_allEncounters", "_allWeakEncounters", "_allRegularEncounters",
                     "_allEliteEncounters", "_allBossEncounters", "_allMonsters"],
                    new HashSet<string> { "_allWeakEncounters", "_allRegularEncounters", "_allEliteEncounters", "_allBossEncounters", "_allMonsters" });
            }
            WireInterop.Begin();
            Active = true;
        }
        catch
        {
            End();
            throw;
        }
    }

    public static void End()
    {
        Active = false;
        terminalPending = false;
        try
        {
            WireInterop.End();
            foreach (var (field, owner, value) in caches) field.SetValue(owner, value);
            caches.Clear();
        }
        finally { PatchSuspension.End(); }
    }

    static void Filter<T>(ref IEnumerable<T> __result) where T : AbstractModel
    {
        // Null only when the original getter threw; its exception propagates.
        if (!Active || __result is null) return;
        try {
            __result = __result.Where(m =>
                NativeModCompatibility.Preserves(m.GetType().Assembly) ||
                (m.GetType().Assembly == typeof(ModelDb).Assembly &&
                 originalIds!.Contains(m.Id.ToString()))).ToArray();
        }
        catch (Exception e) { Fail(e); }
    }
    // Harmony persists method tokens, not constructed generic arguments, when
    // another mod rebuilds a wrapper. Its entry points must be non-generic.
    static void FilterCardModel(ref IEnumerable<CardModel> __result) => Filter(ref __result);
    static void FilterRelicModel(ref IEnumerable<RelicModel> __result) => Filter(ref __result);
    static void FilterPotionModel(ref IEnumerable<PotionModel> __result) => Filter(ref __result);
    static void FilterCharacterModel(ref IEnumerable<CharacterModel> __result) => Filter(ref __result);
    static void FilterCardPoolModel(ref IEnumerable<CardPoolModel> __result) => Filter(ref __result);
    static void FilterPotionPoolModel(ref IEnumerable<PotionPoolModel> __result) => Filter(ref __result);
    static void FilterRelicPoolModel(ref IEnumerable<RelicPoolModel> __result) => Filter(ref __result);
    static void FilterEventModel(ref IEnumerable<EventModel> __result) => Filter(ref __result);
    static void FilterAncientEventModel(ref IEnumerable<AncientEventModel> __result) => Filter(ref __result);
    static void FilterEncounterModel(ref IEnumerable<EncounterModel> __result) => Filter(ref __result);
    static void FilterMonsterModel(ref IEnumerable<MonsterModel> __result) => Filter(ref __result);
    static void FilterActModel(ref IEnumerable<ActModel> __result) => Filter(ref __result);
}
