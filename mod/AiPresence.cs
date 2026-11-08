using System.Diagnostics;
using Godot;
using HarmonyLib;
using HumanPlayRecorder.Ai;
using MegaCrit.Sts2.Core.Combat;
using MegaCrit.Sts2.Core.Entities.Creatures;
using MegaCrit.Sts2.Core.Entities.Multiplayer;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.Map;
using MegaCrit.Sts2.Core.Multiplayer;
using MegaCrit.Sts2.Core.Multiplayer.Game;
using MegaCrit.Sts2.Core.Multiplayer.Game.PeerInput;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Game.Flavor;
using MegaCrit.Sts2.Core.Multiplayer.Serialization;
using MegaCrit.Sts2.Core.Multiplayer.Transport;
using MegaCrit.Sts2.Core.Nodes;
using MegaCrit.Sts2.Core.Nodes.Rooms;
using MegaCrit.Sts2.Core.Nodes.Screens.Map;
using MegaCrit.Sts2.Core.Rooms;
using MegaCrit.Sts2.Core.Runs;

namespace HumanPlayRecorder;

public static partial class RecorderMod
{
    static readonly Dictionary<Player, AiTeammatePresence> aiPresence = new();
    static readonly Dictionary<Player, (CombatRoom Room, long Due)> aiCelebrations = new();
    static AbstractRoom? aiPresenceRoom;

    static void InstallAiPresence() => CombatManager.Instance.CombatWon += AiCelebrate;

    static AiTeammatePresence AiPresenceFor(Player player)
    {
        if (!aiPresence.TryGetValue(player, out var presence))
            aiPresence[player] = presence = new(message => AiCosmetic(() =>
                AiSubmitPresence(RunManager.Instance.NetService, player.NetId, message)), Random.Shared.NextDouble);
        return presence;
    }

    internal static void AiSubmitPresence(INetGameService service, ulong sender, INetMessage message)
    {
        if (message is not RestSiteOptionHoveredMessage) {
            MultiplayerAi.Submit(service, sender, message);
            return;
        }
        if (service is not NetHostGameService host || !MultiplayerAi.IsAi(sender))
            throw new InvalidOperationException("AI presence requires a host-owned virtual peer");
        // The native rest receiver indexes its current options without a bounds
        // check. Put hover and clear on the choice's reliable ordered channel;
        // the native packet type, location and AI sender stay unchanged.
        var bus = (NetMessageBus)AccessTools.Field(typeof(NetHostGameService), "_messageBus").GetValue(host)!;
        byte[] bytes = bus.SerializeMessage(sender, message, out int length);
        foreach (var peer in host.ConnectedPeers.Where(peer => peer.readyForBroadcasting && !MultiplayerAi.IsAi(peer.peerId)))
            host.NetHost!.SendMessageToClient(peer.peerId, bytes, length,
                NetTransferMode.Reliable, NetTransferMode.Reliable.ToChannelId());
        bus.SendMessageToAllHandlers(message, sender);
    }

    static void AiCosmetic(Action show)
    {
        try { show(); }
        catch (Exception error) { AdvisorTrace("AI presence " + error.GetType().Name); }
    }

    static NetScreenType AiPresenceScreen(Player player, AiAdvisor.LiveOption? option = null)
    {
        if (option?.Action == AiAdvisor.DecisionAction.Move || NMapScreen.Instance is { IsOpen: true })
            return NetScreenType.Map;
        if (aiWaitingChoices.ContainsKey(player)) return NetScreenType.CardSelection;
        if (AiHasRewards(player)) return NetScreenType.Rewards;
        return NetScreenType.Room;
    }

    static Vector2? AiCreaturePosition(Creature? creature, Control? root = null)
    {
        root ??= NGame.Instance?.RemoteCursorContainer;
        if (NCombatRoom.Instance?.GetCreatureNode(creature)?.Hitbox is not { } hitbox ||
            root is null) return null;
        return NetCursorHelper.GetNormalizedPosition(hitbox.GlobalPosition + hitbox.Size * .5f, root);
    }

