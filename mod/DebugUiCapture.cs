using Godot;
using MegaCrit.Sts2.Core.Nodes.CommonUi;

namespace HumanPlayRecorder;

// Screenshot harness for the recorder's popups. Compiled only into Debug
// builds and only active with HPR_UI_CAPTURE_DIR set; Release builds (the
// Workshop package) contain the empty stubs below.
static class DebugUiCapture
{
#if DEBUG
    static readonly string? directory = System.Environment.GetEnvironmentVariable("HPR_UI_CAPTURE_DIR");
    static int pending, captured;
    public static bool Active => !string.IsNullOrEmpty(directory);

    public static void Begin()
    {
        DisplayServer.WindowSetMode(DisplayServer.WindowMode.Windowed);
        DisplayServer.WindowSetFlag(DisplayServer.WindowFlags.Borderless, true);
        DisplayServer.WindowSetSize(new Vector2I(1920, 1080));
        DisplayServer.WindowSetPosition(Vector2I.Zero);
        GD.Print("[HumanPlayRecorder] UI capture: started");
    }

    public static void Shown(Control popup, string name)
    {
        if (!Active || Engine.GetMainLoop() is not SceneTree tree) return;
        // The first capture waits out the main menu's fade-in.
        double delay = captured++ == 0 ? 6 : 1.5;
        pending++;
        tree.CreateTimer(delay).Timeout += () =>
        {
            Directory.CreateDirectory(directory!);
            var image = tree.Root.GetTexture().GetImage();
            string path = Path.Combine(directory!, name + ".png");
            image.SavePng(path);
            GD.Print($"[HumanPlayRecorder] UI capture: {name} {image.GetWidth()}x{image.GetHeight()}");
            pending--;
            NModalContainer.Instance?.Clear();
            GamePopup.Pump();
        };
    }

    public static void ForeignModalOpen(NModalContainer? container)
    {
        if (!Active || pending != 0 || container?.OpenModal is null) return;
        GD.Print("[HumanPlayRecorder] UI capture: closing game modal " + container.OpenModal.GetType().Name);
        container.Clear();
    }

    public static void QueueDrained()
    {
        if (!Active || pending != 0 || Engine.GetMainLoop() is not SceneTree tree) return;
        GD.Print("[HumanPlayRecorder] UI capture: done");
        // The bare main menu last, for buttons the recorder adds to it.
        tree.CreateTimer(1.0).Timeout += () =>
        {
            tree.Root.GetTexture().GetImage().SavePng(Path.Combine(directory!, "main-menu.png"));
            GD.Print("[HumanPlayRecorder] UI capture: main-menu");
            tree.Quit();
        };
    }
#else
    public static bool Active => false;
    public static void Begin() { }
    public static void Shown(Control popup, string name) { }
    public static void ForeignModalOpen(NModalContainer? container) { }
    public static void QueueDrained() { }
#endif
}
