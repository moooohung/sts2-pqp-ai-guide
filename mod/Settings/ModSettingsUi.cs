using Godot;
using HarmonyLib;
using HumanPlayRecorder.Ai;
using MegaCrit.Sts2.Core.HoverTips;
using MegaCrit.Sts2.Core.Localization;
using MegaCrit.Sts2.Core.Modding;
using MegaCrit.Sts2.Core.Nodes.CommonUi;
using MegaCrit.Sts2.Core.Nodes.Screens.ModdingScreen;
using MegaCrit.Sts2.Core.Nodes.Screens.Settings;
using MegaCrit.Sts2.addons.mega_text;

namespace HumanPlayRecorder.Settings;

// STS2 has no per-mod settings screen (the modding screen only lists mods with an enable
// tickbox), so the toggle and the advisor status are rows in the game's own Settings screen,
// General tab, which is also reachable during a run. Both rows are clones of the game's
// "Show run timer" line: its label font, tickbox art, focus reticle, click sounds, hover tip
// and controller navigation. The cloned tickbox keeps its game script; the patches below stop
// it from touching the run-timer preference.
static partial class ModSettingsUi
{
    const string ModId = Edition.ModId;
    const string Template = "ShowRunTimer";
    const string RowName = "HumanPlayRecorderAiRecommendations";
    const string StatusName = "HumanPlayRecorderAiStatus";
    const string RulesName = "HumanPlayRecorderOriginalRules";
    static readonly StringName RulesMeta = "hpr_original_rules";
    static readonly StringName CloneMeta = "hpr_ai_recommendations";
    static readonly AccessTools.FieldRef<NRunTimerHoverTip, IHoverTip> hoverTip =
        AccessTools.FieldRefAccess<NRunTimerHoverTip, IHoverTip>("_hoverTip");

    sealed record Row(Node Line, MegaRichTextLabel Label, NTickbox? Tickbox, NRunTimerHoverTip? Tip, bool Status,
        Func<string>? Title = null, Func<bool>? Read = null, Func<bool>? Visible = null,
        Node? Divider = null, NSettingsPanel? Panel = null, Func<string>? Description = null);
    static readonly List<Row> rows = new();
    static readonly List<MegaRichTextLabel> modTitles = new();
    static readonly List<(NModInfoContainer Info, Mod Mod)> modInfos = new();

    static void Patch(Harmony harmony, Type type, string method, string? prefix = null, string? postfix = null) =>
        harmony.Patch(AccessTools.DeclaredMethod(type, method) ?? throw new MissingMethodException(type.Name, method),
            prefix: prefix is null ? null : new HarmonyMethod(typeof(ModSettingsUi), prefix),
            postfix: postfix is null ? null : new HarmonyMethod(typeof(ModSettingsUi), postfix));

    internal static void Install(Harmony harmony)
    {
        VanillaGuard.ContentPools.Install(harmony);
        // The run-timer guards go first: without them a cloned tickbox would drive the real
        // run-timer setting, so a failure there leaves the rows out (the exception aborts).
        foreach (string name in new[] { "OnTick", "OnUntick" })
            Patch(harmony, typeof(NRunTimerTickbox), name, prefix: nameof(OriginalUnlessClone));
        Patch(harmony, typeof(NRunTimerTickbox), nameof(NRunTimerTickbox.SetFromSettings), prefix: nameof(SetCloneFromSettings));
        Patch(harmony, typeof(NSettingsScreen), nameof(NSettingsScreen._Ready), postfix: nameof(AddRows));
        AiAdvisor.Changed += Refresh;
        RecommendationSettings.ShowRecommendationsChanged += Refresh;
        RecommendationSettings.BadgeVisibilityChanged += Refresh;
        OriginalRulesSettings.Changed += Refresh;
        InstallSharing();
        UiText.LanguageChanged += RefreshLanguage;
        UiText.Subscribe();
        UiGuard.Run("mod name hooks", () =>
        {
            Patch(harmony, typeof(NModMenuRow), nameof(NModMenuRow._Ready), postfix: nameof(NameModRow));
            Patch(harmony, typeof(NModInfoContainer), nameof(NModInfoContainer.Fill), postfix: nameof(NameModInfo));
        });
    }

