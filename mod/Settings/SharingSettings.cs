using System.Text.Json;

namespace HumanPlayRecorder.Settings;

// What this PC may send: training consent (asked once; until answered it is "no") and replay
// saving (default on). A run's recording leaves the PC only when at least one is on
// (UploadPolicy); training consent alone decides whether the server may train on it.
public static class SharingSettings
{
    sealed record Saved(bool asked = false, bool training = false, bool replay_saving = true);
    static readonly string PathName = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
        "HumanPlayRecorder", "training-consent.json");
    static Saved? saved;
    static Saved State => saved ??= Load();
    public static event Action? Changed;

    public static bool TrainingAsked => State.asked;
    public static bool TrainingConsent
    {
        get => State.asked && State.training;
        set => Save(State with { asked = true, training = value });
    }
    public static bool ReplaySaving
    {
        get => State.replay_saving;
        set => Save(State with { replay_saving = value });
    }

    static void Save(Saved next)
    {
        if (next == State) return;
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(PathName)!);
            File.WriteAllBytes(PathName + ".tmp", JsonSerializer.SerializeToUtf8Bytes(next));
            File.Move(PathName + ".tmp", PathName, true);
            saved = next;
            Changed?.Invoke();
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        { UiGuard.Report("sharing settings", e); }
    }

    static Saved Load()
    {
        try { return File.Exists(PathName) ? JsonSerializer.Deserialize<Saved>(File.ReadAllBytes(PathName)) ?? new() : new(); }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or JsonException)
        { UiGuard.Report("sharing settings", e); return new(); }
    }

    // Main thread, first main menu: every player answers once, new or existing.
    internal static void OfferTrainingConsent()
    {
        if (TrainingAsked && !DebugUiCapture.Active) return;
        var text = ConsentPopup.Text(UiText.Get);
        GamePopup.EnqueueContent(text.Title, 900, area => ConsentPopup.Fill(area, text.Body),
            new(text.Agree, () => UiGuard.Run("training consent", () => TrainingConsent = true)),
            new(text.Decline, () => UiGuard.Run("training consent", () => TrainingConsent = false)),
            debugName: "training-consent");
    }
}
