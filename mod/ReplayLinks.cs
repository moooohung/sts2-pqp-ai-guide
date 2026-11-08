using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Godot;
using MegaCrit.Sts2.Core.Localization.Fonts;
using MegaCrit.Sts2.Core.Nodes.CommonUi;
using MegaCrit.Sts2.Core.Nodes.GodotExtensions;
using MegaCrit.Sts2.addons.mega_text;
using MegaCrit.Sts2.Core.Nodes.Screens.MainMenu;

namespace HumanPlayRecorder;

public sealed record ReplayConsentState(bool asked, bool public_replay);

// Replays on sts.pqp.me. The install keeps a random private key; receipts carry only its
// SHA-256 (owner), so "내 리플레이" (/me#k=<key>) lists this install's runs without any Steam
// identity. Runs appear in the public list only after the player chose "공개".
public static class ReplayLinks
{
    const string Site = "https://sts.pqp.me";
    static readonly string Root = Path.Combine(
        System.Environment.GetFolderPath(System.Environment.SpecialFolder.LocalApplicationData), "HumanPlayRecorder");
    static readonly string KeyPath = Path.Combine(Root, "replay-key.txt");
    static readonly string ConsentPath = Path.Combine(Root, "replay-consent.json");
    static readonly object sync = new();
    static string? key;

    // 32 random bytes, base64url without padding (43 chars): the shape /me#k= accepts.
    public static string Key
    {
        get
        {
            lock (sync)
            {
                if (key is not null) return key;
                try
                {
                    if (File.Exists(KeyPath))
                    {
                        string saved = File.ReadAllText(KeyPath).Trim();
                        if (ValidKey(saved)) return key = saved;
                    }
                    string created = Base64Url(System.Security.Cryptography.RandomNumberGenerator.GetBytes(32));
                    Directory.CreateDirectory(Root);
                    string temp = KeyPath + ".tmp";
                    File.WriteAllText(temp, created);
                    File.Move(temp, KeyPath, overwrite: true);
                    // Only a key that reads back from disk is used: an upload owned by a key that
                    // is lost with the session could never be deleted.
                    if (File.ReadAllText(KeyPath).Trim() != created) throw new IOException("key not durable");
                    return key = created;
                }
                catch (Exception e) when (e is IOException or UnauthorizedAccessException)
                {
                    // No durable key: nothing is uploaded (Stamp fails closed) and the next call tries again.
                    GD.Print("[HumanPlayRecorder] replay key unavailable: " + e.GetType().Name);
                    return "";
                }
            }
        }
    }

