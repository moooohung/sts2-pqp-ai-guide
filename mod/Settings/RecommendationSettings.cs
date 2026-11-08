using System.Text.Json;
using Godot;

namespace HumanPlayRecorder.Settings;

public enum RecommendationCategory { CardRewards, Combat, Potions, Map, Events, RestSite, Merchant, Relics }

/// <summary>The player's "show AI recommendations" preference, persisted across restarts.</summary>
public static class RecommendationSettings
{
    sealed record Saved(bool? show_recommendations,
        Dictionary<RecommendationCategory, bool>? categories = null, bool top_pick_only = false);

    static readonly string FilePath = Path.Combine(Edition.DataDirectory, "ui-settings.json");
    static bool? show;
    static Saved? saved;
    static Saved Preferences => saved ??= Load();

    /// <summary>Raised on the main thread after <see cref="ShowRecommendations"/> changed.</summary>
    public static event Action? ShowRecommendationsChanged;
    public static event Action? BadgeVisibilityChanged;

    /// <summary>Whether recommendation badges are shown. Default on; setting it saves it.</summary>
    public static bool ShowRecommendations
    {
        get => show ??= Preferences.show_recommendations ?? true;
        set
        {
            if (ShowRecommendations == value) return;
            show = value;
            Save();
            ShowRecommendationsChanged?.Invoke();
        }
    }

    public static bool IsEnabled(RecommendationCategory category) =>
        Preferences.categories?.GetValueOrDefault(category, true) ?? true;

    public static void SetEnabled(RecommendationCategory category, bool enabled)
    {
        if (IsEnabled(category) == enabled) return;
        var categories = new Dictionary<RecommendationCategory, bool>(Preferences.categories ?? new());
        categories[category] = enabled;
        saved = Preferences with { categories = categories };
        Save();
        BadgeVisibilityChanged?.Invoke();
    }

    public static bool TopPickOnly
    {
        get => Preferences.top_pick_only;
        set
        {
            if (TopPickOnly == value) return;
            saved = Preferences with { top_pick_only = value };
            Save();
            BadgeVisibilityChanged?.Invoke();
        }
    }

    public static bool AllowsBadge(RecommendationCategory category, bool top) =>
        ShowRecommendations && IsEnabled(category) && (!TopPickOnly || top);

    static Saved Load()
    {
        try
        {
            if (File.Exists(FilePath))
                return JsonSerializer.Deserialize<Saved>(File.ReadAllBytes(FilePath)) ?? new(null);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or JsonException)
        {
            GD.Print("[HumanPlayRecorder] ui settings unreadable: " + e.GetType().Name);
        }
        return new(null);
    }

    static void Save()
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(FilePath)!);
            string temp = FilePath + ".tmp";
            File.WriteAllBytes(temp, JsonSerializer.SerializeToUtf8Bytes(Preferences with { show_recommendations = ShowRecommendations }));
            File.Move(temp, FilePath, overwrite: true);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            GD.Print("[HumanPlayRecorder] ui settings not saved: " + e.GetType().Name);
        }
    }
}
