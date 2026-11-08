using Godot;
using HarmonyLib;
using HumanPlayRecorder.Ai;
using HumanPlayRecorder.Settings;
using MegaCrit.Sts2.Core.Helpers;
using MegaCrit.Sts2.Core.HoverTips;
using MegaCrit.Sts2.Core.Localization;
using MegaCrit.Sts2.Core.Localization.Fonts;
using MegaCrit.Sts2.Core.Nodes.HoverTips;

namespace HumanPlayRecorder.Recommend;

enum BadgeSide { Above, Below, Left, Right, RightInside }

// One "AI 37.4%" label beside one option node, drawn like the game's own numbers: Kreon Bold
// (or the language's substitute face, as the game's labels use), cream with the energy-cost
// outline, gold with the gold reward outline for the top pick. It is a Node2D child of the
// option node, so it follows the option's movement, hover scaling, visibility and lifetime
// without joining any container layout; it is counter-scaled to a constant on-screen size and
// is never a mouse target. An unavailable badge's reason is the game's own hover tip (NHoverTipSet), raised by
// polling the pointer while it is over the badge or its option: a label that ignores the mouse gets no
// Godot tooltip, and one that caught the mouse would take clicks and hover from the option underneath.
sealed class Badge
{
    internal const float Gap = 6f;
    static readonly Font? Kreon = ResourceLoader.Load<Font>("res://themes/kreon_bold_shared.tres");
    static readonly Color Shadow = new(0f, 0f, 0f, 0.45f);

    readonly Node2D root = new() { Name = "HumanPlayRecorderAiBadge", Visible = false };
    readonly Label label = new() { MouseFilter = Control.MouseFilterEnum.Ignore, FocusMode = Control.FocusModeEnum.None };
    Func<object?> option = () => null;
    Func<RecommendationCategory> category = () => RecommendationCategory.CardRewards;
    object? alternate;
    Func<Rect2> rect;
    // The horizontal span (host-local, like rect) the badge must stay inside, and the global rects of other items'
    // prices it must never be drawn over; shop slots set both.
    Func<(float Left, float Right)>? cell;
    Func<IEnumerable<Rect2>>? avoid;
    BadgeSide side;
    Func<Control, bool>? context;
    bool compact, contextKnown, contextOk, dirty = true;
    object? shown;
    // Most refusals last a frame or two while the game refreshes a screen (hand UI, a new
    // selection); only one that persists is shown, so choosing never flashes an error.
    const long UnavailableDelayMs = 1500;
    long unavailableSince;
    object? unavailableOption;
    string? unavailableReason;
    // The localized reason shown while the unavailable badge is up; null while a percentage is.
    string? reasonText;
    readonly ReasonTip reasonTip;
    // Any player's action (a teammate's card included) clears the advice until the next
    // inference returns; the option's last percentage stays up meanwhile so it never blinks.
    // Long enough for a crowded multiplayer frame budget to finish the next inference.
    const long PendingHoldMs = 5000;
    long pendingSince;
    object? adviceOption;
    // Nearest ancestor that clips its children (the ancients' scrolling dialogue box); a badge
    // beside its option must stay inside it or the clip cuts the text.
    Control? clip;
    bool clipKnown, wrapped;

    // A new percentage counts from the shown one, and the top pick's gold and size grow in,
    // over AnimateMs instead of jumping in one frame.
    const long AnimateMs = 300;
    internal const float TopEmphasis = 1.18f;
    float valueFrom = float.NaN, valueTo, emphasisFrom, emphasisTo, value, emphasis;
    bool showTop;
    long animateSince;

    bool broken;

    public Control Host { get; }
    public bool IsValid => !broken && GodotObject.IsInstanceValid(Host) && GodotObject.IsInstanceValid(root);

    /// <summary>The game's bold label face for the current language (Kreon Bold or its substitute).</summary>
    internal static Font? GameFont() => FontManager.GetSubstituteFont(UiText.Language, FontType.Bold) ?? Kreon;

    /// <summary>After a failure: the badge leaves its option for good; this never throws.</summary>
    public void Disable()
    {
        broken = true;
        UiGuard.Run("badge removal", () =>
        {
            if (GodotObject.IsInstanceValid(root)) root.QueueFree();
        });
        UiGuard.Run("badge reason removal", reasonTip.Release);
    }

