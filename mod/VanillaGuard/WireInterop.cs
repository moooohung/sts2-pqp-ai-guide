using System.Collections;
using System.Buffers.Binary;
using System.Reflection;
using HarmonyLib;
using HumanPlayRecorder.Settings;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.GameActions.Multiplayer;
using MegaCrit.Sts2.Core.Logging;
using MegaCrit.Sts2.Core.Modding;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Multiplayer;
using MegaCrit.Sts2.Core.Multiplayer.Connection;
using MegaCrit.Sts2.Core.Multiplayer.Messages;
using MegaCrit.Sts2.Core.Multiplayer.Serialization;
using MegaCrit.Sts2.Core.Multiplayer.Transport;
using MegaCrit.Sts2.Core.Runs;
using MegaCrit.Sts2.Core.Saves.Runs;
using MegaCrit.Sts2.Core.Timeline;
using MegaCrit.Sts2.Core.Unlocks;

namespace HumanPlayRecorder.VanillaGuard;

internal static class WireInterop
{
    static readonly Type ModelCacheType = typeof(ModelIdSerializationCache);
    static readonly FieldInfo InitializedField = AccessTools.Field(ModelCacheType, "_initialized");
    static readonly FieldInfo[] ModelCacheFields = ModelCacheType
        .GetFields(BindingFlags.Static | BindingFlags.NonPublic)
        .Where(f => f.Name != "_initialized")
        .ToArray();
    static readonly PropertyInfo[] ModelCacheProperties = [
        AccessTools.Property(ModelCacheType, nameof(ModelIdSerializationCache.CategoryIdBitSize)),
        AccessTools.Property(ModelCacheType, nameof(ModelIdSerializationCache.EntryIdBitSize)),
        AccessTools.Property(ModelCacheType, nameof(ModelIdSerializationCache.EpochIdBitSize)),
        AccessTools.Property(ModelCacheType, nameof(ModelIdSerializationCache.PropertyIdBitSize)),
        AccessTools.Property(ModelCacheType, nameof(ModelIdSerializationCache.Hash))
    ];
    static readonly FieldInfo MessageCacheField = AccessTools.Field(typeof(MessageTypes), "_cache");
    static readonly FieldInfo ActionCacheField = AccessTools.Field(typeof(ActionTypes), "_cache");
    static readonly FieldInfo HostLocalVersionField = AccessTools.Field(typeof(NetHostGameService), "<LocalVersion>k__BackingField");
    static readonly FieldInfo ClientLocalVersionField = AccessTools.Field(typeof(NetClientGameService), "<LocalVersion>k__BackingField");
    static readonly FieldInfo HostHandshakeField = AccessTools.Field(typeof(NetHostGameService), "_handshakeManager");
    static readonly FieldInfo ClientHandshakeField = AccessTools.Field(typeof(NetClientGameService), "_handshakeManager");
    static readonly FieldInfo HandshakeVersionField = AccessTools.Field(typeof(HandshakeManager), "_localVersionInfo");
    static readonly FieldInfo TransportHandlerField = AccessTools.Field(typeof(NetHost), "_handler");
    static readonly HashSet<Type> VanillaMessages = INetMessageSubtypes.All.ToHashSet();

    sealed record ModelCacheState(
        bool Initialized,
        (FieldInfo Field, object? Value)[] Fields,
        (PropertyInfo Property, object? Value)[] Properties);

    static readonly object sync = new();
    static readonly Dictionary<object, PeerVersionInfo> savedServiceVersions = new(ReferenceEqualityComparer.Instance);
    static HashSet<string>? vanillaEpochIds;
    static ModelCacheState? vanillaModelCache;
    static HashSet<Type>? vanillaModelTypes;
    static ModelCacheState? savedModelCache;
    static object? savedMessageCache;
    static object? savedActionCache;
    static bool active;
    [ThreadStatic] static bool buildingVanillaCache;

    internal static bool Active => active;

    internal static NetHostGameService? HostService(NetHost transport)
    {
        object handler = TransportHandlerField.GetValue(transport)!;
        // CouchCoop's ENet side decorates the same game service used by its Steam host.
        if (handler.GetType().FullName == "CouchCoop.Mod.Session.DualNetHost+SideHandler")
            handler = AccessTools.Field(handler.GetType(), "_service").GetValue(handler)!;
        return handler as NetHostGameService;
    }

