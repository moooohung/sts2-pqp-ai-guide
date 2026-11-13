// What the offline assembly (PqpAiGuideOffline.dll) may reference and where it may use sensitive
// members. OfflineGate applies it; see the boundary at the top of OfflineGate.cs. Every entry was
// reviewed against the source; a change to any of them needs the same review. Callers are source
// methods: lambdas, local functions and async bodies count as the method that declares them.
static class OfflineAllowlist
{
    internal const string NativeBridge = "sts2_advisor_bridge";

    internal static readonly HashSet<string> Assemblies = new(StringComparer.Ordinal) {
        "System.Runtime", "System.Collections", "System.Collections.Concurrent", "System.Linq", "System.Memory",
        "System.Threading", "System.Threading.Thread", "System.Runtime.InteropServices", "System.Text.Json",
        "System.Security.Cryptography", "System.Console", "System.IO.Hashing",
        "sts2", "0Harmony", "GodotSharp"
    };

    // "assembly: namespace" of every referenced type; ".*" admits sub-namespaces. Game types are
    // admitted by namespace and gated per member (OfflineGate.Sensitive).
    internal static readonly HashSet<string> Namespaces = new(StringComparer.Ordinal) {
        "System.Runtime: System", "System.Runtime: System.Collections", "System.Runtime: System.Collections.Generic",
        "System.Runtime: System.Collections.ObjectModel", "System.Runtime: System.Diagnostics",
        "System.Runtime: System.Globalization", "System.Runtime: System.IO", "System.Runtime: System.Reflection",
        "System.Runtime: System.Runtime.CompilerServices", "System.Runtime: System.Runtime.InteropServices",
        "System.Runtime: System.Runtime.Versioning", "System.Runtime: System.Text", "System.Runtime: System.Threading",
        "System.Runtime: System.Threading.Tasks",
        "System.Collections: System.Collections.Generic", "System.Collections.Concurrent: System.Collections.Concurrent",
        "System.Linq: System.Linq", "System.Memory: System", "System.Memory: System.Buffers.Binary",
        "System.Threading: System.Threading", "System.Threading.Thread: System.Threading",
        "System.Runtime.InteropServices: System.Runtime.InteropServices",
        "System.Text.Json: System.Text.Json", "System.Text.Json: System.Text.Json.Serialization",
        "System.Security.Cryptography: System.Security.Cryptography", "System.Console: System",
        "System.IO.Hashing: System.IO.Hashing",
        "0Harmony: HarmonyLib", "GodotSharp: Godot", "GodotSharp: Godot.Collections",
        "sts2: MegaCrit.Sts2.*"
    };

    // Game methods that call a network, Steam, browser or process API themselves (none reviewed).
    internal static readonly HashSet<string> GameWrappers = new(StringComparer.Ordinal);

    const string Mod = "HumanPlayRecorder.";
    const string Net = "MegaCrit.Sts2.Core.Multiplayer.";
    const string Invoke = "System.Reflection.MethodBase::Invoke";
    const string GetValue = "System.Reflection.PropertyInfo::GetValue";
    const string SetValue = "System.Reflection.PropertyInfo::SetValue";

