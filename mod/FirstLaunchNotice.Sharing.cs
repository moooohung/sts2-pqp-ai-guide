using Godot;
using HarmonyLib;
using MegaCrit.Sts2.Core.Nodes.Screens.MainMenu;

namespace HumanPlayRecorder;

// Online edition: first-launch consent popups, the main-menu replay button and upload notices.
public static partial class FirstLaunchNotice
{
    static int scheduled;
    static int exclusionShown;
    static readonly StorageNotice capacity = new();
    // Any thread; shown once the main menu is up (MainMenuReady), never lost when requested earlier.
    public static void ShowStorageCapacity()
    {
        if (!capacity.Request()) return;
        try { Callable.From(ShowCapacityNow).CallDeferred(); }
        catch (Exception e) { Console.Error.WriteLine("[HumanPlayRecorder] storage notice scheduling failed: " + e.GetType().Name); }
    }
    static void ShowCapacityNow() => capacity.Show(() => ShowShortNote(
        Settings.UiText.Get("storage_capacity_title"), LocalStorageText.CapacityBody,
        new(LocalStorageText.CapacityManage, LocalStorageView.ShowMenu)));

    public static void ShowExcluded(string reason)
    {
        if(Interlocked.Exchange(ref exclusionShown,1)!=0)return;
        try { Callable.From(()=>ShowShortNote("이 런은 AI 학습에서 제외됩니다",
            "게임플레이 변경이 확인되어 전송하지 않습니다.\n"+reason[..Math.Min(reason.Length,400)])).CallDeferred(); }
        catch(Exception){ }
    }

    public static void Install(Harmony harmony) => harmony.Patch(
        AccessTools.Method(typeof(NMainMenu), "_Ready"),
        postfix: new HarmonyMethod(typeof(FirstLaunchNotice), nameof(OnReady)));

    static void OnReady(NMainMenu __instance)
    {
        RuntimePatchAudit.Schedule();
        ReplayLinks.AddMenuButton(__instance);
        if (Interlocked.Exchange(ref scheduled, 1) != 0) return;
        Callable.From(MainMenuReady).CallDeferred();
    }

    // The training-data consent popup (it carries the complete localized disclosure) first, then the
    // start-up mod consent; GamePopup shows them one after another in the game's modal container.
    static void MainMenuReady()
    {
        if (!GameBuild.PolicyEnabled)
            ShowShortNote(Settings.UiText.Get("mod_name"), Settings.UiText.Get("compat_policy_paused"),
                closeAfterSeconds: 12);
#if DEBUG
        if (DebugUiCapture.Active)
        {
            DebugUiCapture.Begin();
            StartupConsent.Enable();
        }
#endif
        Settings.SharingSettings.OfferTrainingConsent();
        ReplayLinks.OfferConsent();
        StartupConsent.OnMainMenuReady();
        if (capacity.MenuReady()) ShowCapacityNow();
#if DEBUG
        if (DebugUiCapture.Active)
        {
            ReplayLinks.DebugPreview();
        }
#endif
    }
}
