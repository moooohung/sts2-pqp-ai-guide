using Godot;

namespace HumanPlayRecorder;

public sealed record GamePopupButton(string Text, Action OnPressed);

// The short non-modal note both editions use. The first-launch consent flow, the main-menu
// replay button and the upload notices are online-only (FirstLaunchNotice.Sharing.cs).
public static partial class FirstLaunchNotice
{
    static int queueFullScheduled;

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
}
