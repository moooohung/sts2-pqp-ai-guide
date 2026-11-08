using Godot;
using MegaCrit.Sts2.Core.Localization.Fonts;
using MegaCrit.Sts2.Core.Nodes.CommonUi;
using MegaCrit.Sts2.Core.Nodes.GodotExtensions;
using MegaCrit.Sts2.addons.mega_text;

namespace HumanPlayRecorder;

public sealed record ConsentPopupText(string Title, string Body, string Agree, string Decline);

// The training-data consent popup shows the complete localized disclosure (ui_strings.json disclosure_text,
// which states what is recorded, where it goes and what cannot be deleted) with the localized title and
// controls; no player-facing wording is written in code.
public static class ConsentPopup
{
    public static ConsentPopupText Text(Func<string, string> get) =>
        new(get("consent_title"), get("disclosure_text"), get("consent_agree"), get("consent_decline"));

    // The disclosure is long: it scrolls inside the popup's description area.
    internal static void Fill(Control area, string body)
    {
        var scroll = new ScrollContainer { HorizontalScrollMode = ScrollContainer.ScrollMode.Disabled };
        scroll.SetAnchorsPreset(Control.LayoutPreset.FullRect);
        var label = new Label { Text = body, AutowrapMode = TextServer.AutowrapMode.WordSmart,
            SizeFlagsHorizontal = Control.SizeFlags.ExpandFill, MouseFilter = Control.MouseFilterEnum.Ignore };
        if (GamePopup.KoreanFont is { } font) label.AddThemeFontOverride(ThemeConstants.Label.Font, font);
        label.AddThemeFontSizeOverride(ThemeConstants.Label.FontSize, 17);
        scroll.AddChild(label);
        area.AddChild(scroll);
    }
}