    internal static bool AllowPacket(NetHost transport, ulong peer, ref byte[] bytes, int length)
    {
        if (!active || bytes.AsSpan(0, length).StartsWith(HandshakeManager.magicBytes)) return true;
        var host = HostService(transport);
        if (host is null) return true;
        var version = host.GetVersionInfoForPeer(peer);
        if (!MessageTypes.TryGetMessageType(bytes[0], out var type))
            return version?.otherMods?.Count > 0 || version?.gameplayAffectingMods?.Count > 0;
        if (VanillaMessages.Contains(type!))
        {
            if (version?.otherMods?.Count > 0 || version?.gameplayAffectingMods?.Count > 0 ||
                (type != typeof(HeartbeatRequestMessage) && type != typeof(HeartbeatResponseMessage))) return true;
            ulong sender = BinaryPrimitives.ReadUInt64LittleEndian(bytes.AsSpan(1, sizeof(ulong)));
            if (sender != host.NetId && sender != transport.NetId) return false;
            // The dual host uses its Steam identity in the lobby, but an unmodded
            // ENet client can only answer transport host 1. Never rewrite gameplay senders.
            if (sender != transport.NetId)
            {
                bytes = bytes[..length];
                BinaryPrimitives.WriteUInt64LittleEndian(bytes.AsSpan(1, sizeof(ulong)), transport.NetId);
            }
            return true;
        }
        var owner = NativeModCompatibility.Owner(type!.Assembly)?.manifest;
        if (owner is null) return false;
        string name = owner.id + "-" + owner.version;
        return version?.otherMods?.Contains(name, StringComparer.Ordinal) == true ||
            version?.gameplayAffectingMods?.Contains(name, StringComparer.Ordinal) == true;
    }

    // A cosmetic manifest permits presentation changes, not changes to the packet grammar.
    // All Original Rules peers share this schema; the modded schema is restored on exit.
    internal static bool IsSchemaTarget(MethodBase method)
    {
        var type = method.DeclaringType;
        return type == ModelCacheType ||
            type == typeof(PacketReader) || type == typeof(PacketWriter) ||
            (type == typeof(NetMessageBus) && method.Name is "SerializeMessage" or "TryDeserializeMessage") ||
            (type == typeof(SavedProperties) && method.Name is "FromInternal" or "FillInternal") ||
            (type?.Assembly == typeof(ModelDb).Assembly && typeof(IPacketSerializable).IsAssignableFrom(type) &&
                method.Name is "Serialize" or "Deserialize");
    }

    static HashSet<string> VanillaEpochIds => vanillaEpochIds ??=
        ((IReadOnlyList<Type>)AccessTools.Field(typeof(EpochModel), "_allEpochs").GetValue(null)!)
            .Where(t => t.Assembly == typeof(ModelDb).Assembly)
            .Select(EpochModel.GetId)
            .ToHashSet(StringComparer.Ordinal);

    internal static void Install(Harmony harmony)
    {
        harmony.Patch(AccessTools.Method(typeof(ModManager), nameof(ModManager.GetGameplayRelevantModNameList)),
            postfix: new HarmonyMethod(typeof(WireInterop), nameof(FilterGameplayMods)) { priority = Priority.Last });
        harmony.Patch(AccessTools.Method(typeof(PeerVersionInfo), nameof(PeerVersionInfo.LocalDefault)),
            prefix: new HarmonyMethod(typeof(WireInterop), nameof(BeforeLocalDefault)) { priority = Priority.First },
            postfix: new HarmonyMethod(typeof(WireInterop), nameof(AfterLocalDefault)) { priority = Priority.Last });
        harmony.Patch(AccessTools.PropertyGetter(typeof(ModelDb), nameof(ModelDb.All)),
            postfix: new HarmonyMethod(typeof(WireInterop), nameof(FilterAllModels)) { priority = Priority.Last });
        harmony.Patch(AccessTools.PropertyGetter(typeof(EpochModel), nameof(EpochModel.AllEpochs)),
            postfix: new HarmonyMethod(typeof(WireInterop), nameof(FilterAllEpochs)) { priority = Priority.Last });
        harmony.Patch(AccessTools.PropertyGetter(typeof(EpochModel), nameof(EpochModel.AllEpochIds)),
            postfix: new HarmonyMethod(typeof(WireInterop), nameof(FilterAllEpochIds)) { priority = Priority.Last });
        harmony.Patch(AccessTools.Method(typeof(UnlockState), nameof(UnlockState.ToSerializable)),
            postfix: new HarmonyMethod(typeof(WireInterop), nameof(FilterSerializableUnlocks)) { priority = Priority.Last });
        harmony.Patch(AccessTools.Method(typeof(Player), nameof(Player.ToSerializable)),
            postfix: new HarmonyMethod(typeof(WireInterop), nameof(FilterSerializablePlayer)) { priority = Priority.Last });
        Harmony.ReversePatch(AccessTools.Method(ModelCacheType, nameof(ModelIdSerializationCache.Init)),
            new HarmonyMethod(typeof(WireInterop), nameof(OriginalInit)));
    }

