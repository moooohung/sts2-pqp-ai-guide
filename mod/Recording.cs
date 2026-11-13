using System.Security.Cryptography;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text;
using System.Globalization;
using System.Threading.Channels;

namespace HumanPlayRecorder;

// No game objects or network identities may cross this DTO boundary.
public sealed record CardRef(string id, int upgrade);
public sealed record TargetRef(string side, int index, string model_id, int creature_index);
public sealed record DigestPlayer(int seat, int hp, int gold, int deck_size,
    CardRef[] deck, string[] relics, string?[] potions);
public sealed record PartySeat(int seat, string character);
public sealed record StateDigest(int act, int floor, DigestPlayer[] players,
    IReadOnlyDictionary<string, long>? rng_counters = null)
{
    public ReplayCheckpoint? replay {get;init;}
}
public sealed record PatchAuditEntry(string mod_id,string method,string kind);
public sealed record PatchAuditSummary(string sha256,int entries,int prefix,int postfix,int transpiler,int finalizer);
public sealed record RngCheckpoint(uint counter,ulong[] state);
public sealed record PowerCheckpoint(string id,int amount);
public sealed record CreatureCheckpoint(string id,int hp,int max_hp,int block,PowerCheckpoint[] powers,RngCheckpoint? rng=null);
public sealed record RelicCheckpoint(string id,string status,int stacks,int display,SortedDictionary<string,decimal> vars);
public sealed record PotionCheckpoint(string id,bool queued,SortedDictionary<string,decimal> vars);
public sealed record CombatSeatCheckpoint(int seat,int? energy,int? stars,CardRef[] hand,CardRef[] draw,
    CardRef[] discard,CardRef[] exhaust,CreatureCheckpoint creature,SortedDictionary<string,RngCheckpoint> rng,
    RelicCheckpoint[] relics,PotionCheckpoint?[] potions,float card_rarity_odds,float potion_reward_odds,
    SortedDictionary<string,string[]> relic_bag);
public sealed record OfferCheckpoint(string kind,bool selected,CardRef[] cards,string? model,int? gold);
public sealed record EventOptionCheckpoint(string id,bool enabled,string? relic);
public sealed record ShopCheckpoint(string kind,string? model,int price,bool stocked);
public sealed record UnlockCheckpoint(int seat,string[] epochs,string[] encounters,int number_of_runs);
public sealed record ReplaySetup(string[] acts,UnlockCheckpoint[] unlocks);
public sealed record ReplayCheckpoint(string contract,SortedDictionary<string,RngCheckpoint> run_rng,
    CombatSeatCheckpoint[] players,CreatureCheckpoint[] enemies,int? round,string? room,string? model,int[]? coord,
    int[][] next,OfferCheckpoint[] rewards)
{
    public RngCheckpoint? event_rng {get;init;}
    public EventOptionCheckpoint[]? event_options {get;init;}
    public ShopCheckpoint[]? shop {get;init;}
    public SortedDictionary<string,string[]>? shared_relic_bag {get;init;}
}
public sealed record RecordedAction(
    string kind, int player, int? hand_index = null, CardRef? card = null,
    TargetRef? target = null, int? slot_index = null, string? potion_id = null,
    int[]? indices = null, CardRef[]? cards = null, int[]? coord = null,
    string? option_id = null, int? reward_index = null, string? reward_type = null,
    int? index = null, uint? choice_id = null, string? relic_id = null, int? price = null,
    string? sync_kind = null, int? generation = null, bool? cancelled = null)
{
    public CardRef[]? offered_cards { get; init; }
    [JsonIgnore] public int? InputSequence { get; init; }
    [JsonIgnore] public int? ParentSequence { get; init; }
    [JsonIgnore] public ReplayCheckpoint? ReplayBefore { get; init; }
}
public sealed record ActionRow(int seq, string phase, string at, RecordedAction action,
    StateDigest after, string state_sha256, int? parent_seq = null)
{
    public ReplayCheckpoint? before {get;init;}
}
public sealed record ModRef(string id, string version, string dll_sha256);
public sealed record RunResult(bool won, int act, int floor, bool boss_cleared, int hp)
{
    public StateDigest? terminal_state {get;init;}
}
public sealed record FloorDigest(int act, int floor, int after_action, StateDigest state, string sha256);
public sealed record RunPayload(string schema, string action_schema, string run_id,
    string mod_version, string game_version, string game_dll_sha256, string character,
    int ascension, string seed, ModRef[] active_mods, ActionRow[] actions,
    FloorDigest[] floor_digests, RunResult result, string started_at, string ended_at,
    string mode, int party_size, int local_seat, PartySeat[] party, string shared_run_id)
{
    public int[]? ai_teammate_seats {get;init;}
    public TrainingDecision? training {get;init;}
    public EnvironmentMod[]? environment_mods {get;init;}
    public PatchAuditEntry[]? patch_audit {get;init;}
    public string? replay_contract {get;init;}
    public string? input_seed {get;init;}
    public ReplaySetup? setup {get;init;}
    public StateDigest? initial_state {get;init;}
}
public sealed record TrainingDecision(string vanilla_fingerprint,string decision);
public sealed record EnvironmentMod(string id,string version);