    public Badge(Control host)
    {
        Host = host;
        reasonTip = new ReasonTip(ShowReason, () =>
        {
            reasonSet = null;
            NHoverTipSet.Remove(label);
        });
        rect = () => new Rect2(Vector2.Zero, host.Size);
        label.AddThemeConstantOverride("outline_size", 10);
        label.AddThemeColorOverride("font_shadow_color", Shadow);
        label.AddThemeConstantOverride("shadow_offset_x", 2);
        label.AddThemeConstantOverride("shadow_offset_y", 3);
        label.AddThemeConstantOverride("shadow_outline_size", 10);
        label.HorizontalAlignment = HorizontalAlignment.Center;
        root.AddChild(label);
        host.AddChild(root);
        // Pooled holders re-enter other screens; the screen decides whether they are options.
        host.TreeEntered += () => contextKnown = clipKnown = false;
    }

    public void Configure(Func<object?> option, object? alternate, Func<Rect2>? rect, BadgeSide side,
        Func<Control, bool>? context, bool compact, Func<RecommendationCategory> category,
        Func<(float Left, float Right)>? cell = null, Func<IEnumerable<Rect2>>? avoid = null)
    {
        this.avoid = avoid;
        this.option = option;
        this.alternate = alternate;
        if (rect is not null) this.rect = rect;
        this.cell = cell;
        this.side = side;
        this.context = context;
        this.compact = compact;
        this.category = category;
        contextKnown = false;
        dirty = true;
    }

    public void MarkDirty() => dirty = true;

    /// <summary>The option this badge stands for now, or null when it must stay hidden.</summary>
    public object? Current()
    {
        if (!RecommendationSettings.ShowRecommendations || !RecommendationSettings.IsEnabled(category()) ||
            !Host.IsVisibleInTree()) return null;
        if (!contextKnown)
        {
            contextOk = context?.Invoke(Host) ?? true;
            contextKnown = true;
            dirty = true;
        }
        if (!contextOk) return null;
        try { return option(); }
        catch (Exception) { return null; } // the option node is between models
    }

    public void Update(object? current)
    {
        Refresh(current);
        string? reason = root.Visible && root.Modulate.A > 0f ? reasonText : null;
        reasonTip.Update(reason, reason is not null && Hovered());
        if (reasonTip.Shown && reasonSet is not null && !Layout(reasonSet)) reasonTip.Release();
    }

    // The control the game itself reports as hovered is the option or inside it; the badge ignores the mouse, and a
    // modal or overlay above the option is that control instead, so it never raises the tip.
    bool Hovered() => Host.GetViewport() is { } viewport &&
        ReasonTip.Within(viewport.GuiGetHoveredControl(), Host, node => node.GetParent() as Control);

    static readonly System.Reflection.FieldInfo? ActiveTips = AccessTools.Field(typeof(NHoverTipSet), "_activeHoverTips");
    NHoverTipSet? reasonSet;

    // Beside the badge, around every other hover tip the game has up (each owner's own set).
    bool Layout(NHoverTipSet set)
    {
        var obstacles = new List<ReasonTip.Box>();
        // Without the game's tip list the reason could cover a game tip, so it is not shown at all.
        if (ActiveTips?.GetValue(null) is not System.Collections.IDictionary active) return false;
        foreach (System.Collections.DictionaryEntry entry in active)
            {
                if (ReferenceEquals(entry.Key, label) || entry.Value is not NHoverTipSet other || !GodotObject.IsInstanceValid(other)) continue;
                foreach (string name in new[] { "textHoverTipContainer", "cardHoverTipContainer" })
                    if (other.GetNodeOrNull<Control>(name) is { Visible: true } part && part.Size.X > 0 && part.Size.Y > 0)
                    {
                        var r = part.GetGlobalRect();
                        obstacles.Add(new(r.Position.X, r.Position.Y, r.End.X, r.End.Y));
                    }
            }
        if (set.GetNodeOrNull<Control>("textHoverTipContainer") is not { } box) return false;
        var on = label.GetGlobalTransform() * new Rect2(Vector2.Zero, label.Size);
        var view = Host.GetViewportRect().Size;
        if (ReasonTip.Place(new(on.Position.X, on.Position.Y, on.End.X, on.End.Y), box.Size.X, box.Size.Y, view.X, view.Y,
            side == BadgeSide.Below, obstacles) is not { } at) return false;
        set.GlobalPosition = new Vector2(at.X, at.Y);
        return true;
    }

