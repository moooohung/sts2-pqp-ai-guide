namespace HumanPlayRecorder;

public sealed record GameplayPatchEvidence(string Type, string Method, bool IsModel = false, bool IsVisualProperty = false);

// Namespace/member evidence, not a list of mod names. A presentation-only
// method on a model can be cosmetic; other model methods change gameplay.
public static class GameplayPatchClassifier
{
    const string Core = "MegaCrit.Sts2.Core.";
    static readonly string[] GameplayNamespaces = [
        "Combat", "Commands", "Models", "Hooks", "Rooms", "Events", "Rewards",
        "Map", "Random", "Runs", "Saves", "Entities", "Factories", "CardSelection",
        "GameActions", "MonsterMoves", "Odds", "Multiplayer.Game", "Unlocks", "ValueProps"
    ];
    static readonly HashSet<string> VisualStringGetters = [
        "get_PortraitPath", "get_BetaPortraitPath", "get_PortraitPngPath",
        "get_FramePath", "get_IconPath", "get_PackedIconPath", "get_ResolvedBigIconPath",
        "get_BigIconPath", "get_ImagePath", "get_LargeImagePath", "get_VisualsPath",
        "get_EnergyIconPath", "get_CharacterSelectBg", "get_CharacterSelectIconPath",
        "get_CharacterSelectLockedIconPath", "get_MapMarkerPath", "get_ArmPointingTexturePath",
        "get_ArmRockTexturePath", "get_ArmPaperTexturePath", "get_ArmScissorsTexturePath"
    ];
    public static bool IsGameplay(GameplayPatchEvidence method)
    {
        if (method.Type == Core + "Commands.SfxCmd" || method.Type == Core + "Commands.VfxCmd") return false;
        if (method.IsModel && (method.IsVisualProperty || VisualStringGetters.Contains(method.Method))) return false;
        if (method.IsModel) return true;
        if (method.Type == "System.Random" || method.Type.StartsWith("System.Random+", StringComparison.Ordinal)) return true;
        return GameplayNamespaces.Any(ns => method.Type.StartsWith(Core + ns + ".", StringComparison.Ordinal) ||
            method.Type.StartsWith(Core + ns + "+", StringComparison.Ordinal) || method.Type == Core + ns);
    }
}
