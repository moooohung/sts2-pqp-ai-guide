using Godot;
using MegaCrit.Sts2.Core.Localization.Fonts;
using MegaCrit.Sts2.Core.Nodes.CommonUi;
using MegaCrit.Sts2.Core.Nodes.GodotExtensions;
using MegaCrit.Sts2.Core.Nodes.Multiplayer;
using MegaCrit.Sts2.addons.mega_text;

namespace HumanPlayRecorder;

// Shows text in the game's own modal: res://scenes/ui/generic_popup.tscn
// (NGenericPopup > NVerticalPopup, popup_vertical panel art, NPopupYesNoButton
// confirm/cancel buttons with their hover/click sounds and hotkeys) inside
// NModalContainer with its backstop. Nothing from the game is copied into the mod.
static class GamePopup
{
    const float PanelAspect = 816f / 716f; // popup_vertical atlas region
    const string CancelButtonScene = "res://scenes/ui/abandon_run_no_button.tscn";
    sealed record Request(string Title, string Body, float Width, GamePopupButton Confirm,
        GamePopupButton? Cancel, GamePopupButton? Extra, string DebugName, Action<Control>? Fill = null);
    static readonly Queue<Request> queue = new();
    static bool retryScheduled;

    // Main thread only. Popups open one at a time, after any modal already open.
    public static void Enqueue(string title, string body, float width, GamePopupButton confirm,
        GamePopupButton? cancel = null, GamePopupButton? extra = null, string debugName = "popup")
    {
        queue.Enqueue(new(title, body, width, confirm, cancel, extra, debugName));
        Pump();
    }

    // The same modal with the body replaced by caller-built controls (lists, row buttons)
    // laid out in the description's area; buttons inside it do not close the popup.
    public static void EnqueueContent(string title, float width, Action<Control> fill, GamePopupButton confirm,
        GamePopupButton? cancel = null, GamePopupButton? extra = null, string debugName = "popup")
    {
        queue.Enqueue(new(title, "", width, confirm, cancel, extra, debugName, fill));
        Pump();
    }

    internal static Font? KoreanFont => FontManager.GetSubstituteFont("kor", FontType.Bold);

    internal static void Pump()
    {
        if (queue.Count == 0) { DebugUiCapture.QueueDrained(); return; }
        var container = NModalContainer.Instance;
        if (container is null || container.OpenModal is not null)
        {
            DebugUiCapture.ForeignModalOpen(container);
            if (retryScheduled || Engine.GetMainLoop() is not SceneTree tree) return;
            retryScheduled = true;
            tree.CreateTimer(0.25).Timeout += () => { retryScheduled = false; Pump(); };
            return;
        }
        var request = queue.Dequeue();
        try { Show(container, request); }
        catch (Exception e)
        {
            GD.Print($"[HumanPlayRecorder] popup '{request.DebugName}' unavailable: {e.GetType().Name}");
            container.Clear();
            Pump();
        }
    }