    public static bool ValidKey(string k) =>
        k.Length == 43 && k.All(c => c is (>= 'A' and <= 'Z') or (>= 'a' and <= 'z') or (>= '0' and <= '9') or '-' or '_');
    static string Base64Url(byte[] bytes) => Convert.ToBase64String(bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_');

    public static string OwnerOf(string k) => Convert.ToHexStringLower(SHA256.HashData(Encoding.ASCII.GetBytes(k)));
    public static string? Owner => Key is { Length: > 0 } k ? OwnerOf(k) : null;
    public static string MyReplaysUrl(string k) => Site + "/me#k=" + k;
    public static string ReplayUrl(string sha256, bool isPublic, string k) =>
        isPublic ? Site + "/r/" + sha256[..16] : Site + "/me?replay=" + sha256[..16] + "#k=" + k;

    public static ReplayConsentState Consent()
    {
        try
        {
            if (File.Exists(ConsentPath))
                return JsonSerializer.Deserialize<ReplayConsentState>(File.ReadAllBytes(ConsentPath)) ?? new(false, false);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or JsonException)
        {
            GD.Print("[HumanPlayRecorder] replay consent unreadable: " + e.GetType().Name);
        }
        return new(false, false);
    }

    static void SaveConsent(bool isPublic)
    {
        try
        {
            Directory.CreateDirectory(Root);
            string temp = ConsentPath + ".tmp";
            File.WriteAllBytes(temp, JsonSerializer.SerializeToUtf8Bytes(new ReplayConsentState(true, isPublic)));
            File.Move(temp, ConsentPath, overwrite: true);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            GD.Print("[HumanPlayRecorder] replay consent not saved: " + e.GetType().Name);
        }
    }

    // Receipt fields for a new upload: the owner (when the key exists), public consent only when given
    // and the replay is saved, and the explicit training / replay-saving flags.
    // Null while there is no durable install key: an upload nobody can delete is not made.
    public static UploadReceipt? Stamp(UploadReceipt receipt) =>
        UploadPolicy.Stamp(receipt, Owner, Consent().public_replay,
            Settings.SharingSettings.TrainingConsent, Settings.SharingSettings.ReplaySaving);

    internal const string ConsentText =
        "기록한 A10 런은 sts.pqp.me 리플레이 탭에서 다시 볼 수 있습니다.\n\n" +
        "공개하면 끝난 판이 누구나 보는 '사람 플레이' 목록에 올라갑니다.\n" +
        "이름·Steam ID는 표시되지 않습니다.\n\n" +
        "나만 보기를 고르면 메인 메뉴의 '내 리플레이' 버튼으로 나만 볼 수 있습니다.\n" +
        "이 설정은 같은 버튼에서 언제든 바꿀 수 있습니다.";

    // Main thread. Asked once; the main-menu button can change it later.
    internal static void OfferConsent()
    {
        if (Settings.SharingSettings.ReplaySaving && (!Consent().asked || DebugUiCapture.Active)) ShowConsent();
    }

#if DEBUG
    // Screenshot harness only: the replay menu and the upload note, without opening a browser.
    internal static void DebugPreview()
    {
        ShowMenu();
        // A search capture is the list alone: the corner note would cover part of it.
        if (System.Environment.GetEnvironmentVariable("HPR_UI_CAPTURE_REPLAY_SEARCH") is { Length: > 0 }) return;
        FirstLaunchNotice.ShowShortNote("리플레이가 올라갔습니다",
            "방금 기록한 런을 sts.pqp.me에서 다시 볼 수 있습니다.\n서버가 기록을 받는 데 1~2분 걸릴 수 있습니다.",
            new("리플레이 보기", () => { }), closeAfterSeconds: 20);
    }
#endif

    static void ShowConsent() =>
        GamePopup.Enqueue("리플레이 공개", ConsentText, 760,
            new("공개", () => SaveConsent(true)),
            new("나만 보기", () => SaveConsent(false)),
            debugName: "replay-consent");

    // Adds "내 리플레이" to the game's own menu buttons, cloned from the settings button so it
    // has the same font, colours, hover and click sounds. Called after every NMainMenu._Ready.
    internal static void AddMenuButton(NMainMenu menu)
    {
        try
        {
            var settings = menu.GetNode<NMainMenuTextButton>("MainMenuTextButtons/SettingsButton");
            var parent = settings.GetParent();
            if (parent.HasNode("HumanPlayRecorderReplays")) return;
            // No signal copies: the clone must not open the settings menu.
            var button = (NMainMenuTextButton)settings.Duplicate(
                (int)(Node.DuplicateFlags.Groups | Node.DuplicateFlags.Scripts | Node.DuplicateFlags.UseInstantiation));
            button.Name = "HumanPlayRecorderReplays";
            parent.AddChild(button);
            // Right under 설정: other mods append buttons, which can push the bottom of the list off screen.
            parent.MoveChild(button, settings.GetIndex() + 1);
            if (button.label is { } label)
            {
                label.Text = "내 리플레이";
                if (FontManager.GetSubstituteFont("kor", FontType.Regular) is { } font)
                    label.AddThemeFontOverride(ThemeConstants.Label.Font, font);
            }
            button.Connect(NClickableControl.SignalName.Released, Callable.From<NButton>(_ => ShowMenu()));
        }
        catch (Exception e)
        {
            GD.Print("[HumanPlayRecorder] replay menu button unavailable: " + e.GetType().Name);
        }
    }

    // One row of /api/human-replays?owner=: never the owner or the full hash. cards holds each card
    // of the final deck once, relics are in pickup order, killer the enemies alive at death.
    public sealed record ReplayRow(string id, string character, int party_size, int ascension,
        string end, int act, int floor, string ended_at, bool @public,
        int deck_size = 0, string[]? cards = null, string[]? relics = null, string[]? killer = null,
        string[]? deck = null);

    static readonly System.Net.Http.HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(8) };

    static async Task<IReadOnlyList<ReplayRow>> FetchRows(string owner)
    {
#if DEBUG
        // Layout check only: sample rows from a local file instead of the server.
        string sample = Path.Combine(Root, "debug-replay-list.json");
        if (File.Exists(sample)) return JsonSerializer.Deserialize<ReplayRow[]>(File.ReadAllBytes(sample)) ?? [];
#endif
        using var response = await Http.GetAsync(Site + "/api/human-replays?owner=" + owner);
        response.EnsureSuccessStatusCode();
        return await JsonSerializer.DeserializeAsync<ReplayRow[]>(await response.Content.ReadAsStreamAsync()) ?? [];
    }

    // The server checks the key itself (its hash only reads); true when the change was saved.
    static async Task<bool> SetPublic(string id, bool isPublic)
    {
        if (Key is not { Length: > 0 } k) return false;
        using var body = new System.Net.Http.StringContent(
            JsonSerializer.Serialize(new { id, key = k, @public = isPublic }), Encoding.UTF8, "application/json");
        using var response = await Http.PostAsync(Site + "/api/human-replay/visibility", body);
        return response.IsSuccessStatusCode;
    }