    delegate void OptionRow(string name, Func<string> title, Func<bool> read, Action<bool> write,
        Func<bool> visible, bool disclosure = false, Func<string>? description = null);
    // Online edition only (ModSettingsUi.Sharing.cs): sharing settings, data deletion and policy links.
    static partial void InstallSharing();
    static partial void AddSharingRows(OptionRow option);

    // On a failure both prefixes run the game's original, as if the mod were absent.
    static bool OriginalUnlessClone(NRunTimerTickbox __instance) =>
        UiGuard.Run("settings tickbox", () => !__instance.HasMeta(CloneMeta), true);

    static bool SetCloneFromSettings(NRunTimerTickbox __instance) => UiGuard.Run("settings tickbox sync", () =>
    {
        if (!__instance.HasMeta(CloneMeta)) return true;
        __instance.IsTicked = __instance.HasMeta(RulesMeta)
            ? OriginalRulesSettings.Enabled : RecommendationSettings.ShowRecommendations;
        return false;
    }, true);

    static void AddRows(NSettingsScreen __instance)
    {
        var added = new List<Node>();
        try
        {
            var panel = __instance.GetNode<NSettingsPanel>("%GeneralSettings");
            var content = panel.Content;
            if (content.HasNode(RowName)) return;
            if (content.GetNodeOrNull<Control>(Template) is not { } template)
            {
                GD.Print("[HumanPlayRecorder] settings rows unavailable: no " + Template + " line");
                return;
            }
            int at = template.GetIndex() + 1;
            Node? divider = at < content.GetChildCount() && content.GetChild(at).Name.ToString().Contains("Divider")
                ? content.GetChild(at) : null;
            if (divider is not null) at++;
            var toggle = Clone(template, RowName);
            var rules = Clone(template, RulesName);
            var status = Clone(template, StatusName);
            var rulesDivider = Copy(divider, RulesName + "Divider");
            var statusDivider = Copy(divider, StatusName + "Divider");
            void Insert(Node? node)
            {
                if (node is null) return;
                content.AddChild(node);
                added.Add(node);
                content.MoveChild(node, at++);
            }
            Insert(toggle);
            Insert(Copy(divider, RowName + "Divider"));
            bool expanded = false;
            void Option(string name, Func<string> title, Func<bool> read, Action<bool> write,
                Func<bool> visible, bool disclosure = false, Func<string>? description = null)
            {
                var line = Clone(template, name);
                var separator = Copy(divider, name + "Divider");
                Insert(line);
                Insert(separator);
                var row = Wire(line, status: false, write) with {
                    Title = title, Read = read, Visible = visible, Divider = separator, Panel = panel,
                    Description = description
                };
                if (disclosure && row.Tickbox?.GetNodeOrNull<Control>("%TickboxVisuals") is { } visuals)
                    visuals.Visible = false;
                if (disclosure && row.Tickbox is { } tickbox)
                {
                    row.Label.MouseFilter = Control.MouseFilterEnum.Stop;
                    row.Label.MouseDefaultCursorShape = Control.CursorShape.PointingHand;
                    row.Label.Connect(Control.SignalName.GuiInput, Callable.From<InputEvent>(input =>
                        UiGuard.Run("settings categories mouse input", () =>
                        {
                            if (input is not InputEventMouseButton mouse ||
                                !IsDisclosureMouseRelease(mouse.ButtonIndex == MouseButton.Left, mouse.Pressed)) return;
                            row.Label.AcceptEvent();
                            tickbox.ForceToggleTick();
                        })));
                }
                rows.Add(row);
            }
            Option(RowName + "Categories",
                () => UiText.Get(expanded ? "settings_ai_categories_collapse" : "settings_ai_categories_expand"),
                () => expanded, value => { expanded = value; Refresh(); },
                () => RecommendationSettings.ShowRecommendations, disclosure: true);
            foreach (var category in Enum.GetValues<RecommendationCategory>())
            {
                Option(RowName + category, () => "    " + UiText.Get("settings_ai_category_" + category),
                    () => RecommendationSettings.IsEnabled(category),
                    value => RecommendationSettings.SetEnabled(category, value),
                    () => RecommendationSettings.ShowRecommendations && expanded);
            }
            Option(RowName + "TopPickOnly", () => "    " + UiText.Get("settings_ai_top_pick_only"),
                () => RecommendationSettings.TopPickOnly, value => RecommendationSettings.TopPickOnly = value,
                () => RecommendationSettings.ShowRecommendations && expanded);
            Insert(rules);
            Insert(rulesDivider);
            AddSharingRows(Option);
            Insert(status);
            Insert(statusDivider);
            rows.Add(Wire(toggle, status: false));
            rows.Add(Wire(rules, status: false));
            rows.Add(Wire(status, status: true) with {
                Visible = () => ShouldShowStatus(StatusText()), Divider = statusDivider, Panel = panel
            });
            Refresh();
            panel.Call(NSettingsPanel.MethodName.RefreshSize);
            panel.Call(NSettingsPanel.MethodName.UpdateNavigation);
        }
        catch (Exception e)
        {
            UiGuard.Report("settings rows", e);
            // Half-built rows are removed rather than left dead on the game's screen.
            UiGuard.Run("settings rows cleanup", () =>
            {
                rows.RemoveAll(r => added.Contains(r.Line));
                foreach (var node in added)
                    if (GodotObject.IsInstanceValid(node)) node.QueueFree();
            });
        }
    }

