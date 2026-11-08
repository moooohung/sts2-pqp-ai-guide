namespace HumanPlayRecorder.Recommend;

/// <summary>
/// The reason tip of an unavailable badge: up while the player hovers its option, gone the frame the pointer
/// leaves, the badge hides, or the text changes (the language). This only decides when; the owner shows and
/// removes the game's hover tip, so the badge never has to become a mouse target.
/// </summary>
/// <param name="show">Creates the tip; false when the game did not (hover tips are blocked), which is retried next frame.</param>
sealed class ReasonTip(Func<string, bool> show, Action hide)
{
    internal const float Gap = 8f;
    string? shown;

    internal bool Shown => shown is not null;

    /// <summary>Call every frame with the reason text the badge shows now (null when none) and whether its option is hovered.</summary>
    internal void Update(string? reason, bool hovering)
    {
        string? wanted = hovering ? reason : null;
        if (shown is not null && wanted != shown)
        {
            hide();
            shown = null;
        }
        if (wanted is not null && shown is null && show(wanted)) shown = wanted;
    }

    internal void Release() => Update(null, false);

    /// <summary>Whether the control the game reports as hovered is the host or inside it (never an overlay above it).</summary>
    internal static bool Within<T>(T? hovered, T host, Func<T, T?> parent) where T : class
    {
        for (T? node = hovered; node is not null; node = parent(node))
            if (ReferenceEquals(node, host)) return true;
        return false;
    }

    internal readonly record struct Box(float Left, float Top, float Right, float Bottom)
    {
        internal bool Intersects(Box o) => Left < o.Right && o.Left < Right && Top < o.Bottom && o.Top < Bottom;
    }

    /// <summary>
    /// Top-left of a tip of the given size: centred above the badge (below for a badge that sits under its option),
    /// else beside it, else beside one of the game's own tips; inside the viewport and never intersecting an
    /// obstacle. Null when nothing fits.
    /// </summary>
    internal static (float X, float Y)? Place(Box badge, float width, float height, float viewWidth, float viewHeight,
        bool below, IReadOnlyList<Box> obstacles)
    {
        float cx = (badge.Left + badge.Right) / 2 - width / 2, cy = (badge.Top + badge.Bottom) / 2 - height / 2;
        float above = badge.Top - Gap - height, under = badge.Bottom + Gap;
        var candidates = new List<(float X, float Y)>
        {
            (cx, below ? under : above), (cx, below ? above : under),
            (badge.Right + Gap, cy), (badge.Left - Gap - width, cy),
        };
        foreach (var o in obstacles)
        {
            candidates.Add((o.Left, o.Top - Gap - height));
            candidates.Add((o.Left, o.Bottom + Gap));
            candidates.Add((o.Right + Gap, o.Top));
            candidates.Add((o.Left - Gap - width, o.Top));
        }
        foreach (var (x, y) in candidates)
        {
            float px = Math.Clamp(x, 0f, Math.Max(0f, viewWidth - width)), py = Math.Clamp(y, 0f, Math.Max(0f, viewHeight - height));
            var box = new Box(px, py, px + width, py + height);
            if (!obstacles.Any(box.Intersects)) return (px, py);
        }
        return null;
    }
}
