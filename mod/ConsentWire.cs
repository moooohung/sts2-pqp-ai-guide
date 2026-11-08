using System.Reflection;
using System.Runtime.CompilerServices;
using HarmonyLib;
using MegaCrit.Sts2.Core.Entities.Multiplayer;
using MegaCrit.Sts2.Core.Multiplayer;
using MegaCrit.Sts2.Core.Multiplayer.Game;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Lobby;
using MegaCrit.Sts2.Core.Multiplayer.Transport;

namespace HumanPlayRecorder;

// How training-data consent moves between players. This is a HINT, never authority: in a host-relayed
// game a dishonest host can say anything about the other players, so what lets a multiplayer recording
// into training or listings is decided by the server from every participant's own receipt (run, seat,
// training=true). The hint only lets this PC skip an upload that cannot qualify and tell the player why.
//  * Only a peer that is known to run this mod is ever sent HumanPlayConsentMessage: the host checks
//    each client's handshake version, a client checks the host's (recorded here at its handshake). A
//    vanilla host or friend never receives a packet type it does not know.
//  * Whose answer a message is comes from the connection that delivered it. The host takes a client's
//    answer only from that client's own connection and relays it under that client's id; a client takes
//    other players' answers from the host's relay of them. Nothing a message says about "who" is read.
static class ConsentWire
{
    [ThreadStatic] static ulong? delivering;
    static readonly ConditionalWeakTable<object, StrongBox<bool>> hostRunsMod = new();
    static readonly MethodInfo sendAs = AccessTools.Method(typeof(NetHostGameService), "SendMessageToClientInternal");

    internal static void Install(Harmony harmony)
    {
        foreach (var type in new[] { typeof(NetHostGameService), typeof(NetClientGameService) })
            harmony.Patch(AccessTools.Method(type, "OnPacketReceived"),
                prefix: new HarmonyMethod(typeof(ConsentWire), nameof(Delivering)),
                finalizer: new HarmonyMethod(typeof(ConsentWire), nameof(Delivered)));
        harmony.Patch(AccessTools.Method(typeof(NetClientGameService), nameof(NetClientGameService.HandshakeSucceeded)),
            postfix: new HarmonyMethod(typeof(ConsentWire), nameof(HostHandshake)));
    }

    // Nested dispatch (a handler that sends, a loopback transport) restores the outer delivery.
    static void Delivering(ulong senderId, out ulong? __state)
    {
        __state = delivering;
        delivering = senderId;
    }
    static void Delivered(ulong? __state) => delivering = __state;
    static void HostHandshake(NetClientGameService __instance, PeerVersionInfo versionInfo) =>
        RecordHost(__instance, versionInfo);

    internal static bool HasMod(PeerVersionInfo? version) => version is { } v &&
        (v.otherMods?.Contains(RecorderMod.NetworkModName, StringComparer.Ordinal) == true ||
         v.gameplayAffectingMods?.Contains(RecorderMod.NetworkModName, StringComparer.Ordinal) == true);

    internal static void RecordHost(object clientService, PeerVersionInfo hostVersion) =>
        hostRunsMod.AddOrUpdate(clientService, new StrongBox<bool>(HasMod(hostVersion)));

    // Unknown (no handshake seen) counts as "no".
    internal static bool HostRunsMod(object? clientService) =>
        clientService is not null && hostRunsMod.TryGetValue(clientService, out var yes) && yes.Value;

    // Which player a consent message speaks for, or false when it cannot be attributed.
    internal static bool TryOrigin(object service, ulong headerSender, out ulong origin)
    {
        origin = 0;
        if (delivering is not { } connection) return false;
        if (service is NetHostGameService)
        {
            origin = connection;
            return headerSender == connection;
        }
        if (service is NetClientGameService { NetClient: not null } client && connection == client.NetClient.HostNetId)
        {
            origin = headerSender;
            return true;
        }
        return false;
    }

    // This player's own answer: a client to a host that runs the mod, a host to every client that does.
    internal static void SendOwn(object service, bool agreed)
    {
        var message = new HumanPlayConsentMessage { agreed = agreed };
        if (service is NetClientGameService client)
        {
            if (client.IsConnected && HostRunsMod(client)) client.SendMessage(message);
        }
        else if (service is NetHostGameService host && host.IsConnected)
        {
            foreach (var peer in host.ConnectedPeers.Where(p => HasMod(p.versionInfo)))
                host.SendMessage(message, peer.peerId);
        }
    }

    // The host passes a client's answer on to the other clients that run the mod, as that client's.
    internal static void Relay(NetHostGameService host, ulong origin, bool agreed)
    {
        var message = new HumanPlayConsentMessage { agreed = agreed };
        var send = sendAs.MakeGenericMethod(typeof(HumanPlayConsentMessage));
        foreach (var peer in host.ConnectedPeers.Where(p => p.peerId != origin && HasMod(p.versionInfo)))
            send.Invoke(host, [message, peer.peerId, message.Mode.ToChannelId(), (ulong?)origin]);
    }

    // Everything the host knows, handed to the clients that run the mod before the run begins.
    internal static void Announce(NetHostGameService host, IReadOnlyDictionary<ulong, bool> known, bool hostAgreed)
    {
        SendOwn(host, hostAgreed);
        foreach (var (origin, agreed) in known)
            if (origin != host.NetId) Relay(host, origin, agreed);
    }

    // Whoever leaves, joins or reconnects during a run ends this PC's upload of it, on every recorder:
    // the host sees its own transport events, a client hears the host's lobby messages about the others
    // and its own disconnect. A host that stays silent is why this is only a hint (see above).
    internal sealed class MembershipWatch : IDisposable
    {
        readonly object service;
        readonly Action<string> changed;
        readonly MessageHandlerDelegate<PlayerLeftMessage> left;
        readonly MessageHandlerDelegate<PlayerReconnectedMessage> reconnected;
        readonly MessageHandlerDelegate<PlayerRejoinedMessage> rejoined;

        internal MembershipWatch(object service, Action<string> changed)
        {
            this.service = service;
            this.changed = changed;
            left = (_, _) => changed("a player left");
            reconnected = (_, _) => changed("a player reconnected");
            rejoined = (_, _) => changed("a player rejoined");
            switch (service)
            {
                case NetHostGameService host:
                    host.ClientConnected += Joined;
                    host.ClientDisconnected += Left;
                    break;
                case NetClientGameService client:
                    client.Disconnected += Lost;
                    client.RegisterMessageHandler(left);
                    client.RegisterMessageHandler(reconnected);
                    client.RegisterMessageHandler(rejoined);
                    break;
            }
        }

        void Joined(ulong peer) => changed("a player joined or reconnected");
        void Left(ulong peer, NetErrorInfo info) => changed("a player left");
        void Lost(NetErrorInfo info) => changed("the connection to the host was lost");

        public void Dispose()
        {
            switch (service)
            {
                case NetHostGameService host:
                    host.ClientConnected -= Joined;
                    host.ClientDisconnected -= Left;
                    break;
                case NetClientGameService client:
                    client.Disconnected -= Lost;
                    client.UnregisterMessageHandler(left);
                    client.UnregisterMessageHandler(reconnected);
                    client.UnregisterMessageHandler(rejoined);
                    break;
            }
        }
    }
}