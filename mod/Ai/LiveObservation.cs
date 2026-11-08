using System.Text.Json;
using System.Runtime.CompilerServices;
using MegaCrit.Sts2.Core.Combat;
using MegaCrit.Sts2.Core.Entities.Cards;
using MegaCrit.Sts2.Core.Entities.Creatures;
using MegaCrit.Sts2.Core.Entities.Merchant;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.Entities.RestSite;
using MegaCrit.Sts2.Core.Events;
using MegaCrit.Sts2.Core.Hooks;
using MegaCrit.Sts2.Core.Localization.DynamicVars;
using MegaCrit.Sts2.Core.Map;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.MonsterMoves.Intents;
using MegaCrit.Sts2.Core.Rewards;
using MegaCrit.Sts2.Core.Rooms;
using MegaCrit.Sts2.Core.Runs;

namespace HumanPlayRecorder.Ai;

/// <summary>Allowlisted public game data, captured on the game thread. No RNG,
/// save objects, native actions, or simulated replacements cross this boundary.</summary>
public static partial class LiveObservation
{
    sealed class MapVoteHistory
    {
        readonly Dictionary<Player, MapCoord?> votes;
        // Players whose retained vote is known: everyone when observed from run start,
        // otherwise (a loaded run) each player from their first vote event onward.
        readonly HashSet<Player> known;
        readonly RunState run;
        public MapVoteHistory(RunState run, bool fromRunStart = false)
        {
            this.run = run;
            known = fromRunStart ? run.Players.ToHashSet() : new();
            var synchronizer = RunManager.Instance.MapSelectionSynchronizer;
            votes = run.Players.ToDictionary(p => p, p => synchronizer.GetVote(p)?.coord);
            synchronizer.PlayerVoteChanged += (player, _, vote) => {
                votes[player] = vote?.coord; known.Add(player); RecorderMod.MarkAdvisorDirty();
            };
            synchronizer.PlayerVoteCancelled += player => {
                votes[player] = null; known.Add(player); RecorderMod.MarkAdvisorDirty();
            };
            // The reference retains accepted votes after travel clears the UI's votes.
        }
        public MapCoord? GetVote(Player player)
        {
            if (RunManager.Instance.MapSelectionSynchronizer.GetVote(player) is { } current) return current.coord;
            // EnterAct enters the run's first room without a vote, so at TotalFloor 1
            // no vote can have been cast and cleared yet.
            // A loaded save keeps no votes (PlayerMapPointHistoryEntry has none), so the
            // vote that moved the party here is gone. The room the party travelled to is
            // the retained vote for every player who agreed with it; refusing instead
            // blocked advice and every AI seat for the whole floor after a reload.
            return RetainedVote(known.Contains(player), player.RunState.TotalFloor, run.CurrentMapCoord,
                votes.GetValueOrDefault(player));
        }
    }
    internal static MapCoord? RetainedVote(bool observed, int totalFloor, MapCoord? current, MapCoord? recorded) =>
        !observed && totalFloor > 1 ? current : recorded;
    static readonly ConditionalWeakTable<RunState, MapVoteHistory> mapVoteHistories = new();
    internal static void ObserveRunStart(RunState state)
    {
        if (state.Players.Count > 1 && state.TotalFloor == 0)
            mapVoteHistories.GetValue(state, run => new MapVoteHistory(run, true));
    }
    internal static MapCoord? CurrentVote(RunState state, Player player) =>
        mapVoteHistories.GetValue(state, run => new MapVoteHistory(run)).GetVote(player);
    // NativeObs wire order differs from the game's MapPointType enum order.
    static readonly MapPointType[] NativeMapPointTypes = [
        MapPointType.Unassigned, MapPointType.Unknown, MapPointType.Monster, MapPointType.Elite,
        MapPointType.Boss, MapPointType.Shop, MapPointType.RestSite, MapPointType.Treasure, MapPointType.Ancient
    ];
    public sealed record Id(string @namespace, string name);
    public sealed class Element(string kind)
    {
        public string kind { get; } = kind;
        public Id? model { get; set; }
        public Id? color { get; set; }
        public Id?[] detail { get; set; } = [];
        public int owner { get; set; }
        public int parent { get; set; }
        public SortedDictionary<string, int> numbers { get; } = new(StringComparer.Ordinal);
        public SortedDictionary<string, float> floats { get; } = new(StringComparer.Ordinal);
        public SortedDictionary<string, bool> flags { get; } = new(StringComparer.Ordinal);
        public Element N(string name, int value) { numbers[name] = value; return this; }
        public Element F(string name, bool value) { flags[name] = value; return this; }
    }
    public sealed record Option(string binding, string action, int source, int target, Id? model,
        SortedDictionary<string, int> numbers, SortedDictionary<string, bool> flags, bool legal = true);
    public sealed class Snapshot(int seat)
    {
        public string schema => "sts2-live-advice-v1";
        public int seat { get; } = seat;
        public List<Element> rows { get; } = [];
        public List<Option> options { get; } = [];
        public List<object[]> edges { get; } = [];
        internal LiveProjectionFingerprint? FingerprintWriter;
        internal bool FingerprintOnly;
        internal long Fingerprint;
        internal int Add(Element row)
        {
            FrameCaptureBudget.Check();
            FingerprintWriter?.Write(row);
            if (FingerprintOnly) {
                // Only flags are read back by the shared projection (map reachability/selection).
                row.model = row.color = null;
                row.detail = [];
                row.numbers.Clear();
                row.floats.Clear();
            }
            rows.Add(row);
            return rows.Count;
        }
        internal void Edge(int from, int to, string relation)
        {
            FrameCaptureBudget.Check();
            FingerprintWriter?.Write(from, to, relation);
            if (!FingerprintOnly) edges.Add([from, to, relation]);
        }
        internal void AddOption(Option option)
        {
            FrameCaptureBudget.Check();
            FingerprintWriter?.Write(option);
            if (!FingerprintOnly) options.Add(option);
        }
    }