    // Same endpoint and owner-key check as SetPublic. The server forgets the receipt and keeps only a
    // tombstone of its hash; a copy already on Catbox is not ours to remove.
    static async Task<bool> Forget(object request)
    {
        if (Key is not { Length: > 0 }) return false;
        using var body = new System.Net.Http.StringContent(JsonSerializer.Serialize(request), Encoding.UTF8, "application/json");
        using var response = await Http.PostAsync(Site + "/api/human-replay/visibility", body);
        return response.IsSuccessStatusCode;
    }
    static Task<bool> DeleteReplay(string id) => Forget(new { id, key = Key, delete = true });
    // Nothing queued or in flight on this PC may leave it once the player asked to delete everything, whatever the
    // server answers: the local upload queues are purged first and refuse every run that began before now. The
    // server gets the same moment as its cutoff, so a receipt already in flight cannot bring the data back.
    internal static async Task<bool> DeleteAllMine()
    {
        var at = DateTimeOffset.UtcNow;
        await RecorderMod.PurgeUploadsAsync(at);
        return await Forget(new { key = Key, delete_all = true, before = at.ToUnixTimeSeconds() });
    }

    static void OnMain(Action action) => Callable.From(action).CallDeferred();

    static Label Text(string text, int size, Color? color = null)
    {
        var label = Plain(text, size, color);
        label.AutowrapMode = TextServer.AutowrapMode.WordSmart;
        label.SizeFlagsHorizontal = Control.SizeFlags.ExpandFill;
        return label;
    }

    // One unwrapped line sized to its text (tags, numbers, names beside an image).
    static Label Plain(string text, int size, Color? color = null)
    {
        var label = new Label { Text = text, VerticalAlignment = VerticalAlignment.Center,
            MouseFilter = Control.MouseFilterEnum.Ignore };
        if (GamePopup.KoreanFont is { } font) label.AddThemeFontOverride(ThemeConstants.Label.Font, font);
        label.AddThemeFontSizeOverride(ThemeConstants.Label.FontSize, size);
        if (color is { } c) label.AddThemeColorOverride(ThemeConstants.Label.FontColor, c);
        return label;
    }

    static StyleBoxFlat Box(Color fill, Color? border = null, int radius = 8, int margin = 8)
    {
        var box = new StyleBoxFlat { BgColor = fill };
        box.SetCornerRadiusAll(radius);
        box.SetContentMarginAll(margin);
        if (border is { } b) { box.BorderColor = b; box.BorderWidthLeft = 4; }
        return box;
    }

    static Control Tag(string text, Color fill)
    {
        var tag = new PanelContainer { SizeFlagsVertical = Control.SizeFlags.ShrinkCenter };
        var box = Box(fill, radius: 6, margin: 2);
        box.ContentMarginLeft = box.ContentMarginRight = 10;
        tag.AddThemeStyleboxOverride("panel", box);
        tag.AddChild(Plain(text, 17, Colors.White));
        return tag;
    }

    // A game image; a missing texture keeps the slot and its tooltip so rows stay aligned.
    static Control Image(Texture2D? texture, Vector2 size, string tooltip, bool cover = false) => new TextureRect
    {
        Texture = texture, CustomMinimumSize = size, TooltipText = tooltip, ClipContents = cover,
        ExpandMode = TextureRect.ExpandModeEnum.IgnoreSize, MouseFilter = Control.MouseFilterEnum.Pass,
        StretchMode = cover ? TextureRect.StretchModeEnum.KeepAspectCovered : TextureRect.StretchModeEnum.KeepAspectCentered,
        SizeFlagsVertical = Control.SizeFlags.ShrinkCenter,
    };

    // Names and pictures of the ids a row holds, read from the game's own models (mod content
    // included) and cached: search compares every row's names on each keystroke.
    static class Art
    {
        static Dictionary<string, MegaCrit.Sts2.Core.Models.CardModel>? cards;
        static Dictionary<string, MegaCrit.Sts2.Core.Models.RelicModel>? relics;
        static Dictionary<string, MegaCrit.Sts2.Core.Models.CharacterModel>? characters;
        static Dictionary<string, MegaCrit.Sts2.Core.Models.MonsterModel>? monsters;
        static readonly Dictionary<string, string> names = new();
        static readonly Dictionary<string, Texture2D?> pictures = new();

        static Dictionary<string, T> Index<T>(IEnumerable<T> all) where T : MegaCrit.Sts2.Core.Models.AbstractModel
        {
            var map = new Dictionary<string, T>();
            try { foreach (var model in all) map.TryAdd(model.Id.Entry, model); }
            catch (Exception e) { GD.Print("[HumanPlayRecorder] replay list models: " + e.GetType().Name); }
            return map;
        }
        static Dictionary<string, MegaCrit.Sts2.Core.Models.CardModel> Cards => cards ??= Index(MegaCrit.Sts2.Core.Models.ModelDb.AllCards);
        static Dictionary<string, MegaCrit.Sts2.Core.Models.RelicModel> Relics => relics ??= Index(MegaCrit.Sts2.Core.Models.ModelDb.AllRelics);
        static Dictionary<string, MegaCrit.Sts2.Core.Models.CharacterModel> Characters => characters ??= Index(MegaCrit.Sts2.Core.Models.ModelDb.AllCharacters);
        static Dictionary<string, MegaCrit.Sts2.Core.Models.MonsterModel> Monsters => monsters ??= Index(MegaCrit.Sts2.Core.Models.ModelDb.Monsters);

