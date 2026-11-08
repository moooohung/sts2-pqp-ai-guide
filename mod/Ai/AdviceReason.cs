namespace HumanPlayRecorder.Ai;

/// <summary>
/// The player-facing reason an advice badge is unavailable. The badge tooltip shows only the localized
/// text of the category; the technical detail (model ids, field names, exception text, budgets) goes to
/// the log and never to the player.
/// </summary>
public enum AdviceReason
{
    /// <summary>Anything without a more specific category, including internal refusal codes.</summary>
    Other,
    /// <summary>The screen's data is still being loaded or filled by the game (an event, the room).</summary>
    ScreenLoading,
    /// <summary>The game did not expose the state or value this choice depends on.</summary>
    StateNotExposed,
    /// <summary>A screen or choice the advisor's input contract does not cover.</summary>
    UnsupportedScreen,
    /// <summary>A card preview or X-cost value that cannot be computed safely.</summary>
    PreviewUnavailable,
    /// <summary>Reading the screen took longer than the per-frame work budget.</summary>
    BudgetExceeded,
    /// <summary>The shop list of this player is not synchronized yet.</summary>
    ShopSyncing,
}

/// <summary>One refusal: the category the player sees and the detail that is only logged.</summary>
public sealed record AdviceRefusal(AdviceReason Reason, string Detail)
{
    /// <summary>ui_strings.json key of the badge text.</summary>
    public const string BadgeKey = "ai_badge_unavailable";

    /// <summary>ui_strings.json key of the tooltip text of a category.</summary>
    public static string Key(AdviceReason reason) => reason switch
    {
        AdviceReason.Other => "ai_unavailable_other",
        AdviceReason.ScreenLoading => "ai_unavailable_screen_loading",
        AdviceReason.StateNotExposed => "ai_unavailable_state_not_exposed",
        AdviceReason.UnsupportedScreen => "ai_unavailable_unsupported_screen",
        AdviceReason.PreviewUnavailable => "ai_unavailable_preview",
        AdviceReason.BudgetExceeded => "ai_unavailable_budget",
        AdviceReason.ShopSyncing => "ai_unavailable_shop_syncing",
        _ => throw new ArgumentOutOfRangeException(nameof(reason)),
    };

    /// <summary>The tooltip: the localized category text, never the detail.</summary>
    public string Tooltip(Func<string, string> get) => get(Key(Reason));
}

/// <summary>Advice cannot be given for this screen. Message is the log detail; Reason is what the player sees.</summary>
public sealed class AdviceUnavailableException(AdviceReason reason, string detail) : Exception(detail)
{
    public AdviceReason Reason { get; } = reason;

    /// <summary>The category of an exception that reached a refusal; any other exception is a fallback.</summary>
    public static AdviceReason ReasonOf(Exception error, AdviceReason fallback = AdviceReason.Other) =>
        error is AdviceUnavailableException unavailable ? unavailable.Reason : fallback;
}