    // The game's own Init body. Mods postfix Init to rebuild the maps from all ModelDb content
    // (STS2-RitsuLib), which would put mod entries and a wider entry bit width into the vanilla tables.
    static void OriginalInit() => throw new NotImplementedException("reverse patch stub");

    static void FilterGameplayMods(ref List<string>? __result)
    {
        if (active || buildingVanillaCache || ContentPools.Active)
            __result = null;
    }

    static bool BeforeLocalDefault(ref PeerVersionInfo __result)
    {
        if (OriginalRulesSettings.Enabled && !ContentPools.Active &&
            ContentPools.NonNeutralizationReason is null && !RunManager.Instance.IsInProgress)
            MenuLifecycle.OpenSelection(true);
        if (MegaCrit.Sts2.Core.Nodes.NGame.Instance is not null) return true;
        __result = new PeerVersionInfo
        {
            version = "0.111.0",
            branch = default,
            idDatabaseHash = ModelIdSerializationCache.Hash,
            gameplayAffectingMods = ModManager.GetGameplayRelevantModNameList(),
            otherMods = ModManager.GetNonGameplayRelevantModNameList()
        };
        return false;
    }

    static void AfterLocalDefault(ref PeerVersionInfo __result)
    {
        if (!active && !ContentPools.Active) return;
        __result.idDatabaseHash = ModelIdSerializationCache.Hash;
        __result.gameplayAffectingMods = null;
    }

    // Exactly the models an unmodded game registers: the game's generated type list, each under
    // its own id. The shipped content catalog covers content categories only, so it cannot decide this.
    static void FilterAllModels(ref IEnumerable<AbstractModel> __result)
    {
        if (!buildingVanillaCache) return;
        var types = vanillaModelTypes ??= AbstractModelSubtypes.All.ToHashSet();
        __result = __result.Where(m => types.Contains(m.GetType()) && m.Id == ModelDb.GetId(m.GetType())).ToArray();
    }

    static void FilterAllEpochs(ref IReadOnlyList<Type> __result)
    {
        if (!buildingVanillaCache && !active && !ContentPools.Active) return;
        __result = __result.Where(t => t.Assembly == typeof(ModelDb).Assembly).ToArray();
    }

    static void FilterAllEpochIds(ref IReadOnlyList<string> __result)
    {
        if (!buildingVanillaCache && !active && !ContentPools.Active) return;
        var allowed = VanillaEpochIds;
        __result = __result.Where(allowed.Contains).ToArray();
    }

    static void FilterSerializableUnlocks(ref SerializableUnlockState __result)
    {
        if ((!active && !ContentPools.Active) || __result is null) return;
        var allowedEpochs = VanillaEpochIds;
        __result.UnlockedEpochs = __result.UnlockedEpochs.Where(allowedEpochs.Contains).ToList();
        __result.EncountersSeen = __result.EncountersSeen
            .Where(id => ContentPools.IsOriginalId(id.ToString())).ToList();
    }