        static string Cached(string kind, string id, Func<string?> read)
        {
            string cacheKey = kind + "/" + id;
            if (names.TryGetValue(cacheKey, out var name)) return name;
            try { name = read(); } catch (Exception) { name = null; }
            return names[cacheKey] = string.IsNullOrWhiteSpace(name) ? id : name;
        }
        static Texture2D? Picture(string kind, string id, Func<Texture2D?> read)
        {
            string cacheKey = kind + "/" + id;
            if (pictures.TryGetValue(cacheKey, out var texture)) return texture;
            try { texture = read(); } catch (Exception) { texture = null; }
            return pictures[cacheKey] = texture;
        }

        public static string Card(string id) => Cached("card", id, () => Cards.TryGetValue(id, out var c) ? c.Title : null);
        public static string Relic(string id) => Cached("relic", id, () => Relics.TryGetValue(id, out var r) ? r.Title.GetFormattedText() : null);
        public static string Monster(string id) => Cached("monster", id, () => Monsters.TryGetValue(id, out var m) ? m.Title.GetFormattedText() : null);
        public static string Character(string id) => Cached("character", id, () => Characters.TryGetValue(id, out var c)
            ? c.Title.GetFormattedText()
            : new MegaCrit.Sts2.Core.Localization.LocString("characters", id + ".title").GetFormattedText());
        public static Texture2D? CardPicture(string id) => Picture("card", id, () => Cards.TryGetValue(id, out var c) ? c.Portrait : null);
        public static Texture2D? RelicPicture(string id) => Picture("relic", id, () => Relics.TryGetValue(id, out var r) ? r.Icon : null);
        public static Texture2D? CharacterPicture(string id) => Picture("character", id, () => Characters.TryGetValue(id, out var c) ? c.IconTexture : null);

        // Fresh models for the game's own card and relic views. A deck entry is "ID" or "ID+N"
        // (upgraded N times, applied the way a save is loaded); ids this install lacks are left out.
        public static List<MegaCrit.Sts2.Core.Models.CardModel> DeckModels(IEnumerable<string> deck)
        {
            var result = new List<MegaCrit.Sts2.Core.Models.CardModel>();
            foreach (string entry in deck)
            {
                string id = entry; int level = 0;
                int plus = entry.IndexOf('+');
                if (plus > 0 && int.TryParse(entry[(plus + 1)..], out level)) id = entry[..plus];
                if (!Cards.TryGetValue(id, out var canonical)) continue;
                try
                {
                    var card = canonical.ToMutable();
                    for (; level > 0 && card.IsUpgradable; level--) { card.UpgradeInternal(); card.FinalizeUpgradeInternal(); }
                    result.Add(card);
                }
                catch (Exception e) { GD.Print("[HumanPlayRecorder] replay deck card " + id + ": " + e.GetType().Name); }
            }
            return result;
        }

        public static List<MegaCrit.Sts2.Core.Models.RelicModel> RelicModels(IEnumerable<string> relics)
        {
            var result = new List<MegaCrit.Sts2.Core.Models.RelicModel>();
            foreach (string id in relics)
            {
                if (!Relics.TryGetValue(id, out var canonical)) continue;
                try { result.Add(canonical.ToMutable()); }
                catch (Exception e) { GD.Print("[HumanPlayRecorder] replay relic " + id + ": " + e.GetType().Name); }
            }
            return result;
        }
    }

    static void Clicked(Control control, Action action)
    {
        control.MouseFilter = Control.MouseFilterEnum.Stop;
        control.MouseDefaultCursorShape = Control.CursorShape.PointingHand;
        control.GuiInput += input =>
        {
            if (input is InputEventMouseButton { ButtonIndex: MouseButton.Left, Pressed: false }) action();
        };
    }

    // The game's inspect screens sit below the modal layer, so the replay popup steps aside
    // while one is open and comes back when it closes (Close() ends with Visible = false).
    static void AboveModal(Control screen)
    {
        if (NModalContainer.Instance is not { } modal || !modal.Visible) return;
        modal.Visible = false;
        void Restore()
        {
            if (screen.Visible) return;
            screen.VisibilityChanged -= Restore;
            if (GodotObject.IsInstanceValid(modal)) modal.Visible = true;
        }
        screen.VisibilityChanged += Restore;
    }

