using System.Reflection;
using System.Security.Cryptography;
using Godot;
using HarmonyLib;
using HumanPlayRecorder.Settings;
using MegaCrit.Sts2.addons.mega_text;
using MegaCrit.Sts2.Core.Commands;
using MegaCrit.Sts2.Core.Entities.Multiplayer;
using MegaCrit.Sts2.Core.Helpers;
using MegaCrit.Sts2.Core.Localization.Fonts;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Unlocks;
using MegaCrit.Sts2.Core.HoverTips;
using MegaCrit.Sts2.Core.Localization;
using MegaCrit.Sts2.Core.Nodes.HoverTips;
using MegaCrit.Sts2.Core.Nodes.Multiplayer;
using MegaCrit.Sts2.Core.Multiplayer;
using MegaCrit.Sts2.Core.Multiplayer.Game;
using MegaCrit.Sts2.Core.Multiplayer.Game.Lobby;
using MegaCrit.Sts2.Core.Multiplayer.Messages.Lobby;
using MegaCrit.Sts2.Core.Multiplayer.Serialization;
using MegaCrit.Sts2.Core.Multiplayer.Transport;
using MegaCrit.Sts2.Core.Nodes.Screens.CharacterSelect;
using MegaCrit.Sts2.Core.Platform;
using MegaCrit.Sts2.Core.Runs;

namespace HumanPlayRecorder;

public static class MultiplayerAi
{
    static Task<byte[]>? admissionPolicy;
    static CancellationTokenSource? admissionCancellation;
    internal static bool PolicyAvailable => admissionPolicy?.IsCompletedSuccessfully == true ||
        HumanPlayRecorder.Ai.AiAdvisor.Status == HumanPlayRecorder.Ai.AdvisorStatus.Ready;
    const ulong FirstId = 0x5051504149000001;
    static readonly Lazy<bool> compatible = new(() => {
        using var stream = File.OpenRead(typeof(RunState).Assembly.Location);
        return Convert.ToHexStringLower(SHA256.HashData(stream)) == GameBuild.Sha256;
    });
    static readonly FieldInfo bus = AccessTools.Field(typeof(NetHostGameService), "_messageBus");
    static readonly MethodInfo broadcast = AccessTools.Method(typeof(NetHostGameService), "BroadcastMessage")
        .MakeGenericMethod(typeof(INetMessage));
    static readonly MethodInfo add = AccessTools.Method(typeof(StartRunLobby), "TryAddPlayerInFirstAvailableSlot");
    static readonly MethodInfo joined = AccessTools.Method(typeof(StartRunLobby), "HandlePlayerJoinedMessage");
    static readonly MethodInfo left = AccessTools.Method(typeof(StartRunLobby), "HandlePlayerLeftMessage");
    static readonly MethodInfo loadJoin = AccessTools.Method(typeof(LoadRunLobby), "HandleClientLoadJoinRequestMessage");
    sealed record AiButton(Control Box, NInvitePlayersButton Button, ulong Id);
    static AiButton? aiButton;
    const string RemoveRowName = "PqpAiRemoveRow";
    static readonly HashSet<ulong> aiButtonIds = new();
    static StartRunLobby? lobby;

    public static bool IsAi(ulong id) => id >= FirstId && id < FirstId + 3;
    public static bool HasAi(MegaCrit.Sts2.Core.Runs.IRunState state) => state.Players.Any(p => IsAi(p.NetId));
    internal static bool HostsAi => RunManager.Instance.NetService?.Type == NetGameType.Host &&
        RunManager.Instance.DebugOnlyGetState() is { } state && HasAi(state);

