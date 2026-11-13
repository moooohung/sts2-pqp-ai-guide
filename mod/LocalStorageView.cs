using Godot;
using T = HumanPlayRecorder.LocalStorageText;

namespace HumanPlayRecorder;

// The runs this PC still keeps (queued, uncertified, too large, retained journals), each with its own delete,
// plus a local-only clear. Both free space on this PC only; replays on sts.pqp.me stay.
static class LocalStorageView
{
    internal static void ShowMenu()
    {
        GamePopup.EnqueueContent(T.Title, 980, area =>
            {
                var column = new VBoxContainer();
                column.SetAnchorsPreset(Control.LayoutPreset.FullRect);
                column.AddThemeConstantOverride("separation", 10);
                area.AddChild(column);
                column.AddChild(ReplayLinks.Text(T.Intro, 19));
                var summary = ReplayLinks.Plain("", 17, ReplayLinks.Gold);
                column.AddChild(summary);
                var scroll = new ScrollContainer { SizeFlagsVertical = Control.SizeFlags.ExpandFill,
                    HorizontalScrollMode = ScrollContainer.ScrollMode.Disabled };
                column.AddChild(scroll);
                var list = new VBoxContainer { SizeFlagsHorizontal = Control.SizeFlags.ExpandFill };
                list.AddThemeConstantOverride("separation", 8);
                scroll.AddChild(list);
                list.AddChild(ReplayLinks.Text(T.Loading, 18));
                _ = Task.Run(() =>
                {
                    IReadOnlyList<LocalRun>? runs;
                    try { runs = RecorderMod.LocalRuns(); }
                    catch (Exception e) { runs = null; GD.Print("[HumanPlayRecorder] local runs unavailable: " + e.GetType().Name); }
                    ReplayLinks.OnMain(() => Fill(list, summary, runs));
                });
            },
            new(T.Clear, ConfirmClear),
            new(T.NoteClose, () => { }),
            debugName: "local-storage");
    }

    static void ConfirmClear() => GamePopup.Enqueue(T.ClearTitle, T.ClearBody, 640,
        new(T.ClearConfirm, Clear), new(T.Cancel, ShowMenu), debugName: "local-storage-clear");

    static void Clear() => _ = Task.Run(async () =>
    {
        int left;
        try { left = await RecorderMod.DeleteLocalRunsAsync(RecorderMod.LocalRuns().Select(r => r.RunId).ToList()); }
        catch (Exception e) { left = -1; GD.Print("[HumanPlayRecorder] local clear failed: " + e.GetType().Name); }
        ReplayLinks.OnMain(() =>
        {
            if (left != 0) FirstLaunchNotice.ShowShortNote(T.Title, T.DeleteFailed);
            ShowMenu();
        });
    });

    static void Fill(VBoxContainer list, Label summary, IReadOnlyList<LocalRun>? runs)
    {
        if (!GodotObject.IsInstanceValid(list)) return;
        foreach (var child in list.GetChildren()) child.QueueFree();
        if (runs is null) { list.AddChild(ReplayLinks.Text(T.LoadFailed, 18)); return; }
        var shown = runs.ToList();
        void Update()
        {
            if (!GodotObject.IsInstanceValid(summary)) return;
            summary.Text = string.Format(Settings.UiText.Culture, T.Summary, shown.Count, Megabytes(shown.Sum(r => r.Bytes)));
            if (shown.Count == 0) list.AddChild(ReplayLinks.Text(T.Empty, 18));
        }
        foreach (var run in runs) list.AddChild(Row(run, () => { shown.Remove(run); Update(); }));
        Update();
    }

    static string Megabytes(long bytes) => (bytes / 1048576.0).ToString("0.0", Settings.UiText.Culture);

    static (string Text, Color Border) State(LocalRunState state) => state switch
    {
        LocalRunState.Pending => (T.StatePending, new Color(0.45f, 0.8f, 0.4f)),
        LocalRunState.Uncertified => (T.StateUncertified, new Color(0.6f, 0.6f, 0.6f)),
        LocalRunState.Oversize => (T.StateOversize, new Color(0.95f, 0.6f, 0.25f)),
        _ => (T.StateRetained, new Color(0.85f, 0.35f, 0.3f)),
    };

    static Control Row(LocalRun run, Action removed)
    {
        var (state, border) = State(run.State);
        var card = new PanelContainer { SizeFlagsHorizontal = Control.SizeFlags.ExpandFill };
        card.AddThemeStyleboxOverride("panel", ReplayLinks.Box(new Color(0, 0, 0, 0.3f), border));
        var column = new VBoxContainer { MouseFilter = Control.MouseFilterEnum.Pass };
        card.AddChild(column);
        var line = ReplayLinks.HRow(12);
        column.AddChild(line);
        line.AddChild(ReplayLinks.Plain(state, 18, ReplayLinks.Gold));
        if (run.Character is { } character) line.AddChild(ReplayLinks.Plain(ReplayLinks.Art.Character(character), 18));
        if (run.Ascension is { } ascension)
            line.AddChild(ReplayLinks.Plain(string.Format(Settings.UiText.Culture, T.RowAscension, ascension), 17, ReplayLinks.Muted));
        if (run.PartySize is { } party && party > 1)
            line.AddChild(ReplayLinks.Plain(string.Format(Settings.UiText.Culture, T.RowParty, party), 17, ReplayLinks.Muted));
        line.AddChild(ReplayLinks.Plain(run.UpdatedUtc.ToLocalTime().ToString("g", Settings.UiText.Culture), 17, ReplayLinks.Muted));
        line.AddChild(ReplayLinks.Plain(string.Format(Settings.UiText.Culture, T.RowSize, Megabytes(run.Bytes)), 17, ReplayLinks.Muted));
        line.AddChild(new Control { SizeFlagsHorizontal = Control.SizeFlags.ExpandFill, MouseFilter = Control.MouseFilterEnum.Ignore });
        var delete = ReplayLinks.RowButton(T.Delete);
        line.AddChild(delete);
        var failure = ReplayLinks.Text(T.DeleteFailed, 16, new Color(0.95f, 0.5f, 0.45f));
        failure.Visible = false;
        column.AddChild(failure);
        // Same two-step confirmation as Settings > Delete All My Data.
        bool armed = false;
        delete.Pressed += () =>
        {
            if (!armed) { armed = true; delete.Text = T.DeleteConfirm; return; }
            delete.Disabled = true;
            _ = Task.Run(async () =>
            {
                int left;
                try { left = await RecorderMod.DeleteLocalRunsAsync([run.RunId]); }
                catch (Exception e) { left = 1; GD.Print("[HumanPlayRecorder] local delete failed: " + e.GetType().Name); }
                ReplayLinks.OnMain(() =>
                {
                    if (!GodotObject.IsInstanceValid(card)) return;
                    if (left == 0) { card.QueueFree(); removed(); return; }
                    armed = false;
                    delete.Disabled = false;
                    delete.Text = T.Delete;
                    failure.Visible = true;
                });
            });
        };
        return card;
    }
}
