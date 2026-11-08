using HumanPlayRecorder.Ai;

namespace HumanPlayRecorder.Settings;

internal static class AdvisorStatusText
{
    internal static string Render(bool enabled, AdvisorStatus status, AiFailureKind failure,
        string? refusal, string? checkpoint, Func<string, string> text, Func<string, string, string> format)
    {
        if (!enabled || status == AdvisorStatus.Disabled) return text("ai_status_disabled");
        return status switch
        {
            AdvisorStatus.Ready => format("ai_status_loaded", checkpoint ?? ""),
            AdvisorStatus.Downloading => text("ai_status_updating"),
            AdvisorStatus.Unavailable when failure == AiFailureKind.None && refusal is null => "",
            _ => text(failure == AiFailureKind.Offline
                ? "ai_status_unavailable_offline" : "ai_status_unavailable")
        };
    }
}
