namespace HumanPlayRecorder;

// Identity of the edition being built. Editions/Online and Editions/Offline each supply the
// constants; Editions.props compiles exactly one of them (-p:Edition=Offline).
static partial class Edition
{
    // The game advertises each loaded mod to peers as "<id>-<version>".
    internal const string NetworkModName = ModId + "-0.1.0";
    internal static string DataDirectory => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), DataFolderName);

    // Null when this edition may start; otherwise why it stays inactive.
    internal static string? StandDownReason()
    {
        string? reason = null;
        CheckStandDown(ref reason);
        return reason;
    }
    static partial void CheckStandDown(ref string? reason);
}
