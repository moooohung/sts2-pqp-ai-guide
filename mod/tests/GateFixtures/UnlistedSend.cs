using MegaCrit.Sts2.Core.Multiplayer.Game;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Lobby;

namespace HumanPlayRecorder.GateFixtures;

// Gate fixture (never shipped): a game multiplayer send from a method that is not a reviewed
// call site; the offline no-network gate must reject it.
static class UnlistedSend
{
    internal static void Run(INetGameService service) => service.SendMessage(new PlayerLeftMessage { playerId = 1 });
}
