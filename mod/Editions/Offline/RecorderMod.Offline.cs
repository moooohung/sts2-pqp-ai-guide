using MegaCrit.Sts2.Core.Multiplayer.Game.Lobby;
using MegaCrit.Sts2.Core.Runs;

namespace HumanPlayRecorder;

// Offline edition: runs are never recorded, so every run start stops after the advisor
// ("run start skipped: offline edition") and the policy comes from the mod folder.
public static partial class RecorderMod
{
    static string PolicyDirectory => Edition.PolicyDirectory;
    static string? RecordingRejection(RunState state, string? runStartToken) => "offline edition";
    static LocalJournal NewJournal(string id, bool persist) => new();
    static void StartMultiplayer(RunState state, StartRunLobby lobby) => StartCore(state, null);
}
