using System.Globalization;
using System.Text.Json;
using Godot;
using MegaCrit.Sts2.Core.Localization;

namespace HumanPlayRecorder.Settings;

/// <summary>
/// The mod's own UI strings (Localization/ui_strings.json, embedded in the DLL as
/// language code → key → text) in the game's current language, falling back to eng.
/// </summary>
public static class UiText
{
    const string Fallback = "eng";
    static readonly Dictionary<string, Dictionary<string, string>> tables = Load();
    static bool subscribed;

    /// <summary>Raised on the main thread after the player changed the game's language.</summary>
    public static event Action? LanguageChanged;

    public static string Language => LocManager.Instance?.Language ?? Fallback;
    public static CultureInfo Culture => LocManager.Instance?.CultureInfo ?? CultureInfo.InvariantCulture;

    public static string Get(string key)
    {
        Subscribe();
        // An edition's own wording ("offline_<key>") replaces the shared text where it exists.
        if (Edition.TextVariant is { } variant && Lookup(variant + key) is { } edition) return edition;
        return Lookup(key) ?? key;
    }

    static string? Lookup(string key)
    {
        if (tables.TryGetValue(Language, out var table) && table.TryGetValue(key, out var text)) return text;
        if (tables.TryGetValue(Fallback, out table) && table.TryGetValue(key, out text)) return text;
        return null;
    }

    public static string Format(string key, string value) => Get(key).Replace("{0}", value);

    internal static void Subscribe()
    {
        // Original-DLL verification has localization but no Godot native host.
        if (MegaCrit.Sts2.Core.TestSupport.TestMode.IsOn) return;
        if (subscribed || LocManager.Instance is null) return;
        subscribed = true;
        // Deferred: LocManager iterates its callback list while it notifies, and the
        // first lookup can happen inside that notification (the main menu reloads then).
        Callable.From(() => UiGuard.Run("language subscription", () =>
            LocManager.Instance.SubscribeToLocaleChange(() => UiGuard.Run("language change", () => LanguageChanged?.Invoke()))))
            .CallDeferred();
    }

    // A static initializer failure would break every later lookup, so any error yields no tables.
    static Dictionary<string, Dictionary<string, string>> Load() => UiGuard.Run("ui strings", () =>
    {
        using var stream = typeof(UiText).Assembly.GetManifestResourceStream("HumanPlayRecorder.ui_strings.json")
            ?? throw new FileNotFoundException("ui_strings.json is not embedded in the DLL");
        return JsonSerializer.Deserialize<Dictionary<string, Dictionary<string, string>>>(stream) ?? new();
    }, new Dictionary<string, Dictionary<string, string>>());
}
