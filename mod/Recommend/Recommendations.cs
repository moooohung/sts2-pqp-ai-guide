using Godot;
using HarmonyLib;
using HumanPlayRecorder.Settings;

namespace HumanPlayRecorder.Recommend;

/// <summary>In-game AI recommendation badges and their settings rows.</summary>
public static class Recommendations
{
    /// <summary>Call once on the game's main thread from RecorderMod.Init, with the recorder's Harmony.</summary>
    public static void Install(Harmony harmony)
    {
        // The recorder keeps working when the advice UI cannot be installed, and the badges
        // keep working when the settings rows cannot (and the other way round).
        UiGuard.Run("settings install", () => ModSettingsUi.Install(harmony));
        UiGuard.Run("badge install", () =>
        {
            BadgeHooks.Install(harmony);
            Badges.Subscribe();
        });
#if DEBUG
        UiGuard.Run("preview install", () =>
        {
            if (!Advice.Preview) return;
            GD.Print("[HumanPlayRecorder] recommendation badges: preview probabilities (HPR_AI_BADGE_PREVIEW=1, Debug build)");
            Badges.HookFrame(); // the preview banner shows before the first option node
        });
#endif
    }
}