    internal static void Install(Harmony harmony)
    {
        if (!GameBuild.PolicyEnabled) return;
        harmony.Patch(AccessTools.Method(typeof(NCharacterSelectScreen), nameof(NCharacterSelectScreen.InitializeMultiplayerAsHost)),
            postfix: new HarmonyMethod(typeof(MultiplayerAi), nameof(ShowControls)));
        harmony.Patch(AccessTools.Method(typeof(NCharacterSelectScreen), nameof(NCharacterSelectScreen._Process)),
            postfix: new HarmonyMethod(typeof(MultiplayerAi), nameof(RefreshControls)));
        harmony.Patch(AccessTools.Method(typeof(NCharacterSelectScreen), "CleanUpLobby"),
            prefix: new HarmonyMethod(typeof(MultiplayerAi), nameof(HideControls)));
        harmony.Patch(AccessTools.Method(typeof(LoadRunLobby), nameof(LoadRunLobby.AddLocalHostPlayer)),
            postfix: new HarmonyMethod(typeof(MultiplayerAi), nameof(RejoinSavedAi)));
        harmony.Patch(AccessTools.Method(typeof(NCharacterSelectButton), nameof(NCharacterSelectButton.Select)),
            prefix: new HarmonyMethod(typeof(MultiplayerAi), nameof(PickForAi)));
        // The AI buttons are copies of the lobby's own Invite button; only its invite
        // dialog, lobby-driven visibility and map hotkey are withheld from the copies.
        harmony.Patch(AccessTools.PropertyGetter(typeof(NInvitePlayersButton), "Hotkeys"),
            prefix: new HarmonyMethod(typeof(MultiplayerAi), nameof(AiButtonHotkeys)));
        harmony.Patch(AccessTools.Method(typeof(NInvitePlayersButton), "UpdateVisibility"),
            prefix: new HarmonyMethod(typeof(MultiplayerAi), nameof(NotAiButton)));
        harmony.Patch(AccessTools.Method(typeof(NInvitePlayersButton), "OnRelease"),
            prefix: new HarmonyMethod(typeof(MultiplayerAi), nameof(NotAiButton)));
        harmony.Patch(AccessTools.Method(typeof(PlatformUtil), nameof(PlatformUtil.GetPlayerNameRaw)),
            prefix: new HarmonyMethod(typeof(MultiplayerAi), nameof(PlayerName)));
        harmony.Patch(AccessTools.Method(typeof(NetHostGameService), nameof(NetHostGameService.GetVersionInfoForPeer)),
            prefix: new HarmonyMethod(typeof(MultiplayerAi), nameof(PeerVersion)));
        // Lobby code sends per-player notifications too. Virtual peers have no
        // transport socket and must never be passed to Steam/ENet.
        foreach (var type in new[] { typeof(NetHost).Assembly, typeof(MultiplayerAi).Assembly,
            Assembly.GetEntryAssembly() ?? typeof(MultiplayerAi).Assembly }.Distinct()
            .SelectMany(a => a.GetTypes()).Where(type => !type.IsAbstract && typeof(NetHost).IsAssignableFrom(type)))
        {
            if (AccessTools.DeclaredMethod(type, nameof(NetHost.SendMessageToClient)) is { } send)
                harmony.Patch(send, prefix: new HarmonyMethod(typeof(MultiplayerAi), nameof(SendToPeer)));
            if (AccessTools.DeclaredMethod(type, nameof(NetHost.DisconnectClient)) is { } disconnect)
                harmony.Patch(disconnect, prefix: new HarmonyMethod(typeof(MultiplayerAi), nameof(DisconnectPeer)));
        }
        harmony.Patch(AccessTools.Method(typeof(NetHostGameService), nameof(NetHostGameService.DisconnectClient)),
            prefix: new HarmonyMethod(typeof(MultiplayerAi), nameof(DisconnectPeer)));
        OriginalRulesSettings.Changed += ReconcileOriginalRules;
        RecorderMod.InstallAiTeammates(harmony);
    }

    static bool SendToPeer(NetHost __instance, ulong __0, ref byte[] __1, int __2) =>
        !IsAi(__0) && VanillaGuard.WireInterop.AllowPacket(__instance, __0, ref __1, __2);
    // A host disconnect of a virtual seat (the lobby Kick/Ban buttons) has no transport
    // peer; the seat leaves through the lobby's normal leave path instead.
    static bool DisconnectPeer(ulong peerId)
    {
        if (!IsAi(peerId)) return true;
        if (lobby is { } current) Remove(current, peerId);
        return false;
    }
    static int MaxPlayers(StartRunLobby target) =>
        (int)AccessTools.Field(typeof(StartRunLobby), "_maxPlayers").GetValue(target)!;
    static bool NotAiButton(NInvitePlayersButton __instance) => !aiButtonIds.Contains(__instance.GetInstanceId());
    static bool AiButtonHotkeys(NInvitePlayersButton __instance, ref string[] __result)
    {
        if (NotAiButton(__instance)) return true;
        __result = [];
        return false;
    }
    static bool PeerVersion(NetHostGameService __instance, ulong peerId, ref PeerVersionInfo? __result)
    {
        if (!IsAi(peerId)) return true;
        __result = __instance.LocalVersion;
        return false;
    }
    static bool PlayerName(ulong playerId, ref string __result)
    {
        if (!IsAi(playerId)) return true;
        __result = UiText.Get("mp_ai_teammate_name") + " " + (playerId - FirstId + 1);
        return false;
    }

