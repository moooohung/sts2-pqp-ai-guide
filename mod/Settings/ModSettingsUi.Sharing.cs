using Godot;

namespace HumanPlayRecorder.Settings;

// Online edition: what this PC shares, data deletion and the policy links.
static partial class ModSettingsUi
{
    static partial void InstallSharing() => SharingSettings.Changed += Refresh;

    static partial void AddSharingRows(List<RowDef> defs, OptionRow Option)
    {
        defs.Add(Option(RowName + "TrainingConsent", () => UiText.Get("settings_training_consent"),
            () => SharingSettings.TrainingConsent, value => SharingSettings.TrainingConsent = value,
            () => true, description: () => UiText.Get("settings_training_consent_tip")));
        defs.Add(Option(RowName + "ReplaySaving", () => UiText.Get("settings_replay_saving"),
            () => SharingSettings.ReplaySaving, value => SharingSettings.ReplaySaving = value,
            () => true, description: () => UiText.Get("settings_replay_saving_tip")));
        // Action rows: the tickbox is hidden and a click on the label runs the action once.
        bool deleteArmed = false;
        defs.Add(Option(RowName + "DeleteAll", () => UiText.Get(deleteArmed ? "settings_delete_all_confirm" : "settings_delete_all"),
            () => false, clicked =>
            {
                if (!deleteArmed) { deleteArmed = true; Refresh(); return; }
                deleteArmed = false;
                _ = Task.Run(async () =>
                {
                    bool done;
                    try { done = await ReplayLinks.DeleteAllMine(); }
                    catch (Exception) { done = false; }
                    if (!done)
                        Callable.From(() => FirstLaunchNotice.ShowShortNote(UiText.Get("settings_delete_all"),
                            UiText.Get("replay_delete_failed"))).CallDeferred();
                });
                Refresh();
            }, () => true, disclosure: true, description: () => ""));
        // This PC only: the local storage window, separate from the server-wide delete above.
        defs.Add(Option(RowName + "LocalStorage", () => LocalStorageText.SettingsRow, () => false, _ =>
        {
            LocalStorageView.ShowMenu();
            Refresh();
        }, () => true, disclosure: true, description: () => LocalStorageText.SettingsRowTip));
        foreach (var (suffix, key, url) in new[] {
            ("Privacy", "settings_privacy_link", "https://pqp.me/privacy/"),
            ("Terms", "settings_terms_link", "https://pqp.me/terms/") })
            defs.Add(Option(RowName + suffix, () => UiText.Get(key), () => false, _ =>
            {
                ReplayLinks.Open(url);
                Refresh();
            }, () => true, disclosure: true, description: () => url));
    }
}
