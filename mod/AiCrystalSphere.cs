using HarmonyLib;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.Events.Custom.CrystalSphereEvent;
using MegaCrit.Sts2.Core.GameActions;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Multiplayer;
using MegaCrit.Sts2.Core.Multiplayer.Game;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Game;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Game.Sync;
using MegaCrit.Sts2.Core.Multiplayer.Serialization;
using MegaCrit.Sts2.Core.Multiplayer.Transport;
using MegaCrit.Sts2.Core.Rewards;
using MegaCrit.Sts2.Core.Runs;

namespace HumanPlayRecorder;

public static partial class RecorderMod
{
    static readonly AsyncLocal<Player?> aiCrystalOwner = new();
    static readonly Dictionary<Player, Task> aiCrystalRewards = new();
    static void InstallAiCrystalSphere(Harmony harmony)
    {
        harmony.Patch(AccessTools.Method(typeof(CrystalSphereMinigame), nameof(CrystalSphereMinigame.PlayMinigame)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(AiCrystalPlay)));
        harmony.Patch(AccessTools.Method(typeof(OneOffSynchronizer), "OfferCrystalSphereRewards"),
            postfix: new HarmonyMethod(typeof(RecorderMod), nameof(AiCrystalOffered)));
        harmony.Patch(AccessTools.Method(typeof(RewardSynchronizer), nameof(RewardSynchronizer.SyncLocalObtainedCard)),
            prefix: new HarmonyMethod(typeof(RecorderMod), nameof(AiCrystalCurse)));
    }
    static bool AiCrystalPlay(CrystalSphereMinigame __instance, Player ____owner, ref Task __result)
    {
        if (!AiOwned(____owner)) return true;
        __result = AiCrystalRun(__instance, ____owner);
        return false;
    }
    internal static async Task<List<CrystalSphereItem>> AiCrystalBoard(CrystalSphereMinigame game)
    {
        // Blind row-major big-tool clicks: no hidden-item inspection or RNG use.
        game.SetTool(CrystalSphereMinigame.CrystalSphereToolType.Big);
        while (!game.IsFinished) {
            var cell = game.cells.Cast<CrystalSphereCell>().FirstOrDefault(cell => cell.IsHidden);
            if (cell is null) break;
            await game.CellClicked(cell);
        }
        return ((List<CrystalSphereItem>)AccessTools.Field(typeof(CrystalSphereMinigame), "_revealed")
            .GetValue(game)!).ToList();
    }
    static async Task AiCrystalRun(CrystalSphereMinigame game, Player player)
    {
        var previous = aiCrystalOwner.Value;
        aiCrystalOwner.Value = player;
        try {
            var revealed = await AiCrystalBoard(game);
            AiSubmit(player, new CrystalSphereRewardsMessage {
                Location = RunManager.Instance.RunLocationTargetedBuffer.CurrentLocation,
                rewards = revealed.Select(item => item.ToSerializable()).ToList()
            });
            if (aiCrystalRewards.Remove(player, out var task)) await task;
            else throw new InvalidOperationException("AI crystal rewards did not enter the original synchronizer");
        }
        finally { aiCrystalOwner.Value = previous; }
    }
    static void AiCrystalOffered(Player owner, Task __result)
    {
        if (AiOwned(owner)) aiCrystalRewards[owner] = __result;
    }
    static bool AiCrystalCurse(CardModel card)
    {
        if (aiCrystalOwner.Value is not { } player) return true;
        // RevealItem already obtained the curse on the host. Forward only, as
        // the real local board does, without applying it twice on that host.
        var message = new RewardObtainedMessage {
            rewardType = MegaCrit.Sts2.Core.Rewards.RewardType.Card, cardModel = card, wasSkipped = false,
            location = RunManager.Instance.RunLocationTargetedBuffer.CurrentLocation
        };
        AccessTools.Method(typeof(NetHostGameService), "BroadcastMessage").MakeGenericMethod(typeof(INetMessage))
            .Invoke(RunManager.Instance.NetService, [message, player.NetId, message.Mode.ToChannelId(), player.NetId]);
        return false;
    }
}