    const int CopyFlags = (int)(Node.DuplicateFlags.Groups | Node.DuplicateFlags.Scripts | Node.DuplicateFlags.UseInstantiation);

    static Node? Copy(Node? source, string name)
    {
        if (source is null) return null;
        var copy = source.Duplicate(CopyFlags);
        copy.Name = name;
        return copy;
    }

    // No signal copies, so the clone never drives the run-timer setting through the template's wiring.
    static Node Clone(Control template, string name)
    {
        var line = Copy(template, name)!;
        Find<NRunTimerTickbox>(line)?.SetMeta(CloneMeta, true);
        if (name == RulesName) Find<NRunTimerTickbox>(line)?.SetMeta(RulesMeta, true);
        return line;
    }

    // Main thread, after the line's _Ready. The status line keeps the cloned tickbox node (it may
    // be the line itself) but hides its art and takes it out of mouse and focus navigation.
    static Row Wire(Node line, bool status, Action<bool>? write = null)
    {
        var tickbox = Find<NRunTimerTickbox>(line);
        if (tickbox is not null && status)
        {
            if (tickbox.GetNodeOrNull<Control>("%TickboxVisuals") is { } visuals) visuals.Visible = false;
            tickbox.MouseFilter = Control.MouseFilterEnum.Ignore;
            tickbox.FocusMode = Control.FocusModeEnum.None;
        }
        else tickbox?.Connect(NTickbox.SignalName.Toggled, Callable.From<NTickbox>(t =>
            UiGuard.Run("settings toggle", () => {
                if (write is not null) write(t.IsTicked);
                else if (line.Name == RulesName) OriginalRulesSettings.Enabled = t.IsTicked;
                else RecommendationSettings.ShowRecommendations = t.IsTicked;
            })));
        return new(line, line.GetNode<MegaRichTextLabel>("Label"), tickbox, Find<NRunTimerHoverTip>(line), status);
    }

    static T? Find<T>(Node root) where T : Node
    {
        if (root is T found) return found;
        foreach (var child in root.GetChildren())
            if (Find<T>(child) is { } inner) return inner;
        return null;
    }

    internal static string StatusText() => AdvisorStatusText.Render(
        RecommendationSettings.ShowRecommendations, AiAdvisor.Status, AiAdvisor.LastFailureKind,
        AiAdvisor.LastRefusal, AiAdvisor.CheckpointLabel, UiText.Get, UiText.Format);

    internal static bool ShouldShowStatus(string text) => !string.IsNullOrWhiteSpace(text);

    internal static bool IsDisclosureMouseRelease(bool leftButton, bool pressed) => leftButton && !pressed;