    // The game's large relic view, browsing this run's relics from the one clicked.
    static void InspectRelic(string[] relics, int index)
    {
        try
        {
            var models = Art.RelicModels(relics);
            if (models.Count == 0) return;
            var clicked = Art.RelicModels([relics[index]]).FirstOrDefault();
            int at = clicked is null ? 0 : Math.Max(0, models.FindIndex(r => r.Id == clicked.Id));
            if (MegaCrit.Sts2.Core.Nodes.NGame.Instance?.GetInspectRelicScreen() is not { } screen) return;
            screen.Open(models, models[at]);
            AboveModal(screen);
        }
        catch (Exception e) { GD.Print("[HumanPlayRecorder] relic view unavailable: " + e); }
    }

    // A run's final deck as real game cards; a click opens the game's large card view.
    static Control DeckGrid(List<MegaCrit.Sts2.Core.Models.CardModel> cards)
    {
        const float scale = 0.5f;
        var size = MegaCrit.Sts2.Core.Nodes.Cards.NCard.defaultSize * scale;
        var grid = new GridContainer { Columns = 5, SizeFlagsHorizontal = Control.SizeFlags.ShrinkCenter };
        grid.AddThemeConstantOverride("h_separation", 12);
        grid.AddThemeConstantOverride("v_separation", 12);
        // Ready, not TreeEntered: a card added before its parent is ready skips its own _Ready,
        // and NCard.UpdateVisuals does nothing until then (title "Broken Card").
        grid.Ready += () =>
        {
            for (int i = 0; i < cards.Count; i++)
            {
                int index = i;
                var holder = new Control { CustomMinimumSize = size, MouseFilter = Control.MouseFilterEnum.Pass };
                grid.AddChild(holder);
                try
                {
                    if (MegaCrit.Sts2.Core.Nodes.Cards.NCard.Create(cards[i]) is { } node)
                    {
                        holder.AddChild(node);
                        node.Scale = Vector2.One * scale;
                        node.Position = size / 2;
                        node.MouseFilter = Control.MouseFilterEnum.Ignore;
                        node.UpdateVisuals(MegaCrit.Sts2.Core.Entities.Cards.PileType.None,
                            MegaCrit.Sts2.Core.Entities.Cards.CardPreviewMode.Normal);
                    }
                }
                catch (Exception e) { GD.Print("[HumanPlayRecorder] replay deck card view: " + e.GetType().Name); }
                var hit = new Button { Flat = true, FocusMode = Control.FocusModeEnum.None, TooltipText = ReplayListText.TooltipCard,
                    MouseDefaultCursorShape = Control.CursorShape.PointingHand };
                hit.SetAnchorsPreset(Control.LayoutPreset.FullRect);
                hit.AddThemeStyleboxOverride("normal", new StyleBoxEmpty());
                hit.AddThemeStyleboxOverride("hover", new StyleBoxEmpty());
                hit.AddThemeStyleboxOverride("pressed", new StyleBoxEmpty());
                hit.Pressed += () =>
                {
                    // GetInspect*Screen creates the screen on first use; the property stays null until then.
                    try
                    {
                        if (MegaCrit.Sts2.Core.Nodes.NGame.Instance?.GetInspectCardScreen() is not { } screen) return;
                        screen.Open(cards, index);
                        AboveModal(screen);
                    }
                    catch (Exception e) { GD.Print("[HumanPlayRecorder] card view unavailable: " + e); }
                };
                holder.AddChild(hit);
            }
        };
        return grid;
    }

    static Button RowButton(string text)
    {
        var button = new Button { Text = text, FocusMode = Control.FocusModeEnum.None,
            CustomMinimumSize = new Vector2(96, 36) };
        if (GamePopup.KoreanFont is { } font) button.AddThemeFontOverride("font", font);
        button.AddThemeFontSizeOverride("font_size", 18);
        return button;
    }

    static readonly Color Muted = new(0.8f, 0.78f, 0.7f), Gold = new(0.95f, 0.8f, 0.4f);
    const int RelicStrip = 16;

    // Search ignores case, spaces and underscores, so "몸통박치기" finds "몸통 박치기" and ids match too.
    static string Fold(string s) => new string(s.Where(c => !char.IsWhiteSpace(c) && c != '_').ToArray()).ToLowerInvariant();
    static bool Hit(string name, string id, string folded) => Fold(name).Contains(folded) || Fold(id).Contains(folded);

    sealed record Shown(ReplayRow Row, string[] Cards, string[] Relics);

    static Shown? Match(ReplayRow row, string folded)
    {
        if (folded.Length == 0) return new(row, [], []);
        var cards = (row.cards ?? []).Where(id => Hit(Art.Card(id), id, folded)).ToArray();
        var relics = (row.relics ?? []).Distinct().Where(id => Hit(Art.Relic(id), id, folded)).ToArray();
        return cards.Length + relics.Length > 0 ? new(row, cards, relics) : null;
    }

    static HBoxContainer HRow(int gap)
    {
        var box = new HBoxContainer { MouseFilter = Control.MouseFilterEnum.Pass };
        box.AddThemeConstantOverride("separation", gap);
        return box;
    }

