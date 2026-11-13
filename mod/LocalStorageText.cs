namespace HumanPlayRecorder;

// Local storage window, storage-full note and the short-note close button; backed by
// Localization/ui_strings.json (16 languages) via Settings.UiText. Format strings take {0}/{1} as noted.
static class LocalStorageText
{
    public static string NoteClose => Settings.UiText.Get("note_close");
    public static string CapacityBody => Settings.UiText.Get("storage_capacity_body");
    public static string CapacityManage => Settings.UiText.Get("storage_capacity_manage");
    public static string Title => Settings.UiText.Get("local_storage_title");
    public static string Intro => Settings.UiText.Get("local_storage_intro");
    public static string Loading => Settings.UiText.Get("local_storage_loading");
    public static string LoadFailed => Settings.UiText.Get("local_storage_load_failed");
    public static string Empty => Settings.UiText.Get("local_storage_empty");
    public static string Summary => Settings.UiText.Get("local_storage_summary"); // {0} runs, {1} MB
    public static string StatePending => Settings.UiText.Get("local_storage_state_pending");
    public static string StateUncertified => Settings.UiText.Get("local_storage_state_uncertified");
    public static string StateOversize => Settings.UiText.Get("local_storage_state_oversize");
    public static string StateRetained => Settings.UiText.Get("local_storage_state_retained");
    public static string RowSize => Settings.UiText.Get("local_storage_row_size"); // {0} MB
    public static string RowAscension => Settings.UiText.Get("local_storage_row_ascension"); // {0} level
    public static string RowParty => Settings.UiText.Get("local_storage_row_party"); // {0} players
    public static string Delete => Settings.UiText.Get("local_storage_delete");
    public static string DeleteConfirm => Settings.UiText.Get("local_storage_delete_confirm");
    public static string DeleteFailed => Settings.UiText.Get("local_storage_delete_failed");
    public static string Clear => Settings.UiText.Get("local_storage_clear");
    public static string ClearTitle => Settings.UiText.Get("local_storage_clear_title");
    public static string ClearBody => Settings.UiText.Get("local_storage_clear_body");
    public static string ClearConfirm => Settings.UiText.Get("local_storage_clear_confirm");
    public static string Cancel => Settings.UiText.Get("local_storage_cancel");
    public static string SettingsRow => Settings.UiText.Get("settings_local_storage");
    public static string SettingsRowTip => Settings.UiText.Get("settings_local_storage_tip");
}
