using HarmonyLib;
using MegaCrit.Sts2.Core.Multiplayer;
using MegaCrit.Sts2.Core.Multiplayer.Game;

namespace HumanPlayRecorder.VanillaGuard;

// Reservations start at service construction, before either host startup or
// client transport connection. Release waits for the disconnect callback and
// all tracked packet/update operations to return.
public static class WireTransportBoundary
{
    public static WireCacheBoundary Boundary { get; } = new();
    static readonly object sync = new();
    sealed class State(IDisposable reservation)
    {
        public IDisposable Reservation = reservation;
        public bool Closed;
        public int Operations;
    }
    static readonly Dictionary<object, State> states = new(ReferenceEqualityComparer.Instance);
    internal static object[] ActiveServices { get { lock (sync) return states.Keys.ToArray(); } }

    internal static void Install(Harmony h)
    {
        foreach (var type in new[] { typeof(NetHostGameService), typeof(NetClientGameService) })
        {
            foreach (var constructor in AccessTools.GetDeclaredConstructors(type))
                h.Patch(constructor, prefix: new HarmonyMethod(typeof(WireTransportBoundary), nameof(Constructing)),
                    finalizer: new HarmonyMethod(typeof(WireTransportBoundary), nameof(Constructed)));
            foreach (var name in new[] { "Update", "OnPacketReceived", "SendHandshakeMessage" })
                h.Patch(AccessTools.Method(type, name),
                    prefix: new HarmonyMethod(typeof(WireTransportBoundary), nameof(Enter)),
                    finalizer: new HarmonyMethod(typeof(WireTransportBoundary), nameof(Exit)));
            h.Patch(AccessTools.Method(type, "Disconnect"),
                prefix: new HarmonyMethod(typeof(WireTransportBoundary), nameof(Enter)),
                finalizer: new HarmonyMethod(typeof(WireTransportBoundary), nameof(DisconnectFinished)));
        }
        h.Patch(AccessTools.Method(typeof(NetHostGameService), "OnDisconnected"),
            prefix: new HarmonyMethod(typeof(WireTransportBoundary), nameof(Enter)),
            finalizer: new HarmonyMethod(typeof(WireTransportBoundary), nameof(Disconnected)));
        h.Patch(AccessTools.Method(typeof(NetClientGameService), "OnDisconnectedFromHost"),
            prefix: new HarmonyMethod(typeof(WireTransportBoundary), nameof(Enter)),
            finalizer: new HarmonyMethod(typeof(WireTransportBoundary), nameof(Disconnected)));
    }
    static void Constructing(object __instance)
    {
        var reservation = Boundary.ReserveConnection();
        lock (sync) states.Add(__instance, new(reservation));
    }
    static void Constructed(object __instance, Exception? __exception)
    {
        if (__exception is not null) Close(__instance);
        else WireInterop.OnServiceConstructed(__instance);
    }
    static void Enter(object __instance, out IDisposable __state)
    {
        __state = Boundary.EnterPacketOperation();
        lock (sync)
        {
            if (states.TryGetValue(__instance, out var state)) state.Operations++;
        }
    }
    static void Exit(object __instance, IDisposable? __state)
    {
        lock (sync)
        {
            __state?.Dispose();
            if (states.TryGetValue(__instance, out var state))
            {
                state.Operations--;
                ReleaseClosed(__instance, state);
            }
        }
    }
    static void Disconnected(object __instance, IDisposable? __state)
    {
        Close(__instance);
        Exit(__instance, __state);
    }
    static void DisconnectFinished(object __instance, IDisposable? __state)
    {
        // IsConnected is false during connection/handshake too. Only absence
        // of the transport proves that no completion callback can be pending.
        if (__instance is NetHostGameService { NetHost: null } ||
            __instance is NetClientGameService { NetClient: null })
            Close(__instance);
        Exit(__instance, __state);
    }
    static void Close(object service)
    {
        lock (sync)
            if (states.TryGetValue(service, out var state))
            {
                state.Closed = true;
                ReleaseClosed(service, state);
            }
    }
    static void ReleaseClosed(object service, State state)
    {
        if (!state.Closed || state.Operations != 0) return;
        state.Reservation.Dispose();
        states.Remove(service);
        WireInterop.OnServiceClosed(service);
    }
}