    // Relic strips and search matches wrap instead of widening the popup.
    static HFlowContainer Flow(int gap)
    {
        var flow = new HFlowContainer { MouseFilter = Control.MouseFilterEnum.Pass };
        flow.AddThemeConstantOverride("h_separation", gap);
        flow.AddThemeConstantOverride("v_separation", 4);
        return flow;
    }

    // One run as a card like the site's finished-run rows: party portraits, result, act and floor,
    // what ended it, the relic strip and deck size; a search adds the matching cards and relics.
    static Control Row(Shown shown, string k, Action<ReplayRow> showDeck)
    {
        var row = shown.Row;
        bool won = row.end == "win";
        var card = new PanelContainer { SizeFlagsHorizontal = Control.SizeFlags.ExpandFill };
        card.AddThemeStyleboxOverride("panel", Box(new Color(0, 0, 0, 0.3f),
            won ? new Color(0.45f, 0.8f, 0.4f) : new Color(0.85f, 0.35f, 0.3f)));
        var line = HRow(12);
        card.AddChild(line);

        string[] party = row.character.Split('+');
        var faces = HRow(2);
        float face = party.Length > 2 ? 44 : 60;
        foreach (string id in party) faces.AddChild(Image(Art.CharacterPicture(id), new(face, face), Art.Character(id)));
        line.AddChild(faces);

        var info = new VBoxContainer { SizeFlagsHorizontal = Control.SizeFlags.ExpandFill };
        info.AddThemeConstantOverride("separation", 4);
        line.AddChild(info);
        var head = HRow(10);
        head.AddChild(Tag(won ? ReplayListText.RowWin : ReplayListText.RowDeath,
            won ? new Color(0.25f, 0.55f, 0.25f) : new Color(0.62f, 0.2f, 0.18f)));
        head.AddChild(Plain(string.Format(ReplayListText.RowAscension, row.ascension), 18, Gold));
        head.AddChild(Plain(string.Format(ReplayListText.RowFloor, row.act, row.floor), 18));
        if (DateTimeOffset.TryParse(row.ended_at, out var ended))
            head.AddChild(Plain(ended.ToLocalTime().ToString("MM/dd HH:mm"), 16, Muted));
        info.AddChild(head);

        var who = new List<string> { string.Join(" + ", party.Select(Art.Character)) };
        if (row.party_size > 1) who.Add(string.Format(ReplayListText.RowParty, row.party_size));
        if (!won && row.killer is { Length: > 0 } killer)
            who.Add(string.Format(ReplayListText.RowKiller, string.Join(", ", killer.Select(Art.Monster))));
        info.AddChild(Text(string.Join(" · ", who), 16, Muted));

        string[] relics = row.relics ?? [];
        if (relics.Length > 0 || row.deck_size > 0)
        {
            var strip = Flow(2);
            for (int i = 0; i < Math.Min(relics.Length, RelicStrip); i++)
            {
                int index = i;
                var icon = Image(Art.RelicPicture(relics[i]), new(32, 32), Art.Relic(relics[i]) + "\n" + ReplayListText.TooltipRelic);
                Clicked(icon, () => InspectRelic(relics, index));
                strip.AddChild(icon);
            }
            if (relics.Length > RelicStrip) strip.AddChild(Plain(string.Format(ReplayListText.RowMore, relics.Length - RelicStrip), 16, Muted));
            strip.AddChild(Plain("  " + string.Format(ReplayListText.RowRelics, relics.Length) + " ·", 16, Muted));
            string deckText = string.Format(ReplayListText.RowDeck, row.deck_size);
            if (row.deck is { Length: > 0 })
            {
                var deck = new Button { Text = deckText, Flat = true, FocusMode = Control.FocusModeEnum.None,
                    TooltipText = ReplayListText.TooltipDeck, MouseDefaultCursorShape = Control.CursorShape.PointingHand };
                if (GamePopup.KoreanFont is { } font) deck.AddThemeFontOverride("font", font);
                deck.AddThemeFontSizeOverride("font_size", 16);
                deck.AddThemeColorOverride("font_color", Gold);
                deck.AddThemeColorOverride("font_hover_color", Colors.White);
                deck.Pressed += () => showDeck(row);
                strip.AddChild(deck);
            }
            else strip.AddChild(Plain(deckText, 16, Muted));
            info.AddChild(strip);
        }

        if (shown.Cards.Length + shown.Relics.Length > 0)
        {
            var found = Flow(6);
            found.AddChild(Tag(ReplayListText.RowMatched, new Color(0.55f, 0.42f, 0.12f)));
            foreach (string id in shown.Cards)
            {
                found.AddChild(Image(Art.CardPicture(id), new(64, 48), Art.Card(id), cover: true));
                found.AddChild(Plain(Art.Card(id), 16));
            }
            foreach (string id in shown.Relics)
            {
                found.AddChild(Image(Art.RelicPicture(id), new(40, 40), Art.Relic(id)));
                found.AddChild(Plain(Art.Relic(id), 16));
            }
            info.AddChild(found);
        }

        var actions = new VBoxContainer { SizeFlagsVertical = Control.SizeFlags.ShrinkCenter };
        actions.AddThemeConstantOverride("separation", 6);
        line.AddChild(actions);
        var view = RowButton(ReplayListText.RowView);
        view.Pressed += () => Open(ReplayUrl(row.id, row.@public, k));
        actions.AddChild(view);
        bool isPublic = row.@public, saving = false;
        var toggle = RowButton("");
        toggle.TooltipText = ReplayListText.TooltipToggle;
        void Show() => toggle.Text = saving ? ReplayListText.RowSaving
            : isPublic ? ReplayListText.RowIsPublic : ReplayListText.RowIsPrivate;
        Show();
        toggle.Pressed += () =>
        {
            if (saving) return;
            saving = true;
            Show();
            bool wanted = !isPublic;
            _ = Task.Run(async () =>
            {
                bool saved;
                try { saved = await SetPublic(row.id, wanted); }
                catch (Exception) { saved = false; }
                OnMain(() =>
                {
                    if (!GodotObject.IsInstanceValid(toggle)) return;
                    saving = false;
                    if (saved) isPublic = wanted;
                    else toggle.TooltipText = ReplayListText.RowToggleFailed;
                    Show();
                });
            });
        };
        actions.AddChild(toggle);
        // Two presses: the first asks, the second deletes the replay and its receipt on sts.pqp.me.
        var delete = RowButton(Settings.UiText.Get("replay_delete"));
        bool armed = false, deleting = false;
        delete.Pressed += () =>
        {
            if (deleting) return;
            if (!armed) { armed = true; delete.Text = Settings.UiText.Get("replay_delete_confirm"); return; }
            deleting = true;
            _ = Task.Run(async () =>
            {
                bool done;
                try { done = await DeleteReplay(row.id); }
                catch (Exception) { done = false; }
                OnMain(() =>
                {
                    if (!GodotObject.IsInstanceValid(card)) return;
                    if (done) { card.QueueFree(); return; }
                    deleting = armed = false;
                    delete.Text = Settings.UiText.Get("replay_delete_failed");
                });
            });
        };
        actions.AddChild(delete);
        return card;
    }

