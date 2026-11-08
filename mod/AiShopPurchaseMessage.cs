using MegaCrit.Sts2.Core.Logging;
using MegaCrit.Sts2.Core.Multiplayer.Game;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Game;
using MegaCrit.Sts2.Core.Multiplayer.Serialization;
using MegaCrit.Sts2.Core.Multiplayer.Transport;
using MegaCrit.Sts2.Core.Runs;

namespace HumanPlayRecorder;

// Unlike native human-shop outcome packets, this host command runs the normal
// merchant wrapper on every peer, preserving inventory/refill RNG and hooks.
public sealed class AiShopPurchaseMessage : INetMessage, IRunLocationTargetedMessage
{
    public ulong playerId;
    public int entryIndex;
    public int price;
    public RunLocation location;
    public RunLocation Location => location;
    public bool ShouldBroadcast => false;
    public bool ShouldBuffer => true;
    public NetTransferMode Mode => NetTransferMode.Reliable;
    public LogLevel LogLevel => LogLevel.VeryDebug;
    public void Serialize(PacketWriter writer)
    {
        writer.WriteULong(playerId);
        writer.WriteInt(entryIndex);
        writer.WriteInt(price);
        writer.Write(location);
    }
    public void Deserialize(PacketReader reader)
    {
        playerId = reader.ReadULong();
        entryIndex = reader.ReadInt();
        price = reader.ReadInt();
        location = reader.Read<RunLocation>();
    }
}