    static void FilterSerializablePlayer(ref MegaCrit.Sts2.Core.Saves.Runs.SerializablePlayer __result)
    {
        if ((!active && !ContentPools.Active) || __result is null) return;
        var allowedEpochs = VanillaEpochIds;
        __result.DiscoveredEpochs = __result.DiscoveredEpochs.Where(allowedEpochs.Contains).ToList();
        __result.DiscoveredCards = __result.DiscoveredCards.Where(id => ContentPools.IsOriginalId(id.ToString())).ToList();
        __result.DiscoveredEnemies = __result.DiscoveredEnemies.Where(id => ContentPools.IsOriginalId(id.ToString())).ToList();
        __result.DiscoveredPotions = __result.DiscoveredPotions.Where(id => ContentPools.IsOriginalId(id.ToString())).ToList();
        __result.DiscoveredRelics = __result.DiscoveredRelics.Where(id => ContentPools.IsOriginalId(id.ToString())).ToList();
    }

    static ModelCacheState CaptureModelCache()
    {
        bool initialized = (bool)InitializedField.GetValue(null)!;
        var fields = ModelCacheFields.Select(f => {
            object? value = f.GetValue(null);
            object? copy = value switch {
                IDictionary d => d.Keys.Cast<object>().Select(k => new DictionaryEntry(
                    k, d[k] is IList list ? list.Cast<object>().ToList() : d[k])).ToArray(),
                IList l => l.Cast<object>().ToArray(),
                _ => value
            };
            return (f, copy);
        }).ToArray();
        var props = ModelCacheProperties.Select(p => (p, p.GetValue(null))).ToArray();
        return new(initialized, fields, props);
    }

    static void ApplyModelCache(ModelCacheState state)
    {
        foreach (var (field, value) in state.Fields)
        {
            if (field.GetValue(null) is IDictionary d && value is DictionaryEntry[] entries)
            {
                d.Clear();
                foreach (var entry in entries)
                {
                    if (entry.Value is IEnumerable<object> list && field.FieldType.IsGenericType &&
                        field.FieldType.GetGenericArguments()[1] == typeof(List<PropertyInfo>))
                        d.Add(entry.Key, list.Cast<PropertyInfo>().ToList());
                    else d.Add(entry.Key, entry.Value);
                }
            }
            else if (field.GetValue(null) is IList l && value is object[] items)
            {
                l.Clear();
                foreach (var item in items) l.Add(item);
            }
            else field.SetValue(null, value);
        }
        foreach (var (prop, value) in state.Properties)
            prop.SetValue(null, value);
        InitializedField.SetValue(null, state.Initialized);
    }

    // Vanilla ids come first, numbered as a peer without mods numbers them. Every mod type of the
    // live cache follows, so a mod that sends its own message (a skin sync) never throws: cosmetic
    // mods first in the game's own order, which a friend with the same cosmetic mods shares, then
    // gameplay mods. The physical send boundary withholds types absent from a peer's mod list.
    static NetTypeCache<T> VanillaFirstCache<T>(List<Type> vanilla, object? live, params Type[] required) where T : class
    {
        var idField = AccessTools.Field(typeof(NetTypeCache<T>), "_idToType");
        var cache = new NetTypeCache<T>(vanilla);
        var idToType = (List<Type>)idField.GetValue(cache)!;
        var typeToId = (Dictionary<Type, int>)AccessTools.Field(typeof(NetTypeCache<T>), "_typeToId").GetValue(cache)!;
        var liveTypes = live is null ? [] : (List<Type>)idField.GetValue(live)!;
        foreach (var type in liveTypes.Concat(required.OrderBy(t => t.Name, StringComparer.Ordinal))
            .Where(t => !typeToId.ContainsKey(t)).Distinct().OrderBy(AffectsGameplay))
        {
            typeToId[type] = idToType.Count;
            idToType.Add(type);
        }
        return cache;
    }

    static bool AffectsGameplay(Type type) => AssemblyInfo.ModForType(type, out _)?.manifest?.affectsGameplay ?? true;

