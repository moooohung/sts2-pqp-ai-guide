namespace HumanPlayRecorder;

static partial class Edition
{
    internal const string ModId = "HumanPlayRecorder";
    internal const string HarmonyId = "me.pqp.humanplayrecorder";
    internal const string ModDataHarmonyId = "me.pqp.advisor.mod-data";
    internal const string DataFolderName = "HumanPlayRecorder";
    // UiText looks up "<variant><key>" first; the online edition uses the shared keys as they are.
    internal static readonly string? TextVariant = null;
    internal static readonly Type[] ModMessageTypes =
        [typeof(AiShopPurchaseMessage), typeof(HumanPlayConsentMessage), typeof(HumanPlayRunNonceMessage)];
}