    static Vector2? AiDestination(Player player, AiAdvisor.LiveOption option)
    {
        if (option.Option is MapPoint point && NMapScreen.Instance is { } map) {
            var points = (Dictionary<MapCoord, NMapPoint>)AccessTools.Field(typeof(NMapScreen), "_mapPointDictionary").GetValue(map)!;
            return points.TryGetValue(point.coord, out var node)
                ? map.GetNetPositionFromScreenPosition(node.GlobalPosition + node.Size * .5f) : null;
        }
        return AiCreaturePosition(option.Target as Creature ?? player.Creature);
    }

    static bool AiThink(Player player, AiAdvisor.LiveOption chosen, IReadOnlyList<AiAdvisor.LiveOption> current,
        IReadOnlyList<(object Option, float Probability)> advice)
    {
        float Probability(AiAdvisor.LiveOption option) => advice.First(entry =>
            entry.Option is AiAdvisor.ActionOption joint && joint.Action == option.Action &&
            ReferenceEquals(joint.Source, option.Option) && ReferenceEquals(joint.Target, option.Target)).Probability;
        float confidence = Probability(chosen);
        var runnerUp = current.Where(option => !ReferenceEquals(option.Option, chosen.Option))
            .OrderByDescending(Probability).FirstOrDefault();
        // Geometry and network delivery are cosmetic boundaries: neither may
        // revoke an otherwise valid policy action.
        Vector2? origin = null, destination = null;
        AiCosmetic(() => {
            origin = chosen.Action == AiAdvisor.DecisionAction.Move ? null : AiCreaturePosition(player.Creature);
            destination = AiDestination(player, chosen);
        });
        var presence = AiPresenceFor(player);
        long now = Stopwatch.GetTimestamp();
        AiCosmetic(() => presence.Think(run!.CurrentRoom!, chosen, runnerUp, confidence,
            AiPresenceScreen(player, chosen), RunManager.Instance.RunLocationTargetedBuffer.CurrentLocation,
            origin, destination, now, Stopwatch.Frequency));
        // Think stores its deadline before emitting any cosmetic messages.
        return presence.Ready(now);
    }

    static void AiClearPresence(Player player) => AiCosmetic(() => {
        if (aiPresence.TryGetValue(player, out var presence))
            presence.Clear(AiPresenceScreen(player), RunManager.Instance.RunLocationTargetedBuffer.CurrentLocation,
                Stopwatch.GetTimestamp(), Stopwatch.Frequency);
    });

    static void AiSuspendPresence(Player player) => AiCosmetic(() => {
        if (aiPresence.TryGetValue(player, out var presence))
            presence.Hide(AiPresenceScreen(player), RunManager.Instance.RunLocationTargetedBuffer.CurrentLocation,
                Stopwatch.GetTimestamp(), Stopwatch.Frequency);
    });

    // A teammate who is done and kept waiting nudges the slow human the way a real peer
    // does: the end-turn ping in combat, pings on its own map vote elsewhere.
    internal sealed class AiNudge { public object? Wait; public long Due; public int Sent, Burst; }
    static readonly Dictionary<Player, AiNudge> aiNudges = new();
    const int AiNudgeLimit = 6;

    static (object? Wait, INetMessage? Ping) AiWaitingOnHumans(Player player)
    {
        var humans = run!.Players.Where(other => !AiOwned(other)).ToArray();
        if (humans.Length == 0) return (null, null);
        var combat = CombatManager.Instance;
        if (run.CurrentRoom is CombatRoom room && combat.IsInProgress) {
            // The native ping button exists only for a player who already ended the turn.
            if (combat.IsPlayerReadyToEndTurn(player) &&
                humans.Any(human => human.Creature.IsAlive && !combat.IsPlayerReadyToEndTurn(human)))
                return (("combat", room.CombatState, room.CombatState.RoundNumber), new EndTurnPingMessage());
            return (null, null);
        }
        var votes = RunManager.Instance.MapSelectionSynchronizer;
        if (votes.GetVote(player) is { } vote && humans.Any(human => !votes.GetVote(human).HasValue))
            return (("map", run.MapLocation, vote.coord), new MapPingMessage { coord = vote.coord });
        return (null, null);
    }