public static class RecordingFormat
{
    public static PatchAuditSummary AuditSummary(PatchAuditEntry[]? audit)
    {
        audit??=[];
        return new(Convert.ToHexStringLower(SHA256.HashData(JsonSerializer.SerializeToUtf8Bytes(audit,Json))),
            audit.Length,audit.Count(p=>p.kind=="prefix"),audit.Count(p=>p.kind=="postfix"),
            audit.Count(p=>p.kind=="transpiler"),audit.Count(p=>p.kind=="finalizer"));
    }
    public static readonly JsonSerializerOptions Json = new()
    {
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull
    };
    public static string Hash(StateDigest digest) =>
        Convert.ToHexStringLower(SHA256.HashData(JsonSerializer.SerializeToUtf8Bytes(digest, Json)));
    public static string SharedRunId(string seed, string runStartToken)
    {
        if (!ulong.TryParse(seed, NumberStyles.None, CultureInfo.InvariantCulture, out var value) ||
            value.ToString(CultureInfo.InvariantCulture) != seed)
            throw new ArgumentException("canonical decimal seed required");
        if (string.IsNullOrEmpty(runStartToken) || Encoding.UTF8.GetByteCount(runStartToken) > 256 ||
            runStartToken.Any(char.IsControl)) throw new ArgumentException("run start token");
        return Convert.ToHexStringLower(SHA256.HashData(Encoding.UTF8.GetBytes(
            "sts2-human-run-v1/shared\n" + seed + "\n" + runStartToken)));
    }
    /// The action schema this recorder writes. v2 adds the solo chest button as its own
    /// `OpenChest` input and the solo treasure Skip (a `SkipRelic` without a reward index);
    /// v1 readers refuse both, so the version says which a recording may carry.
    public const string ActionSchema = "sts2-native-input-v2";
    public static bool KnownActionSchema(string? schema) =>
        schema is "sts2-native-input-v1" or ActionSchema;
    /// The screen an input belongs to (ActionRow.phase). The solo treasure Skip is the treasure
    /// room's proceed button (NTreasureRoom.cs:176-185), not a reward screen's Skip.
    public static string Phase(RecordedAction a) => a.kind switch {
        "Move" => "MapChoice", "RestOption" => "RestSite",
        "EventChoice" => "Event",
        "BuyCard" or "BuyPotion" or "BuyRelic" or "RemoveCard" => "Shop",
        "Collect" or "OpenChest" => "Treasure",
        "SkipRelic" when a.reward_index is null => "Treasure",
        "OpenReward" or "PickCard" or "PickCardBundle" or "RerollCardReward" or
        "PickPotion" or "PickRelicReward" or "SkipRewards" or "SkipPotion" or
        "SkipRelic" or "Skip" => "CardReward",
        _ => "Combat"
    };
    public static void Validate(RecordedAction a)
    {
        if (a.player < 0 || a.player > 3) throw new ArgumentException("seat");
        if (a.sync_kind is not null && a.sync_kind is not
            ("map_vote" or "map_travel" or "event_vote" or "event_execute" or "treasure_vote" or
             "treasure_pick" or "merchant_outcome"))
            throw new ArgumentException("sync kind");
        if (a.generation is < 0 ||
            (a.sync_kind is "map_vote" or "event_vote" && a.generation is null))
            throw new ArgumentException("vote generation");
        if (a.sync_kind is "map_vote" or "map_travel" && a.kind != "Move" ||
            a.sync_kind is "event_vote" or "event_execute" && a.kind != "EventChoice" ||
            a.sync_kind is "treasure_vote" or "treasure_pick" && a.kind != "PickRelicReward")
            throw new ArgumentException("sync/action kind mismatch");
        if (a.cancelled == true && a.sync_kind is not ("map_vote" or "treasure_vote"))
            throw new ArgumentException("only votes can cancel");
        if (a.target is { } t && (t.index < 0 || t.creature_index < 0 ||
            t.side is not ("enemy" or "player"))) throw new ArgumentException("target");
        switch (a.kind)
        {
            case "PlayCard":
                if (a.hand_index is null or < 0 || a.card is null) throw new ArgumentException("card");
                break;
            case "UsePotion":
            case "DiscardPotion":
                if (a.slot_index is null or < 0 || string.IsNullOrEmpty(a.potion_id))
                    throw new ArgumentException("potion");
                if (a.kind == "DiscardPotion" && a.target is not null)
                    throw new ArgumentException("discard has no target");
                break;
            case "Choose":
                if (a.indices is null || a.cards is null || a.indices.Length != a.cards.Length ||
                    a.indices.Any(i => i < 0) || a.indices.Distinct().Count() != a.indices.Length)
                    throw new ArgumentException("choice");
                break;
            case "PickCardBundle":
                if (a.index is null or < 0 || a.cards is null || a.cards.Length == 0)
                    throw new ArgumentException("card bundle");
                break;
            case "Move":
                if (a.sync_kind == "map_vote" && a.cancelled == true)
                {
                    if (a.coord is not null) throw new ArgumentException("cancelled vote has coord");
                }
                else if (a.coord is not { Length: 2 }) throw new ArgumentException("coord");
                break;
            case "EventChoice":
            case "RestOption":
                if (string.IsNullOrEmpty(a.option_id)) throw new ArgumentException("option");
                break;
            case "EndTurn": case "ConfirmChoice": case "Skip": case "LeaveShop":
                break;
            case "BuyCard":
                if (a.card is null || a.price is null or < 0 ||
                    (a.sync_kind != "merchant_outcome" &&
                     (a.index is null or < 0 || a.slot_index != a.index)))
                    throw new ArgumentException("shop card");
                break;
            case "BuyPotion": case "BuyRelic":
                if ((a.sync_kind != "merchant_outcome" &&
                     (a.index is null or < 0 || a.slot_index != a.index)) || a.price is null or < 0 ||
                    (a.kind == "BuyPotion" ? a.potion_id is null : a.relic_id is null))
                    throw new ArgumentException("shop item");
                break;
            case "RemoveCard":
                if (a.sync_kind == "merchant_outcome" && a.price is null or < 0)
                    throw new ArgumentException("shop removal");
                break;
            case "PickCard":
                if (a.reward_index is null or < 0 || a.reward_type != "card" ||
                    a.index is null or < 0 || a.card is null)
                    throw new ArgumentException("card reward");
                break;
            case "RerollCardReward":
                if (a.reward_index is null or < 0 || a.reward_type != "card")
                    throw new ArgumentException("card reward reroll");
                break;
            case "OpenChest":
                break;
            case "OpenReward": case "Collect": case "SkipRewards":
            case "SkipPotion": case "SkipRelic":
                // The solo treasure Skip leaves every offered relic (NTreasureRoom.cs:176-185).
                if (a.kind == "SkipRelic" && a.reward_type == "relic" && a.reward_index is null) break;
                if (a.reward_index is null or < 0 || a.reward_type is null)
                    throw new ArgumentException("reward");
                break;
            case "PickPotion":
                if (a.reward_index is null or < 0 || a.reward_type != "potion" ||
                    string.IsNullOrEmpty(a.potion_id)) throw new ArgumentException("potion reward");
                break;
            case "PickRelicReward":
                if (a.reward_type != "relic") throw new ArgumentException("relic reward");
                if (a.sync_kind == "treasure_vote" && a.cancelled == true)
                {
                    if (a.reward_index is not null || a.relic_id is not null)
                        throw new ArgumentException("cancelled treasure vote");
                }
                else if (a.sync_kind == "treasure_pick")
                {
                    if (string.IsNullOrEmpty(a.relic_id) || a.reward_index is < 0)
                        throw new ArgumentException("treasure pick");
                }
                else if (a.reward_index is null or < 0 || string.IsNullOrEmpty(a.relic_id))
                    throw new ArgumentException("relic reward");
                break;
            default: throw new ArgumentException("unimplemented action adapter");
        }
    }
}