    static void Show(NModalContainer container, Request r)
    {
        var popup = NGenericPopup.Create() ?? throw new InvalidOperationException("generic popup unavailable");
        container.Add(popup);
        var vertical = popup.GetNode<NVerticalPopup>("VerticalPopup");
        // Keep the panel art's aspect so header, body and buttons stay on it.
        float height = r.Width * PanelAspect;
        var center = new Vector2(261, 300); // generic_popup.tscn root is 522x600, screen-centred
        vertical.OffsetLeft = center.X - r.Width / 2;
        vertical.OffsetRight = center.X + r.Width / 2;
        vertical.OffsetTop = center.Y - height / 2;
        vertical.OffsetBottom = center.Y + height / 2;
        Korean(vertical.GetNode<Control>("Header"), ThemeConstants.Label.Font);
        var description = vertical.GetNode<RichTextLabel>("Description");
        Korean(description, ThemeConstants.RichTextLabel.NormalFont, ThemeConstants.RichTextLabel.BoldFont);
        // Body starts right under the title instead of floating mid-panel.
        description.VerticalAlignment = VerticalAlignment.Top;
        vertical.SetText(r.Title, r.Body.Replace("[", "[lb]"));
        if (r.Fill is not null)
        {
            var area = new Control {
                AnchorLeft = description.AnchorLeft, AnchorRight = description.AnchorRight,
                AnchorTop = description.AnchorTop, AnchorBottom = description.AnchorBottom,
                OffsetLeft = description.OffsetLeft, OffsetRight = description.OffsetRight,
                OffsetTop = description.OffsetTop, OffsetBottom = description.OffsetBottom,
                MouseFilter = Control.MouseFilterEnum.Pass,
            };
            description.Visible = false;
            vertical.AddChild(area);
            r.Fill(area);
        }
        // Keep the buttons (180 wide, 196-wide art) inside the panel border.
        float inset = MathF.Round(r.Width * 0.05f);
        vertical.NoButton.OffsetLeft = inset;
        vertical.NoButton.OffsetRight = inset + 180;
        vertical.YesButton.OffsetLeft = -(inset + 196);
        vertical.YesButton.OffsetRight = -(inset + 16);

        bool done = false;
        var labels = new List<MegaLabel>();
        void Wire(NPopupYesNoButton button, GamePopupButton spec)
        {
            var label = button.GetNode<MegaLabel>("%Label");
            Korean(label, ThemeConstants.Label.Font);
            labels.Add(label);
            button.SetText(spec.Text);
            button.Connect(NClickableControl.SignalName.Released, Callable.From<NButton>(_ =>
            {
                if (done) return;
                done = true;
                NModalContainer.Instance?.Clear();
                try { spec.OnPressed(); }
                catch (Exception e) { GD.Print("[HumanPlayRecorder] popup action failed: " + e.GetType().Name); }
                Pump();
            }));
        }
        vertical.YesButton.IsYes = true;
        Wire(vertical.YesButton, r.Confirm);
        if (r.Cancel is null) vertical.HideNoButton();
        else
        {
            vertical.NoButton.IsYes = false;
            Wire(vertical.NoButton, r.Cancel);
        }
        if (r.Extra is not null)
        {
            // A second cancel-style button from the game's own scene, centred
            // between the native pair; only the native No button keeps Esc.
            var extra = ResourceLoader.Load<PackedScene>(CancelButtonScene).Instantiate<NPopupYesNoButton>();
            vertical.AddChild(extra);
            extra.DisconnectHotkeys();
            extra.AnchorLeft = extra.AnchorRight = 0.5f;
            extra.AnchorTop = extra.AnchorBottom = 1f;
            extra.OffsetLeft = -98; extra.OffsetRight = 82;
            extra.OffsetTop = -152; extra.OffsetBottom = -80;
            Wire(extra, r.Extra);
        }
        Callable.From(() => EqualizeLabels(labels)).CallDeferred();
        DebugUiCapture.Shown(popup, r.DebugName);
    }

    // MegaLabel auto-sizes each button on its own; cap them all at the size
    // the longest label needed so every button reads at the same size.
    static void EqualizeLabels(List<MegaLabel> labels)
    {
        labels = labels.Where(l => GodotObject.IsInstanceValid(l) && l.IsVisibleInTree()).ToList();
        if (labels.Count < 2) return;
        int size = labels.Min(l => l.GetThemeFontSize(ThemeConstants.Label.FontSize));
        foreach (var label in labels)
        {
            string text = label.Text;
            label.MaxFontSize = size;
            label.SetTextAutoSize("");
            label.SetTextAutoSize(text);
        }
    }

    // The recorder's text is Korean; use the game's own Korean face
    // (GyeonggiCheonnyeonBatang Bold via FontManager) whatever the game language.
    static void Korean(Control control, params StringName[] themeFonts)
    {
        var font = FontManager.GetSubstituteFont("kor", FontType.Bold);
        if (font is null) return;
        foreach (var name in themeFonts) control.AddThemeFontOverride(name, font);
    }
}