    static IHoverTip Tip(bool rules = false, string? title = null, string? description = null)
    {
        // HoverTip only builds from game LocStrings; its title and text are then replaced.
        object tip = new HoverTip(new LocString("settings_ui", "SHOW_RUN_TIMER_HEADER"), "");
        AccessTools.Property(typeof(HoverTip), nameof(HoverTip.Title)).SetValue(tip,
            title ?? UiText.Get(rules ? "settings_original_rules" : "settings_show_ai_recommendations"));
        AccessTools.Property(typeof(HoverTip), nameof(HoverTip.Description)).SetValue(tip,
            title is not null ? description ?? UiText.Get("settings_ai_categories_description") :
            rules ? UiText.Get("settings_original_rules_description") :
            (UiText.Get("settings_show_ai_recommendations_description") + "\n\n" + StatusText()).TrimEnd());
        AccessTools.Property(typeof(HoverTip), nameof(HoverTip.Id)).SetValue(tip, RowName);
        return (IHoverTip)tip;
    }

    static string Plain(string text) => text.Replace("[", "[lb]");

    static void Refresh() => UiGuard.Run("settings refresh", () =>
    {
        rows.RemoveAll(r => !GodotObject.IsInstanceValid(r.Line));
        var resized = new HashSet<NSettingsPanel>();
        foreach (var row in rows)
        {
            if (row.Visible is { } visible && row.Line is Control control)
            {
                bool show = visible();
                if (control.Visible != show)
                {
                    control.Visible = show;
                    if (row.Panel is { } panel) resized.Add(panel);
                }
                if (row.Divider is CanvasItem separator) separator.Visible = show;
                // The game's navigation enumerator checks the parent, not the row's visibility.
                if (row.Tickbox is { } focus) focus.FocusMode = show ? Control.FocusModeEnum.All : Control.FocusModeEnum.None;
            }
            bool rules = row.Line.Name == RulesName;
            row.Label.Text = Plain(row.Title?.Invoke() ?? (row.Status ? StatusText() : UiText.Get(
                rules ? "settings_original_rules" : "settings_show_ai_recommendations")));
            if (row.Status) row.Label.Modulate = new Color(1, 1, 1, 0.7f);
            else if (row.Tickbox is { } tickbox) tickbox.IsTicked =
                row.Read?.Invoke() ?? (rules ? OriginalRulesSettings.Enabled : RecommendationSettings.ShowRecommendations);
            if (row.Tip is { } tip && GodotObject.IsInstanceValid(tip)) hoverTip(tip) = Tip(rules, row.Title?.Invoke(), row.Description?.Invoke());
        }
        foreach (var panel in resized)
        {
            panel.CallDeferred(NSettingsPanel.MethodName.RefreshSize);
            panel.Call(NSettingsPanel.MethodName.UpdateNavigation);
        }
    });

    static void RefreshLanguage()
    {
        Refresh();
        UiGuard.Run("mod name refresh", () =>
        {
            modTitles.RemoveAll(l => !GodotObject.IsInstanceValid(l));
            foreach (var title in modTitles) title.Text = Plain(UiText.Get("mod_name"));
            modInfos.RemoveAll(i => !GodotObject.IsInstanceValid(i.Info));
            foreach (var (info, mod) in modInfos.ToList()) info.Fill(mod);
        });
    }

    static void NameModRow(NModMenuRow __instance) => UiGuard.Run("mod row name", () =>
    {
        if (__instance.Mod?.manifest?.id != ModId) return;
        var title = __instance.GetNode<MegaRichTextLabel>("Title");
        title.Text = Plain(UiText.Get("mod_name"));
        modTitles.Add(title);
    });

    static void NameModInfo(NModInfoContainer __instance, Mod mod) => UiGuard.Run("mod info name", () =>
    {
        modInfos.RemoveAll(i => !GodotObject.IsInstanceValid(i.Info) || i.Info == __instance);
        if (mod.manifest?.id != ModId) return;
        __instance.GetNode<MegaRichTextLabel>("ModTitle").Text = Plain(UiText.Get("mod_name"));
        if (mod.manifest.description is { Length: > 0 } original)
        {
            var description = __instance.GetNode<MegaRichTextLabel>("ModDescription");
            description.Text = description.Text.Replace(original, Plain(UiText.Get("mod_description")));
        }
        modInfos.Add((__instance, mod));
    });
}
