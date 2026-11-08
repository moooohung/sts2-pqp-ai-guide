using System.Text.Json;

namespace HumanPlayRecorder.Settings;

public static class OriginalRulesSettings
{
    sealed record Saved(bool enabled);
    static readonly string PathName = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "HumanPlayRecorder", "original-rules.json");
    static bool? enabled;
    public static event Action? Changed;
    public static bool Enabled
    {
        get => enabled ??= Load();
        set
        {
            if (Enabled == value) return;
            try
            {
                Directory.CreateDirectory(Path.GetDirectoryName(PathName)!);
                File.WriteAllBytes(PathName + ".tmp", JsonSerializer.SerializeToUtf8Bytes(new Saved(value)));
                File.Move(PathName + ".tmp", PathName, true);
                enabled = value;
                Changed?.Invoke();
            }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException)
            { UiGuard.Report("original rules setting", e); }
        }
    }
    static bool Load()
    {
        try { return File.Exists(PathName) && JsonSerializer.Deserialize<Saved>(File.ReadAllBytes(PathName))?.enabled == true; }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or JsonException)
        { UiGuard.Report("original rules setting", e); return false; }
    }
    internal static string NoticeText(string key)
    {
        string value = UiText.Get(key);
        return value != key ? value : UiText.Get("original_rules_standard_only");
    }
}