    internal static void Begin()
    {
        lock (sync)
        {
            if (active) return;
            if (AssemblyInfo.ModMap is null) AssemblyInfo.Init();
            savedModelCache = CaptureModelCache();
            savedMessageCache = MessageCacheField.GetValue(null);
            savedActionCache = ActionCacheField.GetValue(null);
            var savedMockTypes = AssemblyInfo.MockTypes;
            var savedBaseGame = AssemblyInfo.BaseGame;
            buildingVanillaCache = true;
            try
            {
                if (vanillaModelCache is null)
                {
                    AssemblyInfo.MockTypes = null;
                    AccessTools.Property(typeof(AssemblyInfo), nameof(AssemblyInfo.BaseGame))
                        .SetValue(null, typeof(ModelDb).Assembly);
                    ModelIdSerializationCache.ResetForTest();
                    OriginalInit();
                    vanillaModelCache = CaptureModelCache();
                }
                else
                {
                    ApplyModelCache(vanillaModelCache);
                }
                // Built from the live caches each time, so mod types registered after the first scope are kept.
                var messages = VanillaFirstCache<INetMessage>(INetMessageSubtypes.All.ToList(), savedMessageCache,
                    typeof(AiShopPurchaseMessage), typeof(HumanPlayConsentMessage), typeof(HumanPlayRunNonceMessage));
                MessageCacheField.SetValue(null, messages);
                for (int id = VanillaMessages.Count; id < messages.Count; id++)
                    if (messages.TryGetTypeFromId(id, out var type))
                        Log.Info($"[HumanPlayRecorder] original-rules wire message id={id} type={type!.FullName} owner={NativeModCompatibility.Owner(type.Assembly)?.manifest?.id ?? "unattributed"}");
                ActionCacheField.SetValue(null, VanillaFirstCache<INetAction>(INetActionSubtypes.All.ToList(), savedActionCache));
                active = true;
                foreach (var service in WireTransportBoundary.ActiveServices)
                    ApplyVanillaServiceVersion(service);
            }
            catch
            {
                active = false;
                RestoreCaches();
                throw;
            }
            finally
            {
                buildingVanillaCache = false;
                AssemblyInfo.MockTypes = savedMockTypes;
                AccessTools.Property(typeof(AssemblyInfo), nameof(AssemblyInfo.BaseGame))
                    .SetValue(null, savedBaseGame);
            }
        }
    }

    static void RestoreCaches()
    {
        if (savedModelCache is { } model) ApplyModelCache(model);
        if (savedMessageCache is not null) MessageCacheField.SetValue(null, savedMessageCache);
        if (savedActionCache is not null) ActionCacheField.SetValue(null, savedActionCache);
        savedModelCache = null;
        savedMessageCache = null;
        savedActionCache = null;
        foreach (var (service, original) in savedServiceVersions.ToArray())
            SetServiceVersion(service, original);
        savedServiceVersions.Clear();
    }

    internal static void End()
    {
        lock (sync)
        {
            if (!active && savedModelCache is null) return;
            active = false;
            RestoreCaches();
        }
    }

    internal static void OnServiceConstructed(object service)
    {
        lock (sync)
        {
            if (active) ApplyVanillaServiceVersion(service);
        }
    }

    internal static void OnServiceClosed(object service)
    {
        lock (sync)
        {
            savedServiceVersions.Remove(service);
        }
    }

    static void ApplyVanillaServiceVersion(object service)
    {
        if (TryGetServiceVersion(service, out var current))
        {
            if (!savedServiceVersions.ContainsKey(service))
                savedServiceVersions[service] = current;
            var vanilla = current;
            vanilla.idDatabaseHash = ModelIdSerializationCache.Hash;
            vanilla.gameplayAffectingMods = null;
            SetServiceVersion(service, vanilla);
        }
    }

    static bool TryGetServiceVersion(object service, out PeerVersionInfo version)
    {
        if (service is NetHostGameService host) { version = host.LocalVersion; return true; }
        if (service is NetClientGameService client) { version = client.LocalVersion; return true; }
        version = default;
        return false;
    }

    static void SetServiceVersion(object service, PeerVersionInfo version)
    {
        if (service is NetHostGameService)
        {
            HostLocalVersionField.SetValue(service, version);
            if (HostHandshakeField.GetValue(service) is { } hm)
                HandshakeVersionField.SetValue(hm, version);
        }
        else if (service is NetClientGameService)
        {
            ClientLocalVersionField.SetValue(service, version);
            if (ClientHandshakeField.GetValue(service) is { } hm)
                HandshakeVersionField.SetValue(hm, version);
        }
    }
}
