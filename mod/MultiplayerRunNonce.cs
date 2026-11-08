using MegaCrit.Sts2.Core.Logging;
using MegaCrit.Sts2.Core.Multiplayer.Serialization;
using MegaCrit.Sts2.Core.Multiplayer.Transport;

namespace HumanPlayRecorder;

// A fresh host-generated nonce sent immediately before the game's reliable
// LobbyBeginRunMessage. MessageTypes includes INetMessage subtypes from mods,
// so peers with this mod can derive the same privacy-preserving shared_run_id
// even when the Steam lobby itself is reused for another run.
public sealed class HumanPlayRunNonceMessage : INetMessage
{
    public string nonce = "";

    public bool ShouldBroadcast => true;
    public NetTransferMode Mode => NetTransferMode.Reliable;
    public LogLevel LogLevel => LogLevel.VeryDebug;
    public bool ShouldBuffer => true;

    public void Serialize(PacketWriter writer) => writer.WriteString(nonce);
    public void Deserialize(PacketReader reader) => nonce = reader.ReadString();
}
