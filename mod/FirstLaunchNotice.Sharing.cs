using Godot;
using HarmonyLib;
using MegaCrit.Sts2.Core.Nodes.Screens.MainMenu;

namespace HumanPlayRecorder;

// Online edition: first-launch consent popups, the main-menu replay button and upload notices.
public static partial class FirstLaunchNotice
{
    static int scheduled;
    static int exclusionShown;

    internal const string QueueFullText =
        "플레이 리플레이 기록의 로컬 대기열이 가득 차 새 런 기록을 저장하지 못했습니다.\n" +
        "네트워크 연결과 sts.pqp.me 상태를 확인한 뒤 게임을 다시 시작하세요.";

    public static void ShowQueueFull()
    {
        if (Interlocked.Exchange(ref queueFullScheduled, 1) != 0) return;
        try { Callable.From(ShowQueueFullDeferred).CallDeferred(); }
        catch (Exception e)
        {
            Console.Error.WriteLine("[HumanPlayRecorder] queue-full notice scheduling failed: " + e.GetType().Name);
            Interlocked.Exchange(ref queueFullScheduled, 0);
        }
    }

    static void ShowQueueFullDeferred()=>ShowShortNote("플레이 기록 대기열이 가득 찼습니다",QueueFullText);
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
#if DEBUG
        if (DebugUiCapture.Active)
        {
            ReplayLinks.DebugPreview();
        }
#endif
    }
}
