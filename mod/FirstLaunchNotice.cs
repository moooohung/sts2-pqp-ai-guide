using Godot;
using HarmonyLib;
using MegaCrit.Sts2.Core.Nodes.Screens.MainMenu;

namespace HumanPlayRecorder;

public static class FirstLaunchNotice
{
    static int scheduled;
    static int queueFullScheduled;
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
    // Main thread. action adds a second button; closeAfterSeconds > 0 removes the note by itself.
    internal static void ShowShortNote(string heading,string text,GamePopupButton? action=null,double closeAfterSeconds=0)
    {
        if (Engine.GetMainLoop() is not SceneTree tree) { if (action is null) Interlocked.Exchange(ref queueFullScheduled, 0); return; }
        var layer = new CanvasLayer { Layer = 101 };
        var panel = new PanelContainer {
            Position = new Vector2(24, 24), CustomMinimumSize = new Vector2(560, 0),
            MouseFilter = Control.MouseFilterEnum.Ignore, FocusMode = Control.FocusModeEnum.None
        };
        var margin = new MarginContainer { MouseFilter = Control.MouseFilterEnum.Ignore };
        foreach (var side in new[] { "left", "right", "top", "bottom" })
            margin.AddThemeConstantOverride("margin_" + side, 20);
        var column = new VBoxContainer { MouseFilter = Control.MouseFilterEnum.Ignore };
        var font = new SystemFont { FontNames = ["Malgun Gothic", "Noto Sans CJK KR", "sans-serif"] };
        var title = new Label { Text = heading, MouseFilter = Control.MouseFilterEnum.Ignore };
        title.AddThemeFontOverride("font", font); title.AddThemeFontSizeOverride("font_size", 22);
        var body = new Label { Text = text, CustomMinimumSize = new Vector2(520, 0),
            AutowrapMode = TextServer.AutowrapMode.WordSmart, MouseFilter = Control.MouseFilterEnum.Ignore };
        body.AddThemeFontOverride("font", font); body.AddThemeFontSizeOverride("font_size", 17);
        var close = new Button { Text = "닫기", FocusMode = Control.FocusModeEnum.None,
            CustomMinimumSize = new Vector2(100, 44) };
        close.AddThemeFontOverride("font", font);
        close.Pressed += () => { if (GodotObject.IsInstanceValid(layer)) layer.QueueFree(); };
        column.AddChild(title); column.AddChild(body);
        if (action is null) column.AddChild(close);
        else
        {
            var open = new Button { Text = action.Text, FocusMode = Control.FocusModeEnum.None,
                CustomMinimumSize = new Vector2(160, 44) };
            open.AddThemeFontOverride("font", font);
            open.Pressed += () =>
            {
                try { action.OnPressed(); }
                catch (Exception e) { Console.Error.WriteLine("[HumanPlayRecorder] note action failed: " + e.GetType().Name); }
                if (GodotObject.IsInstanceValid(layer)) layer.QueueFree();
            };
            var row = new HBoxContainer { MouseFilter = Control.MouseFilterEnum.Ignore };
            row.AddThemeConstantOverride("separation", 12);
            row.AddChild(open); row.AddChild(close);
            column.AddChild(row);
        }
        margin.AddChild(column); panel.AddChild(margin); layer.AddChild(panel); tree.Root.AddChild(layer);
        if (closeAfterSeconds > 0)
            tree.CreateTimer(closeAfterSeconds).Timeout += () => { if (GodotObject.IsInstanceValid(layer)) layer.QueueFree(); };
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