    static void AiNudgeFrame(long now)
    {
        foreach (var player in run!.Players.Where(AiOwned)) {
            var (wait, ping) = AiWaitingOnHumans(player);
            if (!aiNudges.TryGetValue(player, out var nudge)) aiNudges[player] = nudge = new();
            if (AiNudgeStep(nudge, ping is null ? null : wait, ping is EndTurnPingMessage, now,
                    Stopwatch.Frequency, Random.Shared.NextDouble))
                AiCosmetic(() => MultiplayerAi.Submit(RunManager.Instance.NetService, player.NetId, ping!));
        }
    }

    /// <summary>True when this frame sends one ping for <paramref name="wait"/>.</summary>
    internal static bool AiNudgeStep(AiNudge nudge, object? wait, bool endTurn, long now, long second,
        Func<double> random)
    {
        double Seconds(double low, double high) => low + random() * (high - low);
        if (wait is null) { nudge.Wait = null; return false; }
        if (!Equals(nudge.Wait, wait)) {
            nudge.Wait = wait;
            nudge.Sent = nudge.Burst = 0;
            // Not every teammate nags, and the ones who do lose patience at different times.
            nudge.Due = random() < .65
                ? now + (long)(second * (endTurn ? Seconds(8, 16) : Seconds(6, 12)))
                : long.MaxValue;
            return false;
        }
        if (now < nudge.Due) return false;
        nudge.Sent++;
        if (nudge.Sent >= AiNudgeLimit) nudge.Due = long.MaxValue;
        else if (nudge.Burst > 0) {
            nudge.Burst--;
            // The native button debounces end-turn pings to one per second.
            nudge.Due = now + (long)(second * (endTurn ? Seconds(1.05, 1.6) : Seconds(.25, .6)));
        } else {
            nudge.Burst = random() < .35 ? 1 + (int)(random() * 2) : 0;
            nudge.Due = now + (long)(second * Seconds(6, 14));
        }
        return true;
    }

    static void AiPresenceFrame()
    {
        if (!MultiplayerAi.HostsAi || run is null) return;
        if (!ReferenceEquals(aiPresenceRoom, run.CurrentRoom)) {
            ClearAiPresence();
            aiPresenceRoom = run.CurrentRoom;
        }
        foreach (var (player, presence) in aiPresence)
            if (!presence.Active) AiClearPresence(player);
        long now = Stopwatch.GetTimestamp();
        AiCosmetic(() => AiNudgeFrame(now));
        foreach (var (player, celebration) in aiCelebrations.ToArray())
            if (now >= celebration.Due) {
                aiCelebrations.Remove(player);
                if (ReferenceEquals(run.CurrentRoom, celebration.Room))
                    AiCosmetic(() => AiPresenceFor(player).Celebrate(
                        AiCreaturePosition(player.Creature, NGame.Instance?.ReactionContainer)));
            }
    }

    static (CombatRoom Room, long At)? aiCombatWon;
    static void AiCelebrate(CombatRoom room)
    {
        aiCombatWon = (room, Stopwatch.GetTimestamp());
        if (!MultiplayerAi.HostsAi || run is null) return;
        foreach (var player in run.Players.Where(AiOwned))
            if (Random.Shared.NextDouble() < .35)
                aiCelebrations[player] = (room, Stopwatch.GetTimestamp() +
                    (long)(Stopwatch.Frequency * (.3 + Random.Shared.NextDouble() * .7)));
    }

    static void ClearAiPresence()
    {
        foreach (var player in aiPresence.Keys) AiClearPresence(player);
        aiPresence.Clear();
        aiCelebrations.Clear();
        aiNudges.Clear();
        aiPresenceRoom = null;
    }
}
