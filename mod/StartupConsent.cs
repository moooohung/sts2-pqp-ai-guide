using System.Text.Json;
using Godot;
using HumanPlayRecorder.Settings;

namespace HumanPlayRecorder;

public enum StartupConsentChoice
{
    OriginalRulesEnabled,
    // "이번에는 기록 안 함": do not record runs in this game session.
    SkipThisSession,
    // "다시 묻지 않기": remembered; recording proceeds and the per-run gate decides.
    NeverAsk
}

public sealed class StartupConsentState
{
    public bool never_ask { get; set; }
}

// Consent changes our preference only, never the game's enabled-mod list.
public static class StartupConsent
{
    static readonly RememberedContentMods ContentEvidence = new(Path.Combine(
        System.Environment.GetFolderPath(System.Environment.SpecialFolder.LocalApplicationData),
        "HumanPlayRecorder", "excluded-content-mods.json"));

    internal static void RememberContentMod(string id)
    {
        try { ContentEvidence.Remember(id); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or JsonException)
        {
            GD.Print("[HumanPlayRecorder] content evidence not saved: " + e.GetType().Name);
        }
    }

    internal static string[] IncludeRememberedMods(string[] scanned, IEnumerable<string> loaded)
    {
        try { return ContentEvidence.Offer(scanned, loaded); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or JsonException)
        {
            GD.Print("[HumanPlayRecorder] content evidence unreadable: " + e.GetType().Name);
            return scanned;
        }
    }

    static readonly string StatePath = Path.Combine(
        System.Environment.GetFolderPath(System.Environment.SpecialFolder.LocalApplicationData),
        "HumanPlayRecorder", "startup-consent.json");
    const float Width = 720; // room for three native buttons inside the panel border
    static Action<StartupConsentChoice>? enabledCallback;
    static int enabled, ran;

    public static bool RecordingDeclinedThisSession { get; private set; }

    // Call once from the mod initializer; the check then runs on the first main menu.
    public static void Enable(Action<StartupConsentChoice>? onChoice = null)
    {
        enabledCallback = onChoice;
        Volatile.Write(ref enabled, 1);
    }

    internal static void OnMainMenuReady()
    {
        if (Volatile.Read(ref enabled) == 0 || Interlocked.Exchange(ref ran, 1) != 0) return;
        var state = LoadState();
        if (state.never_ask || OriginalRulesSettings.Enabled) return;
        IReadOnlyList<StartupOffendingMod> offending;
        try { offending = StartupContentScan.OffendingModsAtStartup(); }
        catch (Exception e)
        {
            GD.Print("[HumanPlayRecorder] start-up content scan unavailable: " + e.GetType().Name);
            return;
        }
        GD.Print("[HumanPlayRecorder] start-up content check: " +
            (offending.Count == 0 ? "no non-vanilla base-run content" : string.Join(",", offending.Select(m => m.Id))));
        if (ShouldOffer(state, OriginalRulesSettings.Enabled, offending.Count))
            ShowStartupConsent(offending, enabledCallback);
    }

    public static bool ShouldOffer(StartupConsentState state, bool rulesEnabled, int offendingCount) =>
        !state.never_ask && !rulesEnabled && offendingCount > 0;

    public static void Choose(StartupConsentChoice choice, Func<bool> enableRules,
        Action skipRecording, Action neverAsk, Action<StartupConsentChoice>? onChoice = null)
    {
        switch (choice)
        {
            case StartupConsentChoice.OriginalRulesEnabled:
                if (!enableRules()) return;
                break;
            case StartupConsentChoice.SkipThisSession: skipRecording(); break;
            case StartupConsentChoice.NeverAsk: neverAsk(); break;
            default: throw new ArgumentOutOfRangeException(nameof(choice));
        }
        onChoice?.Invoke(choice);
    }

    // Main thread only (for example from a NMainMenu._Ready postfix).
    public static void ShowStartupConsent(IReadOnlyList<StartupOffendingMod> mods,
        Action<StartupConsentChoice>? onChoice)
    {
        void Apply(StartupConsentChoice choice) => UiGuard.Run("original rules consent", () => Choose(choice,
            () => { OriginalRulesSettings.Enabled = true; return OriginalRulesSettings.Enabled; },
            () => RecordingDeclinedThisSession = true,
            () => { var state = LoadState(); state.never_ask = true; SaveState(state); }, onChoice));
        GamePopup.Enqueue(UiText.Get("rules_consent_title"),
            UiText.Format("rules_consent_body", List(mods.Select(m => m.Name))) +
                "\n\n" + UiText.Get("settings_original_rules_description"),
            Width,
            new(UiText.Get("rules_consent_enable"), () => Apply(StartupConsentChoice.OriginalRulesEnabled)),
            new(UiText.Get("rules_consent_skip"), () => Apply(StartupConsentChoice.SkipThisSession)),
            new(UiText.Get("rules_consent_never"), () => Apply(StartupConsentChoice.NeverAsk)),
            "startup-consent");
    }

    static string List(IEnumerable<string> names) => string.Join("\n", names.Select(n => "• " + n));

    internal static StartupConsentState LoadState()
    {
        try
        {
            if (File.Exists(StatePath))
                return JsonSerializer.Deserialize<StartupConsentState>(File.ReadAllBytes(StatePath)) ?? new();
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or JsonException)
        {
            GD.Print("[HumanPlayRecorder] start-up consent state unreadable: " + e.GetType().Name);
        }
        return new();
    }

    static void SaveState(StartupConsentState state)
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(StatePath)!);
            string temp = StatePath + ".tmp";
            File.WriteAllBytes(temp, JsonSerializer.SerializeToUtf8Bytes(state));
            File.Move(temp, StatePath, overwrite: true);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            GD.Print("[HumanPlayRecorder] start-up consent state not saved: " + e.GetType().Name);
        }
    }

}