    static Id I(string space, object name) => new(space, name.ToString()!);
    // The game's IntentType has no multi-attack value (MultiAttackIntent reports Attack);
    // the training observation names the MultiAttackIntent class MultiAttack.
    static Id IntentId(AbstractIntent intent) =>
        I("IntentType", intent is MultiAttackIntent ? "MultiAttack" : intent.IntentType);
    static Id? Model(object source) => source switch {
        CardModel c => I("CardId", c.Id.Entry), RelicModel r => I("RelicId", r.Id.Entry),
        PotionModel p => I("PotionId", p.Id.Entry), EventOption e => I("EventOptionId", EventKey(e.TextKey)),
        EventModel e => I("EventId", e.Id.Entry), PowerModel p => I("PowerId", p.Id.Entry),
        MonsterModel m => I("MonsterKind", m.Id.Entry),
        _ => null
    };
    static Id? CardColor(CardModel card)
    {
        CardPoolModel pool;
        try { pool = LiveModelData.Property(card, "Pool", () => card.Pool); }
        catch (InvalidProgramException) { return null; } // Unregistered injected cards have no pool.
        if (pool is MegaCrit.Sts2.Core.Models.CardPools.ColorlessCardPool or
            MegaCrit.Sts2.Core.Models.CardPools.CurseCardPool or
            MegaCrit.Sts2.Core.Models.CardPools.StatusCardPool) return null;
        // ModelDb.AllCharacters re-resolves every character id on each read; one
        // resolution per pool and capture keeps a full deck from repeating it per card.
        if (poolColors.TryGetValue(pool.Id, out var known)) return known;
        var color = ResolveCardColor(card, pool);
        poolColors[pool.Id] = color;
        return color;
    }
    // Cleared at the start of every projection, so characters registered later are seen.
    static readonly Dictionary<ModelId, Id?> poolColors = [];
    static Id? ResolveCardColor(CardModel card, CardPoolModel pool)
    {
        foreach (var character in ModelDb.AllCharacters.Append(card.Owner.Character).Distinct()) {
            FrameCaptureBudget.Check();
            try {
                if (LiveModelData.Property(character, "CardPool", () => character.CardPool).Id == pool.Id)
                    return I("Character", character.Id.Entry);
            } catch (Exception) {
                // An unrelated character cannot veto this card's colour.
                FrameCaptureBudget.Check();
            }
        }
        return null;
    }
    // ModelDb.AllRelicPools re-resolves (slugifies) every pool id on each read: index the
    // pools once per projection; cleared with poolColors.
    static Dictionary<ModelId, Id>? relicPools;
    static Id? RelicPool(RelicModel relic)
    {
        if (relicPools is null) {
            relicPools = [];
            foreach (var pool in ModelDb.AllRelicPools)
                foreach (var id in pool.AllRelicIds)
                    relicPools.TryAdd(id, I("RelicPool", pool.Id.Entry)); // first pool wins, as before
        }
        return relicPools.GetValueOrDefault(relic.Id);
    }
    static void Decimal(Element row, decimal value)
    {
        if (value == decimal.Truncate(value) && value >= int.MinValue && value <= int.MaxValue) {
            row.N("amount", (int)value);
            return;
        }
        var bits = decimal.GetBits(value);
        row.N("decimal_scale", (bits[3] >> 16) & 0xff).N("sign", bits[3] < 0 ? -1 : 1);
        for (int i = 0; i < 6; i++) row.N("coefficient_" + i, (int)(((uint)bits[i / 2] >> (16 * (i % 2))) & 0xffff));
    }
    static void Variables(Snapshot snapshot, IEnumerable<DynamicVar> vars, int parent, int owner)
    {
        foreach (var variable in vars) {
            bool integer = variable.BaseValue == decimal.Truncate(variable.BaseValue) &&
                variable.BaseValue >= int.MinValue && variable.BaseValue <= int.MaxValue;
            var row = new Element(integer ? "EffectParameter" : "Modifier") {
                owner = owner, parent = parent,
                model = integer ? I("PublicVariable", variable.Name) : null,
                detail = integer ? [] : [I("PublicVariable", variable.Name)]
            };
            if (integer) row.N("amount", (int)variable.BaseValue);
            else Decimal(row, variable.BaseValue);
            snapshot.Add(row);
        }
    }
    static int Relic(Snapshot snapshot, RelicModel relic, int owner, int parent, int? order = null)
    {
        var row = new Element("Relic") { model = Model(relic), owner = owner, parent = parent,
            detail = [I("RelicRarity", relic.Rarity), RelicPool(relic)] };
        // Offered relics (event/shop/reward previews) carry no counter in the training observation.
        if (order is { } index) {
            row.N("inventory_order", index).F("Used", relic.Status.ToString() == "Disabled");
            if (relic.ShowCounter) row.N("display_counter", relic.DisplayAmount);
        }
        int handle = snapshot.Add(row);
        Variables(snapshot, relic.DynamicVars.Values, handle, owner);
        return handle;
    }
    static int Potion(Snapshot snapshot, PotionModel? potion, int owner, int parent,
        int? slot = null, int? capacity = null)
    {
        var row = new Element("Potion") { model = potion is null ? null : Model(potion), owner = owner, parent = parent };
        if (slot is { } index) row.N("slot", index);
        if (capacity is { } count) row.N("capacity", count);
        if (potion is not null) row.detail = [I("PotionRarity", potion.Rarity), I("PotionUsageType", potion.Usage),
            I("PotionTargetType", potion.TargetType)];
        int handle = snapshot.Add(row);
        if (potion is not null) Variables(snapshot, potion.DynamicVars.Values, handle, owner);
        return handle;
    }
    static int Bundle(Snapshot snapshot, IReadOnlyList<CardModel> cards, int owner, int parent, int order)
    {
        int offer = snapshot.Add(new Element("Offer") { owner = owner, parent = parent }
            .N("bundle_size", cards.Count).N("offer_order", order));
        foreach (var card in cards) {
            int child = Card(snapshot, card, owner, offer, "None", null);
            snapshot.Edge(offer, child, "Member");
        }
        return offer;
    }
    static bool IsDisplayVariable(DynamicVar variable)
    {
        if (variable is DamageVar or BlockVar or OstyDamageVar or ExtraDamageVar or
            CalculatedDamageVar or CalculatedBlockVar) return true;
        for (var type = variable.GetType(); type is not null; type = type.BaseType)
            if (type.IsGenericType && type.GetGenericTypeDefinition() == typeof(PowerVar<>)) return true;
        return false;
    }
    static void CardValues(Element row, IReadOnlyList<DisplayValue> vars, string damage, string block)
    {
        foreach (var variable in vars) {
            if (variable.Damage)
                row.N(damage, decimal.ToInt32(variable.PreviewValue));
            if (variable.Block)
                row.N(block, decimal.ToInt32(variable.PreviewValue));
        }
    }
    static int ObservationEnergyCost(CardModel card)
    {
        int cost = !FrameCaptureBudget.Active ? card.EnergyCost.GetWithModifiers(CostModifiers.All) :
            LiveModelData.Item(card, "energy_cost", () => LiveModelData.EnergyCost(card));
        return card.Pile?.IsCombatPile == true ? Math.Max(0, cost) : cost;
    }
    static int ObservationStarCost(CardModel card) =>
        Math.Max(0, !FrameCaptureBudget.Active
            ? card.HasStarCostX ? card.CurrentStarCost : card.GetStarCostWithModifiers()
            : LiveModelData.Item(card, "star_cost", () => LiveModelData.StarCost(card)));
    static void DisplayVariables(Snapshot snapshot, CardModel card, IReadOnlyList<DisplayValue> preview,
        int handle, int owner, int target = 0)
    {
        foreach (var variable in preview) {
            var row = new Element("CardPreview") {
                model = Model(card), owner = owner, parent = handle,
                detail = [I("PublicVariable", variable.Name)]
            }.N("display_value", decimal.ToInt32(variable.PreviewValue))
                .N("enchanted_base", decimal.ToInt32(variable.EnchantedValue));
            if (card.Pile?.IsCombatPile == true && card.Owner.PlayerCombatState is { } combat) {
                if (FrameCaptureBudget.Active && (LiveModelData.Energy(card).CostsX ||
                    LiveModelData.Property(card, "HasStarCostX", () => card.HasStarCostX)))
                    throw new AdviceUnavailableException(AdviceReason.PreviewUnavailable, "추천 불가: 게임이 X 비용 미리보기 값을 공개하지 않았습니다.");
                if (card.EnergyCost.CostsX)
                    row.N("energy_x", Hook.ModifyXValue(card.CombatState!, card, combat.Energy));
                if (card.HasStarCostX)
                    row.N("star_x", Hook.ModifyXValue(card.CombatState!, card, combat.Stars));
            }
            int child = snapshot.Add(row);
            if (target > 0) snapshot.Edge(child, target, "Target");
        }
    }
    static int Card(Snapshot snapshot, CardModel card, int owner, int parent, string pile, int? order)
        => LiveModelData.Item(card, "Card", () => CardRows(snapshot, card, owner, parent, pile, order));
    static int CardRows(Snapshot snapshot, CardModel card, int owner, int parent, string pile, int? order)
    {
        FrameCaptureBudget.Check();
        var preview = Preview(card, null);
        var row = new Element("Card") {
            model = Model(card), owner = owner, parent = parent,
            color = CardColor(card),
            detail = [I("CardType", LiveModelData.Property(card, "Type", () => card.Type)),
                I("CardRarity", LiveModelData.Property(card, "Rarity", () => card.Rarity)),
                I("TargetType", LiveModelData.Property(card, "TargetType", () => card.TargetType)), I("PileKind", pile)]
        };
        row.N("energy_cost", ObservationEnergyCost(card))
            .N("star_cost", ObservationStarCost(card)).N("upgrades", card.CurrentUpgradeLevel)
            .N("replay_count", card.BaseReplayCount);
        if (order is { } index) row.N("hand_order", index);
        CardValues(row, preview, "display_damage", "display_block");
        var cardKeywords = !FrameCaptureBudget.Active ? card.Keywords : LiveModelData.Keywords(card);
        var keywords = cardKeywords.Select(k => k.ToString()).ToHashSet(StringComparer.Ordinal);
        foreach (var (flag, keyword) in new[] {
            ("Exhausts", "Exhaust"), ("Ethereal", "Ethereal"), ("Innate", "Innate"),
            ("Sly", "Sly"), ("Eternal", "Eternal")
        }) row.F(flag, keywords.Contains(keyword));
        row.F("Upgraded", card.CurrentUpgradeLevel > 0).F("XCost", LiveModelData.Energy(card).CostsX)
            .F("StarXCost", LiveModelData.Property(card, "HasStarCostX", () => card.HasStarCostX))
            .F("Retained", LiveModelData.Retained(card, cardKeywords));
        int handle = snapshot.Add(row);
        if (card.Enchantment is { } enchantment)
            snapshot.Add(new Element("Modifier") {
                model = I("EnchantmentId", enchantment.Id.Entry), parent = handle, owner = owner
            }.N("amount", enchantment.Amount));
        if (card.Affliction is { } affliction)
            snapshot.Add(new Element("Modifier") {
                model = I("CardAffliction", affliction.Id.Entry), parent = handle, owner = owner
            }.N("amount", affliction.Amount));
        foreach (var variable in LiveModelData.Variables(card).Values.Where(v => v is not CalculatedVar)) {
            var retained = new Element("Modifier") {
                parent = handle, owner = owner, detail = [I("VarKey", variable.Name)]
            };
            Decimal(retained, variable.BaseValue);
            snapshot.Add(retained);
        }
        foreach (var tag in LiveModelData.Tags(card)) snapshot.Add(new Element("Modifier") {
            model = I("CardTag", tag), owner = owner, parent = handle
        }.N("amount", 1));
        DisplayVariables(snapshot, card, preview, handle, owner);
        return handle;
    }
    static void Pile(Snapshot snapshot, IEnumerable<CardModel> source, string pile, int owner,
        Dictionary<object, int> handles, int relics = 0, int potions = 0)
    {
        var cards = new List<CardModel>();
        var row = new Element("PileSummary") { owner = owner, parent = owner, detail = [I("PileKind", pile)] };
        foreach (string name in new[] { "total", "attacks", "skills", "powers", "curses", "statuses", "upgraded",
            "cost_zero", "cost_one", "cost_two", "cost_three", "cost_four_plus", "x_cost", "unplayable" })
            row.N(name, 0);
        row.N("relics", relics).N("potions", potions);
        foreach (var card in source) {
            FrameCaptureBudget.Check();
            cards.Add(card);
            row.numbers["total"]++;
            string? type = LiveModelData.Property(card, "Type", () => card.Type) switch { CardType.Attack => "attacks", CardType.Skill => "skills",
                CardType.Power => "powers", CardType.Curse => "curses", CardType.Status => "statuses", _ => null };
            if (type is not null) row.numbers[type]++;
            if (card.CurrentUpgradeLevel > 0) row.numbers["upgraded"]++;
            int cost = ObservationEnergyCost(card);
            string lane = LiveModelData.Energy(card).CostsX ||
                LiveModelData.Property(card, "HasStarCostX", () => card.HasStarCostX) ? "x_cost" : cost < 0 ? "unplayable" :
                new[] { "cost_zero", "cost_one", "cost_two", "cost_three", "cost_four_plus" }[Math.Min(cost, 4)];
            row.numbers[lane]++;
        }
        int summary = snapshot.Add(row);
        foreach (var (card, index) in cards.Select((c, i) => (c, i))) {
            int handle = Card(snapshot, card, owner, owner, pile, pile == "Hand" ? index : null);
            handles[card] = handle;
            snapshot.Edge(summary, handle, "Member");
        }
    }
    static void Routes(Snapshot snapshot, MapPoint start, Dictionary<object, int> handles)
    {
        var distances = new Dictionary<MapPoint, int>(ReferenceEqualityComparer.Instance) { [start] = 0 };
        var queue = new Queue<MapPoint>();
        queue.Enqueue(start);
        while (queue.TryDequeue(out var point))
            foreach (var child in point.Children)
                if (handles.ContainsKey(child) && distances.TryAdd(child, distances[point] + 1))
                    queue.Enqueue(child);
        foreach (var kind in NativeMapPointTypes) {
            var values = distances.Where(p => p.Key.PointType == kind).Select(p => p.Value).ToArray();
            var row = new Element("RouteSummary") {
                parent = handles[start], detail = [I("MapPointType", kind)]
            }.N("count", values.Length);
            if (values.Length > 0) row.N("nearest", values.Min()).N("farthest", values.Max());
            snapshot.Add(row);
        }
    }

