using MegaCrit.Sts2.Core.Modding;

namespace HumanPlayRecorder;

// Offline edition: its own mod id, assembly, Harmony id and data folder, so it can be installed
// next to the online mod. Both would patch the same game methods, so while the online mod is
// enabled this edition does not start at all (StandDownReason).
static partial class Edition
{
    internal const string ModId = "PqpAiGuideOffline";
    internal const string HarmonyId = "me.pqp.aiguide.offline";
    internal const string ModDataHarmonyId = "me.pqp.aiguide.offline.mod-data";
    internal const string DataFolderName = "PqpAiGuideOffline";
    internal static readonly string? TextVariant = "offline_";
    internal static readonly Type[] ModMessageTypes = [typeof(AiShopPurchaseMessage)];
    internal const string OnlineModId = "HumanPlayRecorder";
    // Folder beside the DLL that holds current.json and its weights.
    internal const string PolicyFolderName = "policy";

    static partial void CheckStandDown(ref string? reason) =>
        reason = StandDownReason(ModManager.Mods.Select(m => (m.manifest?.id, m.state)));

    // Loaded: the online mod initialized first. None: it is enabled and loads after this one
    // (disabled mods are already marked Disabled before any mod initializer runs).
    internal static string? StandDownReason(IEnumerable<(string? Id, ModLoadState State)> mods) =>
        mods.Any(m => m.Id == OnlineModId && m.State is ModLoadState.Loaded or ModLoadState.None)
            ? $"the online edition ({OnlineModId}) is enabled; disable one of the two editions"
            : null;

    internal static string PolicyDirectory => Path.Combine(
        Path.GetDirectoryName(typeof(Edition).Assembly.Location)!, PolicyFolderName);
}