    bool ShowReason(string text)
    {
        // HoverTip only builds from game LocStrings; its title and text are then replaced.
        object tip = new HoverTip(new LocString("settings_ui", "SHOW_RUN_TIMER_HEADER"), "");
        AccessTools.Property(typeof(HoverTip), nameof(HoverTip.Title)).SetValue(tip, UiText.Get(AdviceRefusal.BadgeKey));
        AccessTools.Property(typeof(HoverTip), nameof(HoverTip.Description)).SetValue(tip, text);
        AccessTools.Property(typeof(HoverTip), nameof(HoverTip.Id)).SetValue(tip, "pqp_ai_unavailable");
        // Null while the game blocks hover tips (card targeting): not shown, so it is tried again next frame.
        if (NHoverTipSet.CreateAndShow(label, (IHoverTip)tip) is not { } set) return false;
        if (!Layout(set))
        {
            NHoverTipSet.Remove(label);
            return false;
        }
        reasonSet = set;
        return true;
    }

    void Refresh(object? current)
    {
        if (dirty || !ReferenceEquals(current, shown))
        {
            dirty = false;
            shown = current;
            var (probability, top) = current is null ? (null, false) : Advice.For(current, alternate);
            if (probability is null && current is not null && AiAdvisor.UnavailableReason is null &&
                root.Visible && ReferenceEquals(current, adviceOption))
            {
                long now = System.Environment.TickCount64;
                if (pendingSince == 0) pendingSince = now;
                if (now - pendingSince < PendingHoldMs)
                {
                    dirty = true; // re-check next frame until the new advice arrives
                    Animate();
                    Place();
                    return;
                }
            }
            pendingSince = 0;
            if (probability is null) adviceOption = null;
            if (!RecommendationSettings.AllowsBadge(category(), top))
            {
                unavailableSince = 0;
                root.Visible = false;
                return;
            }
            if (probability is not { } p) {
                if (current is not null && AiAdvisor.Unavailable is { } refusal) {
                    string reason = refusal.Detail;
                    long now = System.Environment.TickCount64;
                    // The delay belongs to one option and one reason; a new one starts it again.
                    if (unavailableSince == 0 || !ReferenceEquals(current, unavailableOption) ||
                        !string.Equals(reason, unavailableReason, StringComparison.Ordinal)) {
                        unavailableSince = now;
                        unavailableOption = current;
                        unavailableReason = reason;
                    }
                    if (now - unavailableSince < UnavailableDelayMs) {
                        // A brief refusal (a screen being re-read) keeps this option's last
                        // percentage up instead of blanking it for a frame or two.
                        bool holding = root.Visible && ReferenceEquals(current, adviceOption);
                        root.Visible = holding;
                        dirty = true; // re-check next frame until it clears or persists
                        if (holding) { Animate(); Place(); }
                        return;
                    }
                    label.Text = UiText.Get(AdviceRefusal.BadgeKey);
                    valueFrom = float.NaN; emphasis = emphasisTo = 0f; adviceOption = null;
                    wrapped = true;
                    // The detail (ids, exception text, budgets) stays in the log, never in the tooltip.
                    reasonText = refusal.Tooltip(UiText.Get);
                    label.AddThemeFontSizeOverride("font_size", compact ? 18 : 22);
                    label.AddThemeColorOverride("font_color", StsColors.cream);
                    if (GameFont() is { } font) label.AddThemeFontOverride("font", font);
                    label.ResetSize();
                    root.Visible = true;
                } else {
                    unavailableSince = 0;
                    root.Visible = false;
                }
                return;
            }
            unavailableSince = 0;
            reasonText = null;
            // A different option (or a badge that was hidden) starts at its value; the same
            // option's new advice animates from what is on screen now.
            bool continuing = root.Visible && ReferenceEquals(current, adviceOption) && !float.IsNaN(valueFrom);
            Retarget(p, top, continuing);
            adviceOption = current;
            root.Visible = true;
        }
        if (root.Visible) { Animate(); Place(); }
    }

    void Retarget(float probability, bool top, bool continuing)
    {
        if (!continuing) { value = probability; emphasis = top ? 1f : 0f; }
        valueFrom = value; valueTo = probability;
        emphasisFrom = emphasis; emphasisTo = top ? 1f : 0f;
        showTop = top;
        animateSince = System.Environment.TickCount64;
        if (GameFont() is { } font)
            label.AddThemeFontOverride("font", font);
        label.AddThemeFontSizeOverride("font_size", compact ? 18 : 22);
        lastText = null;
        colored = float.NaN;
    }