    public static Snapshot Capture(RunState run, int seat, IReadOnlyList<AiAdvisor.LiveOption> options)
        => Project(run, seat, options, false);

    internal static long Signature(RunState run, int seat, IReadOnlyList<AiAdvisor.LiveOption> options)
        => Project(run, seat, options, true).Fingerprint;

    static Snapshot Project(RunState run, int seat, IReadOnlyList<AiAdvisor.LiveOption> options, bool fingerprintOnly)
    {
        FrameCaptureBudget.Check();
        poolColors.Clear();
        relicPools = null;
        var snapshot = new Snapshot(seat) { FingerprintOnly = fingerprintOnly };
        using var fingerprint = new LiveProjectionFingerprint(snapshot.schema, seat);
        snapshot.FingerprintWriter = fingerprint;
        var handles = new Dictionary<object, int>(ReferenceEqualityComparer.Instance);
        var inputs = options.Select(o => {
            FrameCaptureBudget.Check();
            return o.Input;
        }).ToArray();
        bool multi = run.Players.Count > 1;
        var voteHistory = multi ? mapVoteHistories.GetValue(run, state => new MapVoteHistory(state)) : null;
        bool hasMap = run.Map is not NullActMap;
        bool rewards = run.CurrentRoom is not (EventRoom or TreasureRoom or MerchantRoom) &&
            inputs.Any(a => a.kind is "PickCard" or "Skip" or "RerollCardReward" or "OpenReward" or
                "PickPotion" or "SkipPotion" or "PickRelicReward" or "SkipRelic" or "SkipRewards" or "Collect");
        if (!multi && (run.CurrentRoom is not CombatRoom || !CombatManager.Instance.IsInProgress) &&
            inputs.Any(a => a.kind is "Choose" or "ConfirmChoice" or "PickCardBundle")) rewards = true;
        bool map = inputs.Any(a => a.kind == "Move");
        var combat = (run.CurrentRoom as CombatRoom)?.CombatState;
        bool fighting = combat is not null && CombatManager.Instance.IsInProgress && !rewards && !map;
        string phase = map ? (multi ? "Map" : "MapChoice") : rewards ? (multi ? "Rewards" : "CardReward") :
            fighting ? "Combat" : run.CurrentRoom switch {
                MerchantRoom => "Shop", RestSiteRoom => "RestSite", TreasureRoom => "Treasure",
                EventRoom when multi && run.CurrentRoom.ModelId?.Entry == "NEOW" => "Neow", _ => "Event"
            };
        var global = new Element("Global") {
            detail = [I(multi ? "MpPhase" : "RunPhase", phase), I("RoomType", run.CurrentRoom?.RoomType.ToString() ?? "None"),
                hasMap ? I("PublicBossId", run.Act.BossEncounter.Id.Entry) : null]
        }.N("act", run.CurrentActIndex).N("floor", run.TotalFloor).N("ascension", run.AscensionLevel)
            .N("participating_seats", run.Players.Count).N("owner_seat", seat);
        if (run.CurrentMapCoord is { } coord) global.N("current_x", coord.col).N("current_y", coord.row);
        if (fighting) global.N("turn", combat!.RoundNumber).N("side", combat.CurrentSide switch {
            CombatSide.Player => 0, CombatSide.Enemy => 1, _ => -1
        });
        int root = snapshot.Add(global);
        int[] seats = new int[run.Players.Count];
        for (int i = 0; i < run.Players.Count; i++) {
            var player = run.Players[i];
            var pcs = fighting ? player.PlayerCombatState : null;
            var row = new Element("Seat") { parent = root, detail = [I("Character", player.Character.Id.Entry)] }
                .N("absolute_seat", i).N("relative_seat", (i + run.Players.Count - seat) % run.Players.Count)
                .N("hp", player.Creature.CurrentHp).N("max_hp", player.Creature.MaxHp)
                .N("max_energy", player.MaxEnergy).N("gold", player.Gold).N("potion_capacity", player.PotionSlots.Count)
                .F("Alive", player.Creature.IsAlive).F("Participating", true);
            if (pcs is not null) row.N("block", player.Creature.Block).N("energy", pcs.Energy).N("stars", pcs.Stars)
                .N("hand_count", pcs.Hand.Cards.Count).N("draw_count", pcs.DrawPile.Cards.Count)
                .N("discard_count", pcs.DiscardPile.Cards.Count).N("exhaust_count", pcs.ExhaustPile.Cards.Count)
                .N("readiness_generation", pcs.TurnNumber).F("Ready", CombatManager.Instance.IsPlayerReadyToEndTurn(player));
            seats[i] = snapshot.Add(row);
            handles[player] = seats[i];
        }
        if (fighting) {
            var creatures = combat!.Creatures.OrderBy(c => c.IsPlayer ? 0 : c.IsPet ? 2 : 1).ToArray();
            foreach (var creature in creatures) {
                int owner = creature.Player is { } player ? seats[run.GetPlayerSlotIndex(player)] :
                    run.Players.Select((p, index) => (p, index))
                        .Where(p => p.p.PlayerCombatState?.Pets.Contains(creature) == true)
                        .Select(p => seats[p.index]).FirstOrDefault();
                var row = new Element("Creature") {
                    parent = root, owner = owner,
                    model = !creature.IsPet && creature.Monster is { } m ? I("MonsterKind", m.Id.Entry) : null,
                    detail = [I("Side", creature.Side)]
                }.N("hp", creature.CurrentHp).N("max_hp", creature.MaxHp).N("block", creature.Block)
                    .F("Alive", creature.IsAlive).F("Targetable", creature.IsHittable).F("Pet", creature.IsPet);
                if (creature.Monster?.NextMove.Intents.FirstOrDefault() is { } intent)
                    row.detail = [I("Side", creature.Side), IntentId(intent)];
                handles[creature] = snapshot.Add(row);
            }
            foreach (var creature in creatures) {
                int parent = handles[creature];
                if (creature.Monster is { } monster)
                    foreach (var (intent, order) in monster.NextMove.Intents.Select((v, i) => (v, i))) {
                        var row = new Element("Intent") {
                            model = IntentId(intent), parent = parent, owner = parent,
                            detail = [IntentId(intent)]
                        }.N("order", order);
                        if (intent is AttackIntent attack)
                            row.N("displayed_damage", attack.GetSingleDamage([run.Players[seat].Creature], creature))
                                .N("displayed_hits", attack.Repeats);
                        if (intent is StatusIntent status) row.N("status_cards", status.CardCount);
                        snapshot.Add(row);
                    }
                foreach (var power in creature.Powers.Where(p => p.IsVisible)) {
                    var row = new Element("Power") { model = I("PowerId", power.Id.Entry), parent = parent, owner = parent,
                        detail = [I("PowerType", power.Type), I("PowerStackType", power.StackType)] }
                        .N("amount", power.Amount).N("display_counter", power.DisplayAmount);
                    int handle = snapshot.Add(row);
                    if (power.Target is { } target && handles.TryGetValue(target, out int targetHandle))
                        snapshot.Edge(handle, targetHandle, "Target");
                }
            }
        }
        for (int i = 0; i < run.Players.Count; i++) {
            var player = run.Players[i];
            int owner = seats[i];
            Pile(snapshot, player.Deck.Cards, "None", owner, handles, player.Relics.Count, player.PotionSlots.Count(p => p is not null));
            foreach (var (relic, index) in player.Relics.Select((r, at) => (r, at))) {
                handles[relic] = Relic(snapshot, relic, owner, owner, index);
            }
            foreach (var (potion, slot) in player.PotionSlots.Select((p, at) => (p, at))) {
                int handle = Potion(snapshot, potion, owner, owner, slot, player.PotionSlots.Count);
                if (potion is not null) handles[potion] = handle;
            }
            if (fighting && player.PlayerCombatState is { } pcs) {
                foreach (var (orb, index) in pcs.OrbQueue.Orbs.Select((o, at) => (o, at)))
                    snapshot.Add(new Element("Orb") { model = I("OrbType", orb.Id.Entry), owner = owner, parent = owner }
                        .N("order", index).N("passive", decimal.ToInt32(orb.PassiveVal)).N("evoke", decimal.ToInt32(orb.EvokeVal)));
                if (i == seat)
                    foreach (var pile in pcs.AllPiles) Pile(snapshot, pile.Cards, pile.Type.ToString(), owner, handles);
            }
        }
        var screen = new ScreenRows();
        if (fighting && inputs.Any(a => a.kind is "Choose" or "ConfirmChoice"))
            SelectionRows(snapshot, run.Players[seat], seats[seat], options, handles, screen, true);
        var mapOptionOrder = new Dictionary<int, int>();
        if (hasMap) {
        if (map && Environment.GetEnvironmentVariable("STS2_LIVE_PARITY_MAP_TRACE") == "1")
            Console.WriteLine(JsonSerializer.Serialize(new {
                live_map_trace = true,
                current = run.CurrentMapCoord,
                root_children = run.Map.StartingMapPoint.Children.Select(p => new[] { p.coord.col, p.coord.row }),
                current_children = run.CurrentMapPoint?.Children.Select(p => new[] { p.coord.col, p.coord.row }),
                start_points = run.Map.startMapPoints.Select(p => new[] { p.coord.col, p.coord.row })
            }));
        var endpoints = new[] { run.Map.StartingMapPoint, run.Map.BossMapPoint }
            .Concat(run.Map.SecondBossMapPoint is { } secondBoss ? [secondBoss] : Array.Empty<MapPoint>());
        var mapPoints = run.Map.GetAllMapPoints().Concat(endpoints)
            .Where(p => ReferenceEquals(run.Map.GetPoint(p.coord), p)).Distinct()
            .OrderBy(p => p.coord.row).ThenBy(p => p.coord.col).ToArray();
        foreach (var point in mapPoints) {
            int visit = run.VisitedMapCoords.ToList().IndexOf(point.coord);
            var row = new Element("MapNode") { detail = [null, I("MapPointType", point.PointType)] }
                .N("x", point.coord.col).N("y", point.coord.row).N("act", run.CurrentActIndex)
                .F("Revealed", true).F("Reachable", inputs.Any(a => a.kind == "Move" &&
                    a.coord?.SequenceEqual(new[] { point.coord.col, point.coord.row }) == true))
                .F("Current", run.CurrentMapCoord == point.coord).F("Visited", visit >= 0);
            if (visit >= 0) row.N("visit_order", visit);
            handles[point] = snapshot.Add(row);
        }
        foreach (var point in mapPoints)
            foreach (var child in point.Children.OrderBy(p => p.coord.row).ThenBy(p => p.coord.col))
                if (handles.TryGetValue(child, out int target)) snapshot.Edge(handles[point], target, "MapConnection");
        var reachable = mapPoints.Where(p => snapshot.rows[handles[p] - 1].flags["Reachable"]).ToArray();
        IEnumerable<MapPoint> originalOrder = run.CurrentMapCoord == run.Map.StartingMapPoint.coord
            ? run.Map.startMapPoints : run.CurrentMapPoint?.Children ?? Enumerable.Empty<MapPoint>();
        var destinations = reachable.Length <= 1 ? reachable : originalOrder.Where(reachable.Contains).ToArray();
        if (destinations.Length != reachable.Length)
            throw new InvalidOperationException("LIVE_MAP_ORDER_UNAVAILABLE");
        foreach (var (point, index) in destinations.Select((p, i) => (p, i))) {
            mapOptionOrder[handles[point]] = index;
            Routes(snapshot, point, handles);
        }
        bool nextBoss = true;
        var bosses = new[] { (run.Map.BossMapPoint, run.Act.BossEncounter) }
            .Concat(run.Map.SecondBossMapPoint is { } secondPoint && run.Act.SecondBossEncounter is { } secondEncounter
                ? [(secondPoint, secondEncounter)] : Array.Empty<(MapPoint, EncounterModel)>());
        foreach (var ((point, encounter), order) in bosses.Select((boss, index) => (boss, index))) {
            bool complete = run.Players.Any(p => p.Creature.CurrentHp > 0) &&
                run.VisitedMapCoords.Contains(point.coord) && (run.CurrentMapCoord != point.coord || !fighting);
            int boss = snapshot.Add(new Element("Boss") {
                model = I("PublicBossId", encounter.Id.Entry), parent = root
            }.N("order", order).N("act", run.CurrentActIndex)
                .F("Complete", complete).F("Current", nextBoss && !complete));
            snapshot.Edge(boss, handles[point], "Target");
            nextBoss &= complete;
        }
        if (multi)
            foreach (var (player, index) in run.Players.Select((player, index) => (player, index)))
                if (CurrentVote(run, player) is { } vote &&
                    run.Map.GetPoint(vote) is { } destination && handles.TryGetValue(destination, out int target))
                    snapshot.Edge(seats[index], target, "Target");
        }
        ScreenState(snapshot, run, seat, seats[seat], options, handles, screen, fighting);
        var encodedOptions = fighting && inputs.Any(a => a.kind is "PlayCard" or "EndTurn" or "UsePotion" or "DiscardPotion")
            ? GameOptionAdapters.CombatWithMasks(run, run.Players[seat], combat!) : options;
        for (int index = 0; index < encodedOptions.Count; index++) {
            var option = encodedOptions[index];
            var input = option.Input;
            int owner = seats[seat];
            if (!handles.TryGetValue(option.Option, out int source)) {
                source = option.Option switch {
                    CardModel card => Card(snapshot, card, owner, owner, "None", null),
                    IReadOnlyList<CardModel> bundle => Bundle(snapshot, bundle, owner, 0, input.index ?? index),
                    MapPoint point => handles[point],
                    RestSiteOption rest => snapshot.Add(new Element("RestService") { owner = owner,
                        detail = [I("RestOptionKind", rest.OptionId)] }),
                    RelicModel relic => Relic(snapshot, relic, owner, owner),
                    PotionModel potion => Potion(snapshot, potion, owner, owner),
                    GoldReward gold => snapshot.Add(new Element("Reward") { owner = owner }
                        .N("amount", gold.Amount).N("reward_order", input.reward_index ?? index)),
                    Reward => snapshot.Add(new Element("Reward") { owner = owner }.N("reward_order", input.reward_index ?? index)),
                    MerchantEntry merchant => snapshot.Add(new Element("Offer") { owner = owner }
                        .N("price", merchant.Cost).N("offer_order", input.index ?? index).F("Sold", !merchant.IsStocked)),
                    _ => owner
                };
                if (source != owner) handles[option.Option] = source;
            }
            if (input.kind is "UsePotion" or "DiscardPotion") source = owner;
            if (ReferenceEquals(option.Option, AiAdvisor.Options.Confirm) && screen.Choice > 0)
                source = screen.Choice;
            if (input.kind is "Skip" or "RerollCardReward" && screen.FocusedReward > 0)
                source = screen.FocusedReward;
            int target = option.Target is not null && handles.TryGetValue(option.Target, out int found) ? found : 0;
            if (screen.Targets.TryGetValue(option.Option, out int choiceTarget)) target = choiceTarget;
            if (input.kind == "UsePotion" && option.Option is PotionModel { TargetType: TargetType.Self })
                target = 0;
            var candidate = new Element("Candidate");
            if (screen.Parameters.TryGetValue(option.Option, out int parameter)) candidate.N("parameter", parameter);
            if (input.kind == "Choose" && screen.Targets.ContainsKey(option.Option))
                candidate.F("Selected", snapshot.rows[screen.Targets[option.Option] - 1].flags["Selected"]);
            if (option.Option is CardModel cardSource && input.kind == "PlayCard") {
                candidate.N("energy_cost", ObservationEnergyCost(cardSource))
                    .N("star_cost", ObservationStarCost(cardSource));
                var preview = Preview(cardSource, option.Target as Creature);
                CardValues(candidate, preview, "display_damage", "display_block");
                if (target > 0) DisplayVariables(snapshot, cardSource, preview, source, owner, target);
            }
            if (input.price is { } price) candidate.N("gold_cost", price);
            if (input.coord is { Length: 2 } destination) candidate.N("x", destination[0]).N("y", destination[1]);
            if (input.kind == "Move" && option.Option is MapPoint) target = source;
            snapshot.AddOption(new(option.Binding, screen.Actions.GetValueOrDefault(option.Option, input.kind),
                source, target, screen.Models.GetValueOrDefault(option.Option, Model(option.Option)),
                candidate.numbers, candidate.flags, option.Legal));
        }
        if (!multi && mapOptionOrder.Count > 0) {
            var ordered = snapshot.options.OrderBy(o => mapOptionOrder.GetValueOrDefault(o.source, int.MaxValue)).ToArray();
            snapshot.options.Clear();
            snapshot.options.AddRange(ordered);
        }
        snapshot.Fingerprint = fingerprint.Finish();
        snapshot.FingerprintWriter = null;
        return snapshot;
    }
}
