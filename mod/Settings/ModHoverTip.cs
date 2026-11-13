using HarmonyLib;
using MegaCrit.Sts2.Core.HoverTips;

namespace HumanPlayRecorder.Settings;

// The game's HoverTip (a record struct) only takes game LocStrings, so the mod's tips start from a default value
// and get their title, text and id written directly. Since 0.113.0 `Id` has no setter at all (Title and
// Description keep private ones), so a member without a setter is written through its auto-property field.
static class ModHoverTip
{
    internal static IHoverTip Create(string title, string description, string id)
    {
        object tip = default(HoverTip);
        Set(tip, nameof(HoverTip.Title), title);
        Set(tip, nameof(HoverTip.Description), description);
        Set(tip, nameof(HoverTip.Id), id);
        return (IHoverTip)tip;
    }

    static void Set(object tip, string name, string value)
    {
        if (AccessTools.Property(typeof(HoverTip), name)?.GetSetMethod(true) is { } setter) setter.Invoke(tip, [value]);
        else (AccessTools.Field(typeof(HoverTip), $"<{name}>k__BackingField")
            ?? throw new MissingMemberException(nameof(HoverTip), name)).SetValue(tip, value);
    }
}