    // The list and its search box. Rows are drawn in batches as the list scrolls: a full list of
    // 500 runs with images would build thousands of nodes at once.
    sealed class ReplayList(ScrollContainer scroll, VBoxContainer list, Label status, string k)
    {
        const int Batch = 20;
        IReadOnlyList<ReplayRow>? rows;
        List<Shown> shown = [];
        int drawn;
        string folded = "";
        bool failed;

        public void Loaded(IReadOnlyList<ReplayRow>? loaded) { rows = loaded; failed = loaded is null; Render(); }
        public void Search(string text) { folded = Fold(text); if (rows is not null) Render(); }

        void Render()
        {
            if (!GodotObject.IsInstanceValid(list)) return;
            deckOpen = false;
            foreach (var child in list.GetChildren()) child.QueueFree();
            drawn = 0;
            shown = rows is null ? [] : rows.Select(r => Match(r, folded)).OfType<Shown>().ToList();
            status.Visible = folded.Length > 0 && rows is { Count: > 0 };
            status.Text = string.Format(ReplayListText.SearchCount, shown.Count);
            scroll.ScrollVertical = 0;
            if (failed) list.AddChild(Text(ReplayListText.LoadFailed, 18));
            else if (rows is { Count: 0 }) list.AddChild(Text(ReplayListText.Empty, 18));
            else if (shown.Count == 0) list.AddChild(Text(ReplayListText.SearchEmpty, 18));
            else More();
        }

        public void More()
        {
            if (!GodotObject.IsInstanceValid(list)) return;
            foreach (var item in shown.Skip(drawn).Take(Batch)) list.AddChild(Row(item, k, ShowDeck));
            drawn = Math.Min(shown.Count, drawn + Batch);
        }

        public void Scrolled(double value)
        {
            var bar = scroll.GetVScrollBar();
            if (deckOpen) return;
            if (drawn < shown.Count && value + bar.Page >= bar.MaxValue - 400) More();
        }

        bool deckOpen;

        // The run's final deck in place of the list; "목록으로" puts the list (and search) back.
        void ShowDeck(ReplayRow row)
        {
            if (!GodotObject.IsInstanceValid(list)) return;
            foreach (var child in list.GetChildren()) child.QueueFree();
            deckOpen = true;
            status.Visible = false;
            scroll.ScrollVertical = 0;
            var head = HRow(12);
            var back = RowButton(ReplayListText.DeckBack);
            back.Pressed += () => { deckOpen = false; Render(); };
            head.AddChild(back);
            var cards = Art.DeckModels(row.deck ?? []);
            head.AddChild(Plain(string.Format(ReplayListText.DeckTitle,
                string.Join(" + ", row.character.Split('+').Select(Art.Character)), cards.Count), 18, Gold));
            list.AddChild(head);
            list.AddChild(DeckGrid(cards));
        }
    }

