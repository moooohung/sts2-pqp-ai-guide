using System.Reflection;
using System.Runtime.CompilerServices;
using HarmonyLib;
using HumanPlayRecorder.Settings;
using MegaCrit.Sts2.Core.Multiplayer.Game;
using MegaCrit.Sts2.Core.Multiplayer.Game.Lobby;
using MegaCrit.Sts2.Core.Nodes;
using MegaCrit.Sts2.Core.Nodes.Screens.Blitz;
using MegaCrit.Sts2.Core.Nodes.Screens.CharacterSelect;
using MegaCrit.Sts2.Core.Nodes.Screens.CustomRun;
using MegaCrit.Sts2.Core.Nodes.Screens.DailyRun;
using MegaCrit.Sts2.Core.Nodes.Screens.Dread;
using MegaCrit.Sts2.Core.Nodes.Screens.MainMenu;
using MegaCrit.Sts2.Core.Runs;
using MegaCrit.Sts2.Core.Saves;

namespace HumanPlayRecorder.VanillaGuard;

public static class MenuLifecycle
{
    static bool selectionOpen, launching, enteredGameStart;
    static int launchGeneration;
    // These Node methods orchestrate gameplay; their generated bodies are not
    // presentation. The menu callbacks that establish the scope are deliberately
    // absent, so activation never rebuilds a wrapper already on the stack.
    internal static readonly HashSet<MethodBase> RunEntrypoints = Entrypoints();
    static HashSet<MethodBase> Entrypoints()
    {
        var result = new HashSet<MethodBase>();
        foreach (var (type, name) in new[] {
            (typeof(NGame), "StartNewSingleplayerRun"), (typeof(NGame), "StartNewMultiplayerRun"),
            (typeof(NGame), "StartRun"), (typeof(NGame), "LoadRun"),
            (typeof(NCharacterSelectScreen), "StartNewSingleplayerRun"),
            (typeof(NCharacterSelectScreen), "StartNewMultiplayerRun"),
            (typeof(NMultiplayerLoadGameScreen), "StartRun"),
            (typeof(NMainMenu), "OnContinueButtonPressedAsync") })
        {
            var method = AccessTools.Method(type, name);
            result.Add(method);
            var machine = method.GetCustomAttribute<AsyncStateMachineAttribute>()?.StateMachineType;
            if (machine is not null) result.Add(AccessTools.Method(machine, "MoveNext"));
        }
        return result;
    }
    internal static void Install(Harmony h)
    {
        h.Patch(AccessTools.Method(typeof(NCharacterSelectScreen), "OnSubmenuOpened"),
            prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(Opened)));
        h.Patch(AccessTools.Method(typeof(NCharacterSelectScreen), "OnSubmenuClosed"),
            finalizer: new HarmonyMethod(typeof(MenuLifecycle), nameof(Closed)));
        foreach (var name in new[] { "StartNewSingleplayerRun", "StartNewMultiplayerRun" })
            h.Patch(AccessTools.Method(typeof(NCharacterSelectScreen), name),
                prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(Launching)),
                postfix: new HarmonyMethod(typeof(MenuLifecycle), nameof(WatchLauncher)),
                finalizer: new HarmonyMethod(typeof(MenuLifecycle), nameof(LauncherThrew)));
        h.Patch(AccessTools.Method(typeof(NMultiplayerLoadGameScreen), "OnSubmenuOpened"),
            prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(OpenedLoad)));
        h.Patch(AccessTools.Method(typeof(NMultiplayerLoadGameScreen), "OnSubmenuClosed"),
            finalizer: new HarmonyMethod(typeof(MenuLifecycle), nameof(Closed)));
        h.Patch(AccessTools.Method(typeof(NMultiplayerLoadGameScreen), "StartRun"),
            prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(Launching)),
            postfix: new HarmonyMethod(typeof(MenuLifecycle), nameof(WatchLauncher)),
            finalizer: new HarmonyMethod(typeof(MenuLifecycle), nameof(LauncherThrew)));
        h.Patch(AccessTools.Method(typeof(NMultiplayerHostSubmenu), nameof(NMultiplayerHostSubmenu.StartHostAsync)),
            prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(BeforeHostSubmenu)));
        h.Patch(AccessTools.Method(typeof(NMultiplayerSubmenu), nameof(NMultiplayerSubmenu.StartHost)),
            prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(BeforeHostLoad)));
        h.Patch(AccessTools.Method(typeof(NJoinFriendScreen), nameof(NJoinFriendScreen.JoinGameAsync)),
            prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(BeforeJoinFriend)));
        foreach (var type in NonStandardScreens)
            h.Patch(AccessTools.Method(type, "OnSubmenuOpened"),
                prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(OpenedNonStandard)));
        foreach (var ctor in AccessTools.GetDeclaredConstructors(typeof(StartRunLobby)))
            h.Patch(ctor, prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(BeforeStartLobbyCtor)));
        h.Patch(AccessTools.Method(typeof(StartRunLobby), nameof(StartRunLobby.CleanUp)),
            prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(BeforeLobbyCleanUp)));
        h.Patch(AccessTools.Constructor(typeof(LoadRunLobby),
            [typeof(INetGameService), typeof(ILoadRunLobbyListener), typeof(SerializableRun)]),
            prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(BeforeLoadLobbyCtor)));
        h.Patch(AccessTools.Method(typeof(LoadRunLobby), nameof(LoadRunLobby.CleanUp)),
            prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(BeforeLobbyCleanUp)));
        h.Patch(AccessTools.Method(typeof(LoadRunLobby), "BeginRunLocally"),
            prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(BeginLobbyLaunch)));
        h.Patch(AccessTools.Method(typeof(NMainMenu), "OnContinueButtonPressed"),
            prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(Continue)));
        h.Patch(AccessTools.Method(typeof(NMainMenu), "_Ready"),
            prefix: new HarmonyMethod(typeof(MenuLifecycle), nameof(ReturnedToMenu)));
        OriginalRulesSettings.Changed += ToggleChanged;
    }
    // Singleplayer submenus whose runs are never Standard (NSingleplayerSubmenu opens each directly).
    internal static readonly Type[] NonStandardScreens = [ typeof(NDailyRunScreen), typeof(NCustomRunScreen),
        typeof(NDailyRunLoadScreen), typeof(NCustomRunLoadScreen), typeof(NBlitzScreen), typeof(NDreadScreen) ];
    static void Opened(NCharacterSelectScreen __instance) => UiGuard.Run("guard menu open",
        () => OpenSelection(__instance.Lobby is null ||
            __instance.Lobby.GameMode == GameMode.Standard && !__instance.Lobby.IsCustom));
    static void OpenedLoad(NMultiplayerLoadGameScreen __instance) => UiGuard.Run("guard load menu open", () => {
        var lobby = (LoadRunLobby?)AccessTools.Field(typeof(NMultiplayerLoadGameScreen), "_runLobby").GetValue(__instance);
        if (lobby?.Run is { } save) PrepareLoad(save);
    });
    static void OpenedNonStandard() => UiGuard.Run("guard nonstandard menu open", () => {
        selectionOpen = false;
        if (OriginalRulesSettings.Enabled) ContentPools.UnsupportedMode();
    });
    static void BeforeHostSubmenu(GameMode gameMode, bool isCustom) => UiGuard.Run("guard host submenu", () => {
        if (gameMode == GameMode.Standard && !isCustom) OpenSelection(true);
        else if (OriginalRulesSettings.Enabled) ContentPools.UnsupportedMode();
    });
    static void BeforeHostLoad(SerializableRun run) => UiGuard.Run("guard host load", () => PrepareLoad(run));
    static void BeforeJoinFriend() => UiGuard.Run("guard join friend", () => {
        if (OriginalRulesSettings.Enabled) OpenSelection(true);
    });
    static void BeforeStartLobbyCtor(GameMode gameMode, bool isCustom, INetGameService netService) => UiGuard.Run("guard start lobby", () => {
        if (!netService.Type.IsMultiplayer()) return;
        if (gameMode == GameMode.Standard && !isCustom) OpenSelection(true);
        else if (OriginalRulesSettings.Enabled) ContentPools.UnsupportedMode();
    });
    static void BeforeLoadLobbyCtor(SerializableRun runSave) => UiGuard.Run("guard load lobby", () => PrepareLoad(runSave));
    static void BeforeLobbyCleanUp(bool disconnectSession) => UiGuard.Run("guard lobby cleanup", () => {
        if (disconnectSession) CloseSelection();
    });
    static void Closed() => UiGuard.Run("guard menu close", CloseSelection);
    internal static void BeginLobbyLaunch() => launching = true;
    static void Launching(out int __state)
    {
        launching = true;
        enteredGameStart = false;
        __state = ++launchGeneration;
    }
    internal static void EnteredGameStart() => enteredGameStart = true;
    static void WatchLauncher(ref Task __result, int __state) => __result = FinishLauncher(__result, __state);
    static async Task FinishLauncher(Task task, int generation)
    {
        bool completed = false;
        try { await task; completed = true; }
        finally
        {
            // The original launcher can also swallow an exception and return
            // normally without ever reaching NGame.StartNewSingleplayerRun.
            if (generation == launchGeneration && (!completed || !enteredGameStart))
                RestoreFailedLaunch();
        }
    }
    static void LauncherThrew(Exception? __exception)
    {
        if (__exception is not null) RestoreFailedLaunch();
    }
    static void RestoreFailedLaunch()
    {
        launching = enteredGameStart = false;
        UiGuard.Run("guard launcher failure", ContentPools.End);
    }
    public static void OpenSelection(bool singleplayer)
    {
        selectionOpen = singleplayer;
        launching = false;
        launchGeneration++;
        if (singleplayer) ContentPools.BeginMenuScope(OriginalRulesSettings.Enabled);
    }
    public static void CloseSelection()
    {
        selectionOpen = false;
        if (!launching) ContentPools.End();
    }
    public static void ToggleChanged()
    {
        UiGuard.Run("guard menu toggle", () => {
            if (selectionOpen && !launching) ContentPools.BeginMenuScope(OriginalRulesSettings.Enabled);
        });
    }
    public static void ReturnedToMenu()
    {
        selectionOpen = launching = false;
        launchGeneration++;
        UiGuard.Run("guard main menu", () => ContentPools.BeginMenuScope(false));
    }
    static void Continue(ReadSaveResult<SerializableRun>? ____readRunSaveResult) =>
        UiGuard.Run("guard continue", () => {
            if (____readRunSaveResult is { Success: true, SaveData: { } save })
            {
                PrepareContinue(save.GameMode, save.Players.Count == 1 && !save.IsCustomGame && save.DailyTime is null && (save.Modifiers?.Count ?? 0) == 0);
                if (ContentPools.Active) ContentPools.ValidateSave(save);
            }
        });
    public static void PrepareContinue(GameMode mode, bool solo)
    {
        selectionOpen = false;
        launching = true;
        if (!OriginalRulesSettings.Enabled) { ContentPools.BeginMenuScope(false); return; }
        if (mode != GameMode.Standard || !solo) { ContentPools.UnsupportedMode(); return; }
        ContentPools.BeginMenuScope(true);
    }
    public static void PrepareLoad(SerializableRun save)
    {
        selectionOpen = true;
        launching = false;
        launchGeneration++;
        if (!OriginalRulesSettings.Enabled) { ContentPools.BeginMenuScope(false); return; }
        if (!ContentPools.ValidateSave(save)) { selectionOpen = false; return; }
        ContentPools.BeginMenuScope(true);
    }
}