    internal static string? CannotAdd(StartRunLobby target)
    {
        if (!GameBuild.PolicyEnabled) return UiText.Get("compat_policy_paused");
        if (target.NetService.Type != NetGameType.Host ||
            (bool)AccessTools.Field(typeof(StartRunLobby), "_isBeginningRun").GetValue(target)! ||
            target.LocalPlayer.isReady) return UiText.Get("mp_ai_cannot_add_note");
        if (target.Players.Count >= MaxPlayers(target))
            return UiText.Get("mp_ai_cannot_add_lobby_full");
        if (!PolicyAvailable) return UiText.Get(admissionPolicy is { IsCompleted: false }
            ? "ai_status_updating" : "ai_status_unavailable_offline");
        if (!compatible.Value) return UiText.Get("mp_ai_cannot_add_version_mismatch");
        return null;
    }

    internal static bool Add(StartRunLobby target)
    {
        if (CannotAdd(target) is not null) return false;
        ulong id = Enumerable.Range(0, 3).Select(i => FirstId + (ulong)i)
            .First(value => target.Players.All(p => p.id != value));
        var local = target.LocalPlayer;
        var player = (StartRunLobbyPlayer?)add.Invoke(target,
            [local.unlockState, local.maxStandardAscension, local.maxBlitzAscension, true, id]);
        if (player is null) return false;
        // Let the normal join handler own notifications, event subscribers and
        // ascension reconciliation; TryAdd already inserted the slot.
        target.Players.RemoveAll(p => p.id == id);
        // A new seat joins already holding the first unlocked character nobody has yet
        // (else the default). Changing it after the join would shake the freshly created
        // lobby row before it is laid out and leave it off screen.
        var seat = player.Value;
        if (Characters(target).FirstOrDefault(c => target.Players.All(p => p.character != c)) is { } free)
            seat.character = free;
        var message = new PlayerJoinedMessage { lobbyPlayer = seat };
        // Allowed send (offline gate boundary, tests/PackagingCheck/OfflineGate.cs): the game's own
        // multiplayer service, to the players in this game session only; never any other host or URL.
        target.NetService.SendMessage(message);
        joined.Invoke(target, [message, target.NetService.NetId]);
        Submit(target.NetService, id, new LobbyPlayerSetReadyMessage { ready = true });
        return true;
    }

    // The host's unlocked characters, in the character select order.
    static IEnumerable<CharacterModel> Characters(StartRunLobby target)
    {
        var unlocked = UnlockState.FromSerializable(target.LocalPlayer.unlockState);
        return ModelDb.AllCharacters.Where(c => unlocked.Characters.Contains(c) &&
            (!OriginalRulesSettings.Enabled || VanillaGuard.ContentPools.IsOriginalCharacter(c)));
    }

    static void ReconcileOriginalRules()
    {
        if (!OriginalRulesSettings.Enabled || lobby is not { } current ||
            current.NetService.Type != NetGameType.Host) return;
        foreach (var seat in current.Players.Where(p => IsAi(p.id) &&
            !VanillaGuard.ContentPools.IsOriginalCharacter(p.character)).ToList())
        {
            var replacement = Characters(current).FirstOrDefault(c => current.Players.All(p => p.character != c))
                ?? Characters(current).FirstOrDefault();
            if (replacement is not null) SetCharacter(current, seat.id, replacement);
        }
    }

