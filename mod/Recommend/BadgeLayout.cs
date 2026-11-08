namespace HumanPlayRecorder.Recommend;

// Where a shop badge may sit. A badge stays within its own slot's hitbox sideways (scaled down when it is
// wider, top pick included) and never covers another item's price, so badges of neighbouring slots cannot
// meet whatever their text is. Plain floats: the layout is checked offline.
static class BadgeLayout
{
    /// <summary>Space kept between a badge and the edge of its slot.</summary>
    internal const float Margin = 4f;

    /// <summary>Scale, at most 1, that makes a badge <paramref name="width"/> wide fit a slot <paramref name="slotWidth"/> wide.</summary>
    internal static float Fit(float width, float slotWidth) =>
        width <= 0 ? 1f : Math.Min(1f, Math.Max(0f, slotWidth - 2 * Margin) / width);

    /// <summary>Centre for a badge <paramref name="width"/> wide (already fitted) that keeps it inside the slot.</summary>
    internal static float Centre(float centre, float width, float left, float right) =>
        Math.Min(Math.Max(centre, left + Margin + width / 2), right - Margin - width / 2);

    internal static bool Overlaps(float left1, float top1, float right1, float bottom1,
        float left2, float top2, float right2, float bottom2) =>
        left1 < right2 && left2 < right1 && top1 < bottom2 && top2 < bottom1;
}