    // (caller, member) -> exact number of uses.
    internal static readonly Dictionary<(string Caller, string Member), int> Sites = new() {
        // Game multiplayer session sends (the allowed boundary): AI teammates are virtual peers on the
        // host. Each send goes through the game's own multiplayer service to the players in the
        // same game session, or to the host's own handlers, never to any other host or service.
        // AI seat joins / leaves the host lobby (PlayerJoinedMessage, PlayerLeftMessage).
        [(Mod + "MultiplayerAi::Add", Net + "Game.INetGameService::SendMessage")] = 1,
        [(Mod + "MultiplayerAi::Remove", Net + "Game.INetGameService::SendMessage")] = 1,
        // AI seat's merchant purchase to session peers running this mod (AiShopPurchaseMessage).
        [(Mod + "RecorderMod::AiBuy", Net + "Game.INetGameService::SendMessage")] = 1,
        // AI seat's message rebroadcast to session peers with the AI as sender (BroadcastMessage)
        // and delivered to the host's own handlers, exactly as a real peer's message.
        [(Mod + "MultiplayerAi::.cctor", "\"BroadcastMessage\"")] = 1,
        [(Mod + "MultiplayerAi::Submit", Invoke)] = 1,
        [(Mod + "MultiplayerAi::Submit", Net + "NetMessageBus::SendMessageToAllHandlers")] = 1,
        // AI seat's rest-site hover on the reliable channel to connected session peers.
        [(Mod + "RecorderMod::AiSubmitPresence", Net + "Transport.NetHost::SendMessageToClient")] = 1,
        [(Mod + "RecorderMod::AiSubmitPresence", Net + "NetMessageBus::SendMessageToAllHandlers")] = 1,
        // Crystal Sphere curse an AI seat obtained, forwarded to session peers (BroadcastMessage).
        [(Mod + "RecorderMod::AiCrystalCurse", "\"BroadcastMessage\"")] = 1,
        [(Mod + "RecorderMod::AiCrystalCurse", Invoke)] = 1,
        // AI seat's merchant sync messages to session peers without this mod (SendMessageToClientInternal).
        [(Mod + "RecorderMod::.cctor", "\"SendMessageToClientInternal\"")] = 1,
        [(Mod + "RecorderMod::TellVanillaPeers", Invoke)] = 1,

        // Game send methods named only to patch them (Harmony), not to call them.
        // NetHost.SendMessageToClient: drop packets addressed to a virtual AI peer (MultiplayerAi.SendToPeer).
        [(Mod + "MultiplayerAi::Install", "\"SendMessageToClient\"")] = 1,
        [(Mod + "MultiplayerAi::Install", "\"SendToPeer\"")] = 1,
        // NetHostGameService.SendMessage<T>: route an AI seat's reward and gold sync to vanilla peers only.
        [(Mod + "RecorderMod::InstallAiTeammates", "\"SendMessage\"")] = 1,
        // Transport handshake: the vanilla wire guard's patch on the game's own handshake.
        [(Mod + "VanillaGuard.WireTransportBoundary::Install", "\"SendHandshakeMessage\"")] = 1,

        // Reflection invocation of game or Harmony members resolved from typeof(...) and constant names.
        // StartRunLobby.TryAddPlayerInFirstAvailableSlot / HandlePlayerJoinedMessage / HandlePlayerLeftMessage.
        [(Mod + "MultiplayerAi::Add", Invoke)] = 2,
        [(Mod + "MultiplayerAi::Remove", Invoke)] = 1,
        // LoadRunLobby.HandleClientLoadJoinRequestMessage: a saved run's AI seat rejoins.
        [(Mod + "MultiplayerAi::RejoinSavedAi", Invoke)] = 1,
        // HarmonySharedState.GetPatchInfo / UpdatePatchInfo, PatchFunctions.UpdateWrapper (original-rules suspension).
        [(Mod + "VanillaGuard.PatchSuspension::Apply", Invoke)] = 2,
        [(Mod + "VanillaGuard.PatchSuspension::Current", Invoke)] = 1,
        [(Mod + "VanillaGuard.PatchSuspension::HasForeignPatches", Invoke)] = 1,

        // Property accessors read or set through reflection.
        // A card's own declared property, read as the game reads it during play (LiveModelData.Protected).
        [(Mod + "Ai.LiveModelData::Protected", GetValue)] = 1,
        // Rest-site Mend choice state and the run's RNG set (game properties).
        [(Mod + "RecorderMod::AiMendDefault", GetValue)] = 1,
        [(Mod + "RecorderMod::BeforeMendChoice", GetValue)] = 1,
        [(Mod + "ReplayCheckpoints::Rngs", GetValue)] = 1,
        // Vanilla content fingerprint and the wire guard's model cache (game model properties).
        [(Mod + "VanillaContent::ModelHash", GetValue)] = 2,
        [(Mod + "VanillaGuard.WireInterop::CaptureModelCache", GetValue)] = 1,
        [(Mod + "VanillaGuard.WireInterop::ApplyModelCache", SetValue)] = 1,
        [(Mod + "VanillaGuard.WireInterop::Begin", SetValue)] = 2,
        // Tooltip text on game UI controls.
        [(Mod + "MultiplayerAi::ShowTip", SetValue)] = 3,
        [(Mod + "Recommend.Badge::ShowReason", SetValue)] = 3,
        [(Mod + "Settings.ModSettingsUi::Tip", SetValue)] = 3,

        // Type and assembly lookups.
        // sts2's file location, to hash the installed game (ReleaseBootstrap).
        [(Mod + "ReleaseBootstrap::Init", "System.Reflection.Assembly::Load")] = 1,
        // Harmony's HarmonySharedState and PatchFunctions types (constant names, Harmony's own assembly).
        [(Mod + "VanillaGuard.PatchSuspension::.cctor", "System.Reflection.Assembly::GetType")] = 2,
        // NetHost subclasses in the game, this mod and the entry assembly, to patch their sends to AI peers.
        [(Mod + "MultiplayerAi::Install", "System.Reflection.Assembly::GetEntryAssembly")] = 1,
        [(Mod + "MultiplayerAi::Install", "System.Reflection.Assembly::GetTypes")] = 1,
        // Content-pool model types, for the original-rules content scope.
        [(Mod + "VanillaGuard.ContentPools::Install", "System.Reflection.Assembly::GetTypes")] = 1,

        // The bridge resolver: loads sts2_advisor_bridge.dll from the mod folder, nothing else.
        [(Mod + "RecorderMod::InstallAdvisorIntegration", "System.Runtime.InteropServices.NativeLibrary::Load")] = 1,
        [(Mod + "RecorderMod::InstallAdvisorIntegration", "System.Runtime.InteropServices.NativeLibrary::SetDllImportResolver")] = 1,

        // Godot: deferred calls of this mod's own delegates, settings panel layout, and res:// assets.
        [(Mod + "Settings.UiText::Subscribe", "Godot.Callable::CallDeferred")] = 1,
        [(Mod + "VanillaGuard.ContentPools::ShowModeNotice", "Godot.Callable::CallDeferred")] = 1,
        [(Mod + "Settings.ModSettingsUi::AddRows", "Godot.GodotObject::Call")] = 2,
        [(Mod + "Settings.ModSettingsUi::Refresh", "Godot.GodotObject::Call")] = 1,
        [(Mod + "Settings.ModSettingsUi::Refresh", "Godot.GodotObject::CallDeferred")] = 1,
        [(Mod + "MultiplayerAi::AddRemoveButton", "Godot.GD::Load")] = 1,
        [(Mod + "Recommend.Badge::.cctor", "Godot.ResourceLoader::Load")] = 1,
    };
}
