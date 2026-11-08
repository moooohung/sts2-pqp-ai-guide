using MegaCrit.Sts2.Core.Logging;
using MegaCrit.Sts2.Core.Multiplayer.Serialization;
using MegaCrit.Sts2.Core.Multiplayer.Transport;

namespace HumanPlayRecorder;

// One player's own training-data consent (PartyConsent). The message names nobody: whose answer it
// is comes from the connection that delivered it (ConsentWire), never from a field the sender writes.
// The game must not relay it by itself (it would pass on the sender's claimed id), and it is
// dispatched immediately so the delivering connection is still known when the handler runs.
public sealed class HumanPlayConsentMessage : INetMessage
{
    public bool agreed;

    public bool ShouldBroadcast => false;
    public NetTransferMode Mode => NetTransferMode.Reliable;
    public LogLevel LogLevel => LogLevel.VeryDebug;
    public bool ShouldBuffer => false;

    public void Serialize(PacketWriter writer) => writer.WriteInt(agreed ? 1 : 0);

    public void Deserialize(PacketReader reader) => agreed = reader.ReadInt() == 1;
}