    // An AI seat takes a character through the same lobby message a real player sends.
    internal static bool SetCharacter(StartRunLobby target, ulong id, CharacterModel character)
    {
        if (target.NetService.Type != NetGameType.Host || !IsAi(id) ||
            (bool)AccessTools.Field(typeof(StartRunLobby), "_isBeginningRun").GetValue(target)! ||
            target.Players.All(p => p.id != id) || !Characters(target).Contains(character)) return false;
        Submit(target.NetService, id, new LobbyPlayerChangedCharacterMessage { character = character });
        return true;
    }

    // Right-clicking an AI row steps that seat to the next unlocked character.
    internal static bool NextCharacter(StartRunLobby target, ulong id)
    {
        var all = Characters(target).ToList();
        if (all.Count < 2 || target.Players.FirstOrDefault(p => p.id == id) is not { character: { } current }) return false;
        return SetCharacter(target, id, all[(all.IndexOf(current) + 1) % all.Count]);
    }

    // Left-clicking an AI row starts an AI pick: the row and the character bar get a blue
    // (remote player) frame, and the next character button pressed goes to that AI instead
    // of the host. Left-clicking the row again cancels it.
    const string PickMeta = "pqp_ai_pick", PickFrameName = "PqpAiPickFrame";
    static ulong? pickingFor;
    static NCharacterSelectScreen? screen;
    static void HookCharacterPick(NRemoteLobbyPlayer row)
    {
        if (row.HasMeta(PickMeta)) return;
        row.SetMeta(PickMeta, true);
        row.MouseFilter = Control.MouseFilterEnum.Stop;
        ulong player = row.PlayerId;
        row.GuiInput += input =>
        {
            if (input is not InputEventMouseButton { Pressed: true, ButtonIndex: var button }) return;
            try
            {
                if (button == MouseButton.Left) SetPicking(pickingFor == player ? null : player);
                else if (button != MouseButton.Right || lobby is not { } current || !NextCharacter(current, player)) return;
                SfxCmd.Play("event:/sfx/ui/clicks/ui_click", 1f);
            }
            catch (Exception error) { GD.Print("[HumanPlayRecorder] AI lobby: " + error); }
        };
        row.Connect(Control.SignalName.MouseEntered, Callable.From(() =>
            ShowTip(row, UiText.Get("mp_ai_teammate_name"), UiText.Get("mp_ai_change_character"))));
        row.Connect(Control.SignalName.MouseExited, Callable.From(() => NHoverTipSet.Remove(row)));
    }
    static void SetPicking(ulong? player)
    {
        pickingFor = player;
        if (aiButton is not { } ai || !GodotObject.IsInstanceValid(ai.Box) || ai.Box.GetParent() is not { } column) return;
        foreach (var row in column.GetChildren().OfType<NRemoteLobbyPlayer>())
        {
            var frame = row.GetNodeOrNull<Panel>(PickFrameName);
            if (row.PlayerId != player) { frame?.QueueFree(); continue; }
            if (frame is not null) continue;
            row.AddChild(frame = NewPickFrame());
            frame.SetAnchorsPreset(Control.LayoutPreset.FullRect);
            frame.OffsetLeft = frame.OffsetTop = -6f;
            frame.OffsetRight = frame.OffsetBottom = 6f;
        }
        PlaceBarFrame();
    }
    static Panel NewPickFrame()
    {
        var box = new StyleBoxFlat { DrawCenter = false, BorderColor = StsColors.blue };
        box.SetBorderWidthAll(3);
        box.SetCornerRadiusAll(10);
        var frame = new Panel { Name = PickFrameName, MouseFilter = Control.MouseFilterEnum.Ignore };
        frame.AddThemeStyleboxOverride("panel", box);
        return frame;
    }
    // The character bar's container is laid out by the screen, so its frame lives on the
    // screen itself and follows the buttons' on-screen rectangle every frame.
    static void PlaceBarFrame()
    {
        if (screen is null || !GodotObject.IsInstanceValid(screen)) return;
        var frame = screen.GetNodeOrNull<Panel>(PickFrameName);
        var container = screen.GetNodeOrNull<Control>("CharSelectButtons/ButtonContainer");
        // Layout rectangles, not drawn ones: a hovered button scales up and would shake the frame.
        // Buttons scrolled out of the strip's clip (past its arrow) stay out of the frame.
        var view = screen.GetViewportRect();
        for (var clip = container?.GetParent() as Control; clip is not null && clip != screen; clip = clip.GetParent() as Control)
            if (clip.ClipContents) view = view.Intersection(clip.GetGlobalRect());
        var buttons = container?.GetChildren().OfType<Control>()
            .Where(c => c.Visible && view.Encloses(c.GetGlobalRect()))
            .Select(c => new Rect2(c.Position, c.Size)).ToList();
        if (pickingFor is null || container is null || buttons is not { Count: > 0 }) { frame?.QueueFree(); return; }
        if (frame is null) screen.AddChild(frame = NewPickFrame());
        var local = buttons.Aggregate((a, b) => a.Merge(b));
        var toScreen = screen.GetGlobalTransform().AffineInverse() * container.GetGlobalTransform();
        var rect = (toScreen * local).Grow(10f);
        if (frame.Position != rect.Position) frame.Position = rect.Position;
        if (frame.Size != rect.Size) frame.Size = rect.Size;
    }
    // While an AI pick is open, a character button gives its character to that AI and
    // leaves the host's own selection untouched; locked and random buttons are ignored.
    static bool PickForAi(NCharacterSelectButton __instance)
    {
        if (pickingFor is not { } player || lobby is not { } current) return true;
        if (!__instance.IsLocked && !__instance.IsRandom && SetCharacter(current, player, __instance.Character))
            SetPicking(null);
        return false;
    }

