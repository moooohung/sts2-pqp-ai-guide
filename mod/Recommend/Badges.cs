using System.Runtime.CompilerServices;
using Godot;
using HumanPlayRecorder.Ai;
using HumanPlayRecorder.Settings;

namespace HumanPlayRecorder.Recommend;

// Where a badge's numbers come from: AiAdvisor only. Debug builds alone add a preview:
// HPR_AI_BADGE_PREVIEW=1 (read once at start-up) replaces AiAdvisor with stable fake
// probabilities over the options on screen, through the same badges, for visual checks in
// the real game, under an on-screen banner saying so. Release builds contain none of it.
static class Advice
{
#if DEBUG
    internal static readonly bool Preview = System.Environment.GetEnvironmentVariable("HPR_AI_BADGE_PREVIEW") == "1";
#endif

    internal static (float? Probability, bool Top) For(object option, object? alternate)
    {
#if DEBUG
        if (Preview) return PreviewAdvice.For(option);
#endif
        if (AiAdvisor.ProbabilityFor(option) is { } p) return (p, AiAdvisor.IsTopPick(option));
        if (alternate is not null && AiAdvisor.ProbabilityFor(alternate) is { } q) return (q, AiAdvisor.IsTopPick(alternate));
        return (null, false);
    }
}

#if DEBUG
// Top-centre banner over every screen while preview numbers are shown, so fake percentages
// cannot pass for real advice in screenshots. Never a mouse target.
static class PreviewBanner
{
    static Label? label;

    internal static void Show()
    {
        if (label is null || !GodotObject.IsInstanceValid(label))
        {
            if (Engine.GetMainLoop() is not SceneTree tree) return;
            var layer = new CanvasLayer { Name = "HumanPlayRecorderAiPreview", Layer = 128 };
            label = new Label
            {
                MouseFilter = Control.MouseFilterEnum.Ignore,
                FocusMode = Control.FocusModeEnum.None,
                HorizontalAlignment = HorizontalAlignment.Center,
            };
            label.SetAnchorsAndOffsetsPreset(Control.LayoutPreset.TopWide);
            label.OffsetTop = 12;
            label.AddThemeFontSizeOverride("font_size", 26);
            label.AddThemeColorOverride("font_color", MegaCrit.Sts2.Core.Helpers.StsColors.gold);
            label.AddThemeColorOverride("font_outline_color", MegaCrit.Sts2.Core.Helpers.StsColors.rewardLabelGoldOutline);
            label.AddThemeConstantOverride("outline_size", 10);
            layer.AddChild(label);
            // Deferred: this runs from the tree's frame signal, while the root may be busy.
            tree.Root.CallDeferred(Node.MethodName.AddChild, layer);
        }
        string text = UiText.Get("ai_preview_label");
        if (label.Text == text) return;
        label.Text = text;
        if (Badge.GameFont() is { } font) label.AddThemeFontOverride("font", font);
    }
}

static class PreviewAdvice
{
    static readonly Dictionary<object, float> values = new(ReferenceEqualityComparer.Instance);
    static object? top;
    static int signature;

    // Called every frame with the options on screen; true when the fake distribution changed.
    internal static bool Update(List<object> options)
    {
        int next = options.Count;
        foreach (var option in options) next = HashCode.Combine(next, RuntimeHelpers.GetHashCode(option));
        if (next == signature) return false;
        signature = next;
        values.Clear();
        top = null;
        if (options.Count < 2) return true; // forced choices get no advice
        float sum = 0;
        foreach (var option in options)
        {
            if (values.ContainsKey(option)) continue;
            float weight = 0.05f + (uint)RuntimeHelpers.GetHashCode(option) % 1000 / 1000f;
            values[option] = weight;
            sum += weight;
        }
        foreach (var option in values.Keys.ToList())
        {
            values[option] /= sum;
            if (top is null || values[option] > values[top]) top = option;
        }
        return true;
    }

    internal static (float?, bool) For(object option) =>
        values.TryGetValue(option, out float p) ? (p, ReferenceEquals(option, top)) : (null, false);
}
#endif

static class Badges
{
    static readonly Dictionary<Control, Badge> byHost = new(ReferenceEqualityComparer.Instance);
    static readonly List<Badge> badges = new();
#if DEBUG
    static readonly List<object> onScreen = new();
#endif
    static bool frameHooked;

    internal static void Subscribe()
    {
        AiAdvisor.Changed += MarkAllDirty;
        RecommendationSettings.ShowRecommendationsChanged += MarkAllDirty;
        RecommendationSettings.BadgeVisibilityChanged += MarkAllDirty;
        UiText.LanguageChanged += MarkAllDirty;
    }

    static void MarkAllDirty() => UiGuard.Run("badge refresh", () =>
    {
        foreach (var badge in badges) badge.MarkDirty();
    });

    internal static void HookFrame()
    {
        if (frameHooked || Engine.GetMainLoop() is not SceneTree tree) return;
        frameHooked = true;
        tree.ProcessFrame += Frame;
    }

    /// <summary>
    /// Main thread. Shows <paramref name="option"/>'s advice beside <paramref name="host"/>;
    /// <paramref name="alternate"/> is asked when the option itself has none. Calling it again
    /// for the same host (pooled nodes) replaces the badge's option.
    /// </summary>
    internal static void Attach(Control? host, Func<object?> option, Func<RecommendationCategory> category, BadgeSide side,
        Func<Rect2>? rect = null, object? alternate = null, Func<Control, bool>? context = null, bool compact = false,
        Func<(float Left, float Right)>? cell = null, Func<IEnumerable<Rect2>>? avoid = null)
    {
        if (host is null || !GodotObject.IsInstanceValid(host)) return;
        UiGuard.Run("badge attach", () =>
        {
            if (!byHost.TryGetValue(host, out var badge) || !badge.IsValid)
            {
                badge = new Badge(host);
                byHost[host] = badge;
                badges.Add(badge);
            }
            badge.Configure(option, alternate, rect, side, context, compact, category, cell, avoid);
            HookFrame();
        });
    }

    static void Frame() => UiGuard.Run("badge frame", () =>
    {
        for (int i = badges.Count - 1; i >= 0; i--)
        {
            if (badges[i].IsValid) continue;
            byHost.Remove(badges[i].Host);
            badges.RemoveAt(i);
        }
#if DEBUG
        if (Advice.Preview)
        {
            PreviewBanner.Show();
            onScreen.Clear();
            foreach (var badge in badges)
                if (Guarded(badge, badge.Current) is { } option) onScreen.Add(option);
            if (PreviewAdvice.Update(onScreen)) MarkAllDirty();
        }
#endif
        foreach (var badge in badges) Guarded(badge, () => { badge.Update(badge.Current()); return null; });
    });

    // One failing badge is taken off its option (and dropped next frame); the others keep updating.
    static object? Guarded(Badge badge, Func<object?> step)
    {
        if (!badge.IsValid) return null;
        try { return step(); }
        catch (Exception e)
        {
            UiGuard.Report("badge update", e);
            badge.Disable();
            return null;
        }
    }
}
