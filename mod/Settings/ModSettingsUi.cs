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
    // Godot names live apart from the row definitions, so those can be read without a Godot host.
    static class Meta
    {
        internal static readonly StringName Rules = "hpr_original_rules";
        internal static readonly StringName Clone = "hpr_ai_recommendations";
    }
    static readonly AccessTools.FieldRef<NRunTimerHoverTip, IHoverTip> hoverTip =
        AccessTools.FieldRefAccess<NRunTimerHoverTip, IHoverTip>("_hoverTip");

    internal enum RowKind { Option, Status }
    // One row in screen order: its label, its hover tip and what its tickbox reads and writes. AddRows clones a
    // game line for each; Refresh and the settings-text regression read the texts from here.
    internal sealed record RowDef(string Name, Func<string> Label, Func<string> TipTitle, Func<string> TipText,
        Func<bool>? Visible = null, Func<bool>? Read = null, Action<bool>? Write = null, bool Disclosure = false,
        RowKind Kind = RowKind.Option);
    sealed record Row(RowDef Def, Node Line, MegaRichTextLabel Label, NTickbox? Tickbox, NRunTimerHoverTip? Tip,
        Node? Divider, NSettingsPanel Panel);
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

    delegate RowDef OptionRow(string name, Func<string> title, Func<bool> read, Action<bool> write,
        Func<bool> visible, bool disclosure = false, Func<string>? description = null);
    // Online edition only (ModSettingsUi.Sharing.cs): sharing settings, data deletion and policy links.
    static partial void InstallSharing();
    static partial void AddSharingRows(List<RowDef> defs, OptionRow option);

    // On a failure both prefixes run the game's original, as if the mod were absent.
    static bool OriginalUnlessClone(NRunTimerTickbox __instance) =>
        UiGuard.Run("settings tickbox", () => !__instance.HasMeta(Meta.Clone), true);

    static bool SetCloneFromSettings(NRunTimerTickbox __instance) => UiGuard.Run("settings tickbox sync", () =>
    {
        if (!__instance.HasMeta(Meta.Clone)) return true;
        __instance.IsTicked = __instance.HasMeta(Meta.Rules)
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
            void Insert(Node? node)
            {
                if (node is null) return;
                content.AddChild(node);
                added.Add(node);
                content.MoveChild(node, at++);
            }
            foreach (var def in Definitions())
            {
                var line = Clone(template, def.Name);
                var separator = Copy(divider, def.Name + "Divider");
                Insert(line);
                Insert(separator);
                var row = Wire(def, line, separator, panel);
                if (def.Disclosure && row.Tickbox?.GetNodeOrNull<Control>("%TickboxVisuals") is { } visuals)
                    visuals.Visible = false;
                if (def.Disclosure && row.Tickbox is { } tickbox)
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

    // Screen order. Guide off for this game build (game-build.json): the recommendation rows stay hidden and the
    // status line says why; consent, replay saving, deletion, local storage and the links are always shown.
    internal static IReadOnlyList<RowDef> Definitions()
    {
        bool guide = GameBuild.PolicyEnabled;
        bool expanded = false; // per settings screen, like every row's state below
        string Ai() => UiText.Get("settings_show_ai_recommendations");
        string AiTip() => (UiText.Get("settings_show_ai_recommendations_description") + "\n\n" + StatusText()).TrimEnd();
        string CategoriesTip() => UiText.Get("settings_ai_categories_description");
        RowDef Option(string name, Func<string> label, Func<bool> read, Action<bool> write, Func<bool> visible,
            bool disclosure = false, Func<string>? description = null) =>
            new(name, label, label, description ?? CategoriesTip, visible, read, write, disclosure);
        var defs = new List<RowDef>
        {
            new(RowName, Ai, Ai, AiTip, () => guide, () => RecommendationSettings.ShowRecommendations,
                value => RecommendationSettings.ShowRecommendations = value),
            Option(RowName + "Categories",
                () => UiText.Get(expanded ? "settings_ai_categories_collapse" : "settings_ai_categories_expand"),
                () => expanded, value => { expanded = value; Refresh(); },
                () => guide && RecommendationSettings.ShowRecommendations, disclosure: true),
        };
        foreach (var category in Enum.GetValues<RecommendationCategory>())
            defs.Add(Option(RowName + category, () => "    " + UiText.Get("settings_ai_category_" + category),
                () => RecommendationSettings.IsEnabled(category),
                value => RecommendationSettings.SetEnabled(category, value),
                () => guide && RecommendationSettings.ShowRecommendations && expanded));
        defs.Add(Option(RowName + "TopPickOnly", () => "    " + UiText.Get("settings_ai_top_pick_only"),
            () => RecommendationSettings.TopPickOnly, value => RecommendationSettings.TopPickOnly = value,
            () => guide && RecommendationSettings.ShowRecommendations && expanded));
        defs.Add(new(RulesName, () => UiText.Get("settings_original_rules"), () => UiText.Get("settings_original_rules"),
            () => UiText.Get("settings_original_rules_description"), Read: () => OriginalRulesSettings.Enabled,
            Write: value => OriginalRulesSettings.Enabled = value));
        AddSharingRows(defs, Option);
        defs.Add(new(StatusName, StatusText, Ai, AiTip, () => ShouldShowStatus(StatusText()), Kind: RowKind.Status));
        return defs;
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
        Find<NRunTimerTickbox>(line)?.SetMeta(Meta.Clone, true);
        if (name == RulesName) Find<NRunTimerTickbox>(line)?.SetMeta(Meta.Rules, true);
        return line;
    }

    // Main thread, after the line's _Ready. The status line keeps the cloned tickbox node (it may
    // be the line itself) but hides its art and takes it out of mouse and focus navigation.
    static Row Wire(RowDef def, Node line, Node? divider, NSettingsPanel panel)
    {
        var tickbox = Find<NRunTimerTickbox>(line);
        if (tickbox is not null && def.Kind == RowKind.Status)
        {
            if (tickbox.GetNodeOrNull<Control>("%TickboxVisuals") is { } visuals) visuals.Visible = false;
            tickbox.MouseFilter = Control.MouseFilterEnum.Ignore;
            tickbox.FocusMode = Control.FocusModeEnum.None;
        }
        else if (def.Write is { } write) tickbox?.Connect(NTickbox.SignalName.Toggled, Callable.From<NTickbox>(t =>
            UiGuard.Run("settings toggle", () => write(t.IsTicked))));
        return new(def, line, line.GetNode<MegaRichTextLabel>("Label"), tickbox, Find<NRunTimerHoverTip>(line), divider, panel);
    }

    static T? Find<T>(Node root) where T : Node
    {
        if (root is T found) return found;
        foreach (var child in root.GetChildren())
            if (Find<T>(child) is { } inner) return inner;
        return null;
    }

    internal static string StatusText() => !GameBuild.PolicyEnabled ? UiText.Get("compat_policy_paused") : AdvisorStatusText.Render(
        RecommendationSettings.ShowRecommendations, AiAdvisor.Status, AiAdvisor.LastFailureKind,
        AiAdvisor.LastRefusal, AiAdvisor.CheckpointLabel, UiText.Get, UiText.Format);

    internal static bool ShouldShowStatus(string text) => !string.IsNullOrWhiteSpace(text);

    internal static bool IsDisclosureMouseRelease(bool leftButton, bool pressed) => leftButton && !pressed;

    internal static IHoverTip Tip(RowDef def) => ModHoverTip.Create(def.TipTitle(), def.TipText(), RowName);

    static string Plain(string text) => text.Replace("[", "[lb]");

    static void Refresh() => UiGuard.Run("settings refresh", () =>
    {
        rows.RemoveAll(r => !GodotObject.IsInstanceValid(r.Line));
        var resized = new HashSet<NSettingsPanel>();
        foreach (var row in rows)
        {
            var def = row.Def;
            if (def.Visible is { } visible && row.Line is Control control)
            {
                bool show = visible();
                if (control.Visible != show)
                {
                    control.Visible = show;
                    resized.Add(row.Panel);
                }
                if (row.Divider is CanvasItem separator) separator.Visible = show;
                // The game's navigation enumerator checks the parent, not the row's visibility.
                if (row.Tickbox is { } focus) focus.FocusMode = show ? Control.FocusModeEnum.All : Control.FocusModeEnum.None;
            }
            row.Label.Text = Plain(def.Label());
            if (def.Kind == RowKind.Status) row.Label.Modulate = new Color(1, 1, 1, 0.7f);
            else if (row.Tickbox is { } tickbox && def.Read is { } read) tickbox.IsTicked = read();
            // Its own guard: a tip that cannot be built never leaves the other rows with the cloned line's text.
            if (row.Tip is { } tip && GodotObject.IsInstanceValid(tip))
                UiGuard.Run("settings hover tip", () => hoverTip(tip) = Tip(def));
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