    internal static bool Remove(StartRunLobby target, ulong id)
    {
        if (target.NetService.Type != NetGameType.Host || !IsAi(id) || target.Players.All(p => p.id != id) ||
            (bool)AccessTools.Field(typeof(StartRunLobby), "_isBeginningRun").GetValue(target)!) return false;
        var message = new PlayerLeftMessage { playerId = id };
        // Allowed send (offline gate boundary): the game's multiplayer service, this session's players only.
        target.NetService.SendMessage(message);
        left.Invoke(target, [message, target.NetService.NetId]);
        return true;
    }

    // A saved run keeps its AI seats; without a transport peer they would never send the
    // load-join request, and the resume lobby would wait for them forever. They rejoin
    // through the lobby's own join handler and ready up like a real peer.
    static void RejoinSavedAi(LoadRunLobby __instance)
    {
        if (__instance.NetService.Type != NetGameType.Host || !compatible.Value) return;
        foreach (ulong id in __instance.Run.Players.Select(p => p.NetId).Where(IsAi).ToList())
        {
            if (__instance.Players.Any(p => p.id == id)) continue;
            loadJoin.Invoke(__instance, [new ClientLoadJoinRequestMessage(), id]);
            Submit(__instance.NetService, id, new LobbyPlayerSetReadyMessage { ready = true });
        }
    }

    // Exactly the receive path of a real peer: rebroadcast with the original
    // sender, then enter the host's normal message/location/synchronizer pipeline.
    // Do not spoof LocalContext or replace the human's service-wide NetId.
    internal static void Submit(INetGameService service, ulong player, INetMessage message)
    {
        if (service is not NetHostGameService host || !IsAi(player))
            throw new InvalidOperationException("AI inputs require a host-owned virtual peer");
        // Allowed sends (offline gate boundary): the game's BroadcastMessage to this session's
        // players and the host's own message bus; never any other host or URL.
        if (message.ShouldBroadcast)
            broadcast.Invoke(host, [message, player, message.Mode.ToChannelId(), player]);
        ((NetMessageBus)bus.GetValue(host)!).SendMessageToAllHandlers(message, player);
    }

