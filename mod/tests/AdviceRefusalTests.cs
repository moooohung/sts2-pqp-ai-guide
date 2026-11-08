using System.Text.Json;
using HumanPlayRecorder.Ai;

static class AdviceRefusalTests
{
    static void Require(bool value, string reason) { if (!value) throw new Exception(reason); }

    public static void Run()
    {
        using var strings = JsonDocument.Parse(typeof(AdviceRefusalTests).Assembly
            .GetManifestResourceStream("HumanPlayRecorder.ui_strings.json")!);
        var languages = strings.RootElement.EnumerateObject().ToArray();
        Require(languages.Length == 16, "ui_strings.json must carry 16 languages");

        var categories = Enum.GetValues<AdviceReason>();
        string[] keys = [.. categories.Select(AdviceRefusal.Key), AdviceRefusal.BadgeKey];
        Require(keys.Distinct().Count() == keys.Length, "every category needs its own key, distinct from the badge key");
        foreach (var language in languages)
            foreach (string key in keys)
                Require(language.Value.TryGetProperty(key, out var text) && !string.IsNullOrWhiteSpace(text.GetString()),
                    language.Name + " lacks the unavailable-badge string " + key);

        // The tooltip is the localized category text and carries nothing of the technical detail.
        string[] details = ["추천 불가: LiveModelData.CARD_X.energy: boom", "추천 불가: 화면 캡처가 작업 예산(4ms)을 초과했습니다.",
            "추천 불가: IRONCLAD_STRIKE.Cost: Object reference not set"];
        foreach (var language in languages)
        {
            string Get(string key) => language.Value.GetProperty(key).GetString()!;
            foreach (var category in categories)
                foreach (string detail in details)
                {
                    string tooltip = new AdviceRefusal(category, detail).Tooltip(Get);
                    Require(tooltip == Get(AdviceRefusal.Key(category)) && !tooltip.Contains(detail) &&
                        !tooltip.Contains("CARD_X") && !tooltip.Contains("4ms") && !tooltip.Contains("boom"),
                        language.Name + "/" + category + ": the tooltip must be the category text, not the detail");
                }
        }

        // The exception keeps the detail as its message (the log) and the category beside it.
        var error = new AdviceUnavailableException(AdviceReason.ShopSyncing, details[0]);
        Require(error.Message == details[0] && error.Reason == AdviceReason.ShopSyncing, "the exception must carry detail and category");
        Require(AdviceUnavailableException.ReasonOf(error) == AdviceReason.ShopSyncing &&
            AdviceUnavailableException.ReasonOf(new InvalidOperationException("x")) == AdviceReason.Other &&
            AdviceUnavailableException.ReasonOf(new InvalidOperationException("x"), AdviceReason.StateNotExposed) == AdviceReason.StateNotExposed,
            "a foreign exception falls back to its given category");
        Console.WriteLine("PASS: unavailable-badge categories map to keys present in 16 languages; tooltip never shows the detail");
    }
}