    static void ShowMenu()
    {
        bool isPublic = Consent().public_replay;
        string k = Key;
        GamePopup.EnqueueContent(ReplayListText.Title, 980, area =>
            {
                var column = new VBoxContainer();
                column.SetAnchorsPreset(Control.LayoutPreset.FullRect);
                column.AddThemeConstantOverride("separation", 10);
                area.AddChild(column);
                column.AddChild(Text(ReplayListText.Intro, 19));
                column.AddChild(Text(isPublic ? ReplayListText.DefaultPublic : ReplayListText.DefaultPrivate, 17,
                    new Color(0.85f, 0.8f, 0.65f)));
                var bar = HRow(12);
                column.AddChild(bar);
                var search = new LineEdit { PlaceholderText = ReplayListText.SearchPlaceholder, ClearButtonEnabled = true,
                    SizeFlagsHorizontal = Control.SizeFlags.ExpandFill, CustomMinimumSize = new Vector2(0, 40),
                    Editable = false };
                if (GamePopup.KoreanFont is { } font) search.AddThemeFontOverride("font", font);
                search.AddThemeFontSizeOverride("font_size", 18);
                search.AddThemeStyleboxOverride("normal", Box(new Color(0, 0, 0, 0.35f), margin: 8));
                search.AddThemeStyleboxOverride("focus", Box(new Color(0, 0, 0, 0.45f), margin: 8));
                bar.AddChild(search);
                var status = Plain("", 17, Gold);
                status.Visible = false;
                bar.AddChild(status);
                var scroll = new ScrollContainer { SizeFlagsVertical = Control.SizeFlags.ExpandFill,
                    HorizontalScrollMode = ScrollContainer.ScrollMode.Disabled };
                column.AddChild(scroll);
                var list = new VBoxContainer { SizeFlagsHorizontal = Control.SizeFlags.ExpandFill };
                list.AddThemeConstantOverride("separation", 8);
                scroll.AddChild(list);
                var view = new ReplayList(scroll, list, status, k);
                search.TextChanged += view.Search;
                scroll.GetVScrollBar().ValueChanged += view.Scrolled;
                if (Owner is not { } owner) { view.Loaded(null); return; }
                list.AddChild(Text(ReplayListText.Loading, 18));
                _ = Task.Run(async () =>
                {
                    IReadOnlyList<ReplayRow>? rows;
                    try { rows = await FetchRows(owner); }
                    catch (Exception e) { rows = null; GD.Print("[HumanPlayRecorder] replay list unavailable: " + e.GetType().Name); }
                    OnMain(() =>
                    {
                        if (!GodotObject.IsInstanceValid(search)) return;
                        search.Editable = rows is { Count: > 0 };
                        view.Loaded(rows);
#if DEBUG
                        // Screenshot harness: show a search in progress.
                        if (DebugUiCapture.Active && System.Environment.GetEnvironmentVariable("HPR_UI_CAPTURE_REPLAY_SEARCH") is { Length: > 0 } query)
                        {
                            search.Text = query;
                            view.Search(query);
                        }
#endif
                    });
                });
            },
            new(ReplayListText.OpenSite, () => Open(MyReplaysUrl(Key))),
            new(ReplayListText.Close, () => { }),
            new(isPublic ? ReplayListText.DefaultToPrivate : ReplayListText.DefaultToPublic,
                () => { SaveConsent(!isPublic); ShowMenu(); }),
            "replay-menu");
    }

    internal static void Open(string url)
    {
        if (url.EndsWith("#k=", StringComparison.Ordinal)) return;
        var error = OS.ShellOpen(url);
        if (error != Error.Ok) GD.Print("[HumanPlayRecorder] browser open failed: " + error);
    }

    // Upload worker thread: a run reached the server; tell the player where its replay is.
    public static void Announce(UploadReceipt receipt)
    {
        // A recording kept only for training has no replay and no link.
        if (Key is not { Length: > 0 } k || !UploadPolicy.Announces(receipt, OwnerOf(k))) return;
        string url = ReplayUrl(receipt.sha256, receipt.public_replay == true, k);
        try
        {
            // A corner note, not a modal: the upload may finish in the middle of the next run.
            Callable.From(() => FirstLaunchNotice.ShowShortNote("리플레이가 올라갔습니다",
                "방금 기록한 런을 sts.pqp.me에서 다시 볼 수 있습니다.\n서버가 기록을 받는 데 1~2분 걸릴 수 있습니다.",
                new("리플레이 보기", () => Open(url)), closeAfterSeconds: 20)).CallDeferred();
        }
        catch (Exception e)
        {
            Console.Error.WriteLine("[HumanPlayRecorder] replay notice scheduling failed: " + e.GetType().Name);
        }
    }
}