    static void ShowControls(NCharacterSelectScreen __instance)
    {
        HideControls();
        lobby = __instance.Lobby;
        screen = __instance;
        admissionCancellation = new CancellationTokenSource();
        admissionPolicy = RecorderMod.PreloadAiPolicy(admissionCancellation.Token);
        if (FindInvite(__instance) is not { } invite || invite.GetParent() is not Control inviteBox ||
            inviteBox.GetParent() is not Control column)
        {
            GD.Print("[HumanPlayRecorder] AI lobby: the lobby Invite button was not found");
            return;
        }
        var box = (Control)inviteBox.Duplicate();
        var button = FindInvite(box)!;
        // The hover shader parameters are per button, so the copy owns its material.
        if (button.GetNodeOrNull<Control>("Background") is { Material: { } material } background)
            background.Material = (Material)material.Duplicate();
        ulong id = button.GetInstanceId();
        aiButtonIds.Add(id);
        column.AddChild(box);
        button.GetNode<MegaRichTextLabel>("Label").SetTextAutoSize(UiText.Get("mp_add_ai_button"));
        button.Released += _ => {
            try { if (lobby is { } current) Add(current); }
            catch (Exception error) { GD.Print("[HumanPlayRecorder] AI lobby: " + error); }
        };
        button.Connect(Control.SignalName.MouseEntered, Callable.From(() =>
        {
            string? reason = lobby is { } current ? CannotAdd(current) : null;
            string note = UiText.Get("mp_ai_local_only_note");
            ShowTip(button, UiText.Get("mp_add_ai_button"), reason is null ? note : reason + "\n\n" + note);
        }));
        button.Connect(Control.SignalName.MouseExited, Callable.From(() => NHoverTipSet.Remove(button)));
        aiButton = new(box, button, id);
        RefreshControls();
    }
    // Without sts2_matchmaker an AI row gets its own "-" button, built and placed exactly
    // like that mod's red Kick button (Sts2ModalPanel.BuildTextActionButton, height 50,
    // #991816, row offset 200/9) so the lobby looks the same with or without it.
    const float RemoveHeight = 50f;
    static void AddRemoveButton(NRemoteLobbyPlayer row)
    {
        var holder = new HBoxContainer { Name = RemoveRowName };
        holder.SetAnchorsPreset(Control.LayoutPreset.TopLeft);
        holder.OffsetLeft = 200f;
        holder.OffsetTop = 9f;
        var color = new Color("991816");
        StyleBoxFlat Box(Color background)
        {
            var box = new StyleBoxFlat { BgColor = background, ShadowColor = new Color(0, 0, 0, .5f),
                ShadowSize = Mathf.RoundToInt(RemoveHeight * .12f),
                ShadowOffset = new Vector2(0, Mathf.RoundToInt(RemoveHeight * .06f)),
                ContentMarginLeft = RemoveHeight * .3f, ContentMarginRight = RemoveHeight * .3f,
                ContentMarginTop = RemoveHeight * .08f, ContentMarginBottom = RemoveHeight * .08f };
            box.SetCornerRadiusAll(Mathf.RoundToInt(RemoveHeight * .2f));
            return box;
        }
        var hover = Box(Color.FromHsv(color.H, Mathf.Min(1, color.S * 1.1f), Mathf.Min(1, color.V * 1.1f)));
        var button = new Button();
        button.AddThemeStyleboxOverride("normal", Box(color));
        button.AddThemeStyleboxOverride("hover", hover);
        button.AddThemeStyleboxOverride("pressed", Box(Color.FromHsv(color.H, color.S, color.V * .85f)));
        button.AddThemeStyleboxOverride("focus", hover);
        int size = Mathf.RoundToInt(RemoveHeight * .4f), shadow = Mathf.RoundToInt(RemoveHeight * .04f);
        var label = new Label { Text = "-", HorizontalAlignment = HorizontalAlignment.Center,
            VerticalAlignment = VerticalAlignment.Center, MouseFilter = Control.MouseFilterEnum.Ignore };
        label.SetAnchorsPreset(Control.LayoutPreset.FullRect);
        label.AddThemeColorOverride("font_color", StsColors.cream);
        label.AddThemeColorOverride("font_outline_color", color.Darkened(.6f));
        label.AddThemeConstantOverride("outline_size", Mathf.RoundToInt(RemoveHeight * .12f));
        label.AddThemeColorOverride("font_shadow_color", new Color(0, 0, 0, .6f));
        label.AddThemeConstantOverride("shadow_offset_x", shadow);
        label.AddThemeConstantOverride("shadow_offset_y", shadow);
        label.AddThemeConstantOverride("shadow_outline_size", Mathf.RoundToInt(RemoveHeight * .1f));
        label.AddThemeFontSizeOverride("font_size", size);
        var font = FontManager.GetSubstituteFont(LocManager.Instance?.Language ?? "", (FontType)1)
            ?? GD.Load<Font>("res://themes/kreon_bold_glyph_space_one.tres");
        if (font is not null) label.AddThemeFontOverride("font", font);
        button.AddChild(label);
        FitRemoveButton(button, label);
        ulong player = row.PlayerId;
        button.Pressed += () => {
            try
            {
                SfxCmd.Play("event:/sfx/ui/clicks/ui_click", 1f);
                if (lobby is { } current) Remove(current, player);
            }
            catch (Exception error) { GD.Print("[HumanPlayRecorder] AI lobby: " + error); }
        };
        holder.AddChild(button);
        row.AddChild(holder);
    }
    // sts2_matchmaker's UpdateTextActionButtonText: the button hugs its label.
    static void FitRemoveButton(Button button, Label label)
    {
        float height = button.CustomMinimumSize.Y > 0 ? button.CustomMinimumSize.Y : RemoveHeight;
        var font = label.GetThemeFont("font") ?? ThemeDB.FallbackFont;
        float width = font.GetStringSize(label.Text, HorizontalAlignment.Left, -1,
            label.GetThemeFontSize("font_size")).X + height * .6f;
        var size = new Vector2(width, height);
        if (button.CustomMinimumSize != size) button.CustomMinimumSize = size;
    }
    static NInvitePlayersButton? FindInvite(Node root)
    {
        if (root is NInvitePlayersButton found) return found;
        foreach (var child in root.GetChildren())
            if (FindInvite(child) is { } inner) return inner;
        return null;
    }
    static void ShowTip(Control owner, string title, string description)
    {
        NHoverTipSet.CreateAndShow(owner, Settings.ModHoverTip.Create(title, description, "pqp_ai_lobby"))
            ?.SetGlobalPosition(owner.GlobalPosition + new Vector2(owner.Size.X + 20f, 0f));
    }
    static void RefreshControls()
    {
        if (lobby is null || aiButton is not { } ai || !GodotObject.IsInstanceValid(ai.Box) ||
            ai.Box.GetParent() is not { } column) return;
        if (pickingFor is { } picked && lobby.Players.All(p => p.id != picked)) SetPicking(null);
        PlaceBarFrame();
        foreach (var row in column.GetChildren().OfType<NRemoteLobbyPlayer>().Where(r => IsAi(r.PlayerId)))
        {
            HookCharacterPick(row);
            var own = row.GetNodeOrNull<Control>(RemoveRowName);
            // sts2_matchmaker gives every row Kick then Ban; an AI seat keeps only Kick, shown
            // as "-" (its host disconnect becomes the seat's leave, see DisconnectPeer).
            if (row.GetNodeOrNull<Container>("Sts2MatchmakerModerationRow")?.GetChildren().OfType<Button>().ToArray()
                is [var kick, .. var others] && kick.GetChildren().OfType<Label>().FirstOrDefault() is { } label)
            {
                if (label.Text != "-") { label.Text = "-"; FitRemoveButton(kick, label); }
                foreach (var other in others) other.Visible = false;
                own?.QueueFree();
            }
            else if (own is null) AddRemoveButton(row);
        }
        // Player rows join the column after the lobby opens; keep the AI button below them.
        if (ai.Box.GetIndex() != column.GetChildCount() - 1)
            column.MoveChild(ai.Box, -1);
        // A full lobby has no seat to fill, and one more row would wrap the column.
        ai.Box.Visible = lobby.Players.Count < MaxPlayers(lobby);
        bool enabled = CannotAdd(lobby) is null;
        if (ai.Button.IsEnabled != enabled) ai.Button.SetEnabled(enabled);
        ai.Box.Modulate = enabled ? Colors.White : new Color(1, 1, 1, .5f);
    }
    static void HideControls()
    {
        admissionCancellation?.Cancel();
        admissionCancellation?.Dispose();
        admissionCancellation = null;
        if (aiButton is { } ai)
        {
            aiButtonIds.Remove(ai.Id);
            if (GodotObject.IsInstanceValid(ai.Box)) ai.Box.QueueFree();
        }
        aiButton = null;
        pickingFor = null;
        screen = null;
        lobby = null;
    }
}