    string? lastText;
    void Animate()
    {
        if (float.IsNaN(valueFrom)) return;
        float t = Math.Clamp((System.Environment.TickCount64 - animateSince) / (float)AnimateMs, 0f, 1f);
        float eased = 1f - (1f - t) * (1f - t) * (1f - t);
        value = valueFrom + (valueTo - valueFrom) * eased;
        emphasis = emphasisFrom + (emphasisTo - emphasisFrom) * eased;
        if (t < 1f) dirty = true; // keep stepping each frame until it settles
        // Compact badges (closely spaced slots such as the potion belt) round to whole percent.
        string percent = (value * 100f).ToString(compact ? "0" : "0.0", UiText.Culture);
        string text = UiText.Format(showTop ? "ai_top_pick_badge" : "ai_badge_format", percent);
        if (text != lastText)
        {
            lastText = text;
            label.Text = text;
            wrapped = false;
            label.ResetSize();
        }
        if (emphasis == colored) return;
        colored = emphasis;
        label.AddThemeColorOverride("font_color", StsColors.cream.Lerp(StsColors.gold, emphasis));
        label.AddThemeColorOverride("font_outline_color",
            StsColors.defaultEnergyCostOutline.Lerp(StsColors.rewardLabelGoldOutline, emphasis));
    }
    float colored = float.NaN;

    void Place()
    {
        Vector2 scale = Host.GetGlobalTransform().Scale;
        var inverse = new Vector2(scale.X != 0 ? 1f / scale.X : 1f, scale.Y != 0 ? 1f / scale.Y : 1f);
        // The top pick is drawn larger by scale (not font size) so the growth can animate.
        float k = 1f + (TopEmphasis - 1f) * emphasis;
        Rect2 r = rect();
        Vector2 center = r.GetCenter();
        // A badge with a cell shrinks to its width, top pick included, and stays inside it.
        if (cell?.Invoke() is { } span)
        {
            float natural = label.Size.X * k * inverse.X;
            float fit = BadgeLayout.Fit(natural, span.Right - span.Left);
            k *= fit;
            center.X = BadgeLayout.Centre(center.X, natural * fit, span.Left, span.Right);
        }
        root.Scale = inverse * k;
        Vector2 half = label.Size / 2 * k;
        label.Position = -label.Size / 2;
        root.Position = side switch
        {
            BadgeSide.Above => new(center.X, r.Position.Y - (Gap + half.Y) * inverse.Y),
            BadgeSide.Below => new(center.X, r.End.Y + (Gap + half.Y) * inverse.Y),
            BadgeSide.Left => new(r.Position.X - (Gap + half.X) * inverse.X, center.Y),
            BadgeSide.Right => new(r.End.X + (Gap + half.X) * inverse.X, center.Y),
            _ => new(r.End.X - (2 * Gap + half.X) * inverse.X, center.Y),
        };
        FitInsideClip(inverse);
        StepAside();
    }

    // A badge that would cover another item's price (a hovered shop card's price grows into the row below)
    // steps aside while it does, instead of drawing over it.
    void StepAside()
    {
        if (avoid is null) return;
        var size = label.Size * root.GetGlobalTransform().Scale;
        var at = root.GlobalPosition - size / 2;
        bool covers = avoid().Any(price => BadgeLayout.Overlaps(at.X, at.Y, at.X + size.X, at.Y + size.Y,
            price.Position.X, price.Position.Y, price.End.X, price.End.Y));
        root.Modulate = covers ? new Color(1f, 1f, 1f, 0f) : Colors.White;
    }

    // A badge cut by a clipping ancestor first breaks onto two lines at its last space
    // ("★ AI" over "100.0%"), then slides sideways back inside. Only horizontal: an option
    // scrolled out of the box vertically keeps its badge hidden with it.
    void FitInsideClip(Vector2 inverse)
    {
        if (!clipKnown)
        {
            clip = null;
            for (Node? n = Host; n is not null; n = n.GetParent())
                if (n is Control c && c.ClipContents) { clip = c; break; }
            clipKnown = true;
        }
        if (clip is null || !GodotObject.IsInstanceValid(clip)) return;
        Rect2 bounds = clip.GetGlobalRect();
        Rect2 badge = label.GetGlobalRect();
        bool cut = badge.Position.X < bounds.Position.X || badge.End.X > bounds.End.X;
        if (!cut) return;
        if (!wrapped)
        {
            wrapped = true;
            int space = label.Text.LastIndexOf(' ');
            if (space > 0)
            {
                label.Text = label.Text[..space] + "\n" + label.Text[(space + 1)..];
                label.ResetSize();
                Place();
                return;
            }
        }
        float dx = badge.Position.X < bounds.Position.X
            ? bounds.Position.X - badge.Position.X
            : bounds.End.X - badge.End.X;
        root.Position += new Vector2(dx * inverse.X, 0f);
    }
}
