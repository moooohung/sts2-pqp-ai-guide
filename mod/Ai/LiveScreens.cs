using System.Runtime.CompilerServices;
using System.Text.Json;
using HarmonyLib;
using MegaCrit.Sts2.Core.CardSelection;
using MegaCrit.Sts2.Core.Context;
using MegaCrit.Sts2.Core.Entities.Players;
using MegaCrit.Sts2.Core.Events;
using MegaCrit.Sts2.Core.Localization;
using MegaCrit.Sts2.Core.Localization.DynamicVars;
using MegaCrit.Sts2.Core.Models;
using MegaCrit.Sts2.Core.Rewards;
using MegaCrit.Sts2.Core.Runs;
using MegaCrit.Sts2.Core.Rooms;
using MegaCrit.Sts2.Core.Entities.Merchant;
using MegaCrit.Sts2.Core.Nodes.Screens.CardSelection;
using MegaCrit.Sts2.Core.Nodes.Screens.Overlays;
using MegaCrit.Sts2.Core.Nodes.Combat;
using MegaCrit.Sts2.Core.Nodes.GodotExtensions;
using MegaCrit.Sts2.Core.TestSupport;

namespace HumanPlayRecorder.Ai;

public static partial class LiveObservation
{
    sealed record SelectionState(CardSelectorPrefs Prefs, string Pile, AbstractModel? Source,
        EnchantmentModel? Enchantment, int Amount, bool SingleCardScreen = false, bool PreviewScreen = false)
    {
        public CardModel[] Selected { get; set; } = [];
        public CardModel[]? Candidates { get; init; }
        public bool? CanCancel { get; init; }
        public bool ManualConfirmation => PreviewScreen || Pile == "Hand" || Prefs.RequireManualConfirmation;
    }
    sealed record RewardLedger(Reward[] Rewards);
    static readonly ConditionalWeakTable<Player, SelectionState> selections = new();
    static readonly ConditionalWeakTable<Player, RewardLedger> rewardLedgers = new();

    internal static void RememberSelection(Player player, CardSelectorPrefs prefs, string pile,
        AbstractModel? source, EnchantmentModel? enchantment = null, int amount = 0)
        => SetSelection(player, new(prefs, pile, source, enchantment, amount));
    internal static void RememberSingleSelection(Player player, bool canSkip, AbstractModel? source)
        => SetSelection(player, new(new CardSelectorPrefs(new LocString("gameplay_ui", "CHOOSE_CARD_HEADER"),
            canSkip ? 0 : 1, 1) { RequireManualConfirmation = false }, "None", source, null, 0, true));
    internal static void RememberDeckSelection(Player player, CardSelectorPrefs prefs,
        AbstractModel? source, EnchantmentModel? enchantment = null, int amount = 0)
        => SetSelection(player, new(prefs, "None", source, enchantment, amount, PreviewScreen: true));
    internal static void RememberHandSelection(Player player, CardSelectorPrefs prefs,
        AbstractModel? source, CardModel[] cards)
        => SetSelection(player, new(prefs, "Hand", source, null, 0) { Candidates = cards });
    static void SetSelection(Player player, SelectionState selection)
    {
        selections.Remove(player);
        selections.Add(player, selection);
    }
    internal static void RememberRewards(RewardsSet rewards)
    {
        foreach (var group in rewards.Rewards.GroupBy(reward => reward.Player)) {
            rewardLedgers.Remove(group.Key);
            rewardLedgers.Add(group.Key, new(group.ToArray()));
        }
    }
    internal static void ForgetSelection(Player player) => selections.Remove(player);
    internal static void RememberSelectedCards(Player player, IEnumerable<CardModel> cards)
    {
        if (!selections.TryGetValue(player, out var selection)) return;
        var current = cards.ToArray();
        if (!selection.Selected.SequenceEqual(current)) RecorderMod.MarkAdvisorDirty();
        selection.Selected = current;
    }
    internal static object? CurrentSelectionScreen() =>
        !TestMode.IsOn && NOverlayStack.Instance?.Peek() is NCardGridSelectionScreen screen ? screen : null;
    internal static IReadOnlyList<CardModel>? ReadSelectionScreen(Player player, object screen)
    {
        FrameCaptureBudget.Check();
        var type = screen.GetType();
        var cards = AccessTools.Field(type, "_cards")?.GetValue(screen) as IReadOnlyList<CardModel>;
        if (cards is not { Count: > 0 } || !ReferenceEquals(cards[0].Owner, player)) return null;
        if (AccessTools.Field(type, "_prefs")?.GetValue(screen) is not CardSelectorPrefs prefs ||
            AccessTools.Field(type, "_selectedCards")?.GetValue(screen) is not IEnumerable<CardModel> selected)
            throw new AdviceUnavailableException(AdviceReason.StateNotExposed, "추천 불가: 게임이 이 선택 화면의 현재 선택 상태를 공개하지 않았습니다.");
        selections.TryGetValue(player, out var previous);
        var source = previous is not null && previous.Prefs.Equals(prefs) ? previous.Source : null;
        var enchantment = AccessTools.Field(type, "_enchantment")?.GetValue(screen) as EnchantmentModel;
        int amount = AccessTools.Field(type, "_enchantmentAmount")?.GetValue(screen) is int value ? value : 0;
        string pile = AccessTools.Field(type, "_pile")?.GetValue(screen) is MegaCrit.Sts2.Core.Entities.Cards.CardPile livePile
            ? livePile.Type.ToString() : "None";
        bool preview = screen is NDeckCardSelectScreen or NDeckEnchantSelectScreen or
            NDeckUpgradeSelectScreen or NDeckTransformSelectScreen;
        bool? canCancel = AccessTools.Field(type, "_closeButton")?.GetValue(screen) is NButton close
            ? close.IsEnabled : null;
        SetSelection(player, new(prefs, pile, source, enchantment, amount, PreviewScreen: preview) {
            Selected = previous?.Selected ?? [], CanCancel = canCancel
        });
        RememberSelectedCards(player, selected);
        return cards;
    }
    internal static IReadOnlyList<CardModel>? ReadHandSelection(Player player, object hand)
    {
        if (!selections.TryGetValue(player, out var previous) || previous.Pile != "Hand" ||
            previous.Candidates is not { } cards)
            throw new AdviceUnavailableException(AdviceReason.StateNotExposed, "추천 불가: 게임이 현재 손패 선택의 후보 목록을 공개하지 않았습니다.");
        var type = hand.GetType();
        if (AccessTools.Field(type, "_prefs")?.GetValue(hand) is not CardSelectorPrefs prefs ||
            AccessTools.Field(type, "_selectedCards")?.GetValue(hand) is not IEnumerable<CardModel> selected)
            throw new AdviceUnavailableException(AdviceReason.StateNotExposed, "추천 불가: 게임이 현재 손패 선택 상태를 공개하지 않았습니다.");
        SetSelection(player, previous with { Prefs = prefs });
        RememberSelectedCards(player, selected);
        return cards;
    }
    internal static IReadOnlyList<AiAdvisor.LiveOption>? CurrentSelectionOptions(RunState run, Player player)
    {
        RecorderMod.CheckUnsupportedChoice(player);
        IReadOnlyList<CardModel>? cards = null;
        if (CurrentSelectionScreen() is { } screen) cards = ReadSelectionScreen(player, screen);
        else if (!TestMode.IsOn && LocalContext.IsMe(player) && NPlayerHand.Instance is { IsInCardSelection: true } hand)
            cards = ReadHandSelection(player, hand);
        return cards is null ? null : SelectionOptions(run.GetPlayerSlotIndex(player), player, cards);
    }
    internal static IReadOnlyList<AiAdvisor.LiveOption> SelectionOptions(int seat, Player player, IReadOnlyList<CardModel> cards)
    {
        if (!selections.TryGetValue(player, out var selection))
            return GameOptionAdapters.Cards(seat, cards, false);
        if (selection.SingleCardScreen)
            return GameOptionAdapters.Cards(seat, cards, selection.Prefs.MinSelect == 0);
        var result = GameOptionAdapters.Cards(seat, cards, false).Where(option =>
            selection.Selected.Contains((CardModel)option.Option) ||
            selection.Selected.Length < selection.Prefs.MaxSelect).ToList();
        bool cancel = selection.Prefs.Cancelable && selection.CanCancel != false;
        if (cancel)
            result.Add(new(AiAdvisor.Options.Skip, JsonSerializer.Serialize(new RecordedAction("Choose", seat,
                indices: [], cards: []), RecordingFormat.Json), AiAdvisor.DecisionAction.Confirm));
        if ((selection.ManualConfirmation || selection.Prefs.MinSelect == 0) &&
            selection.Selected.Length >= selection.Prefs.MinSelect && (selection.Selected.Length > 0 || !cancel)) {
            var indices = selection.Selected.Select(card => Array.IndexOf(cards.ToArray(), card)).ToArray();
            result.Add(new(AiAdvisor.Options.Confirm, JsonSerializer.Serialize(new RecordedAction("ConfirmChoice", seat,
                indices: indices, cards: selection.Selected.Select(card => new CardRef(card.Id.Entry, card.CurrentUpgradeLevel)).ToArray()),
                RecordingFormat.Json), AiAdvisor.DecisionAction.Confirm));
        }
        return result;
    }
    internal static object ConfirmationOption(Player player) =>
        selections.TryGetValue(player, out var selection) && selection.Prefs.MinSelect == 0 &&
        selection.Prefs.Cancelable && selection.CanCancel != false && selection.Selected.Length == 0
            ? AiAdvisor.Options.Skip : AiAdvisor.Options.Confirm;
    sealed class ScreenRows
    {
        public int FocusedReward;
        public int Choice;
        public readonly Dictionary<object, int> Targets = new(ReferenceEqualityComparer.Instance);
        public readonly Dictionary<object, Id?> Models = new(ReferenceEqualityComparer.Instance);
        public readonly Dictionary<object, string> Actions = new(ReferenceEqualityComparer.Instance);
        public readonly Dictionary<object, int> Parameters = new(ReferenceEqualityComparer.Instance);
    }
    static string EventKey(string key) =>
        key.Replace(".pages.", ".", StringComparison.Ordinal).Replace(".options.", ".", StringComparison.Ordinal);

    // The simulator's combat Resolver.step for each selector source (sim content/potions.rs,
    // cards_silent.rs, cards_regent.rs, flow.rs); every other combat selector resolves at step 0.
    static int ResolverStage(AbstractModel? source) => source switch {
        MegaCrit.Sts2.Core.Models.Potions.Ashwater or MegaCrit.Sts2.Core.Models.Powers.StratagemPower
            or MegaCrit.Sts2.Core.Models.Powers.ToolsOfTheTradePower
            or MegaCrit.Sts2.Core.Models.Powers.ForegoneConclusionPower
            or MegaCrit.Sts2.Core.Models.Powers.TyrannyPower => 1,
        MegaCrit.Sts2.Core.Models.Potions.GamblersBrew => 2,
        MegaCrit.Sts2.Core.Models.Potions.TouchOfInsanity => 3,
        MegaCrit.Sts2.Core.Models.Potions.DropletOfPrecognition => 4,
        MegaCrit.Sts2.Core.Models.Potions.LiquidMemories => 5,
        _ => 0
    };
    static void SelectionRows(Snapshot snapshot, Player player, int owner,
        IReadOnlyList<AiAdvisor.LiveOption> options, Dictionary<object, int> handles, ScreenRows screen, bool combat)
    {
        if (CurrentSelectionScreen() is { } current) ReadSelectionScreen(player, current);
        if (!selections.TryGetValue(player, out var selection)) {
            if (FrameCaptureBudget.Active)
                throw new AdviceUnavailableException(AdviceReason.StateNotExposed, "추천 불가: 이 선택 화면의 목적과 선택 범위가 게임 컨텍스트에 없습니다.");
            return;
        }
        string? purpose = selection.Enchantment is not null ? "RunChoiceEffectEnchant" : selection.Source switch {
            CardModel => "ChoiceSrcCard", RelicModel => "ChoiceSrcRelic", PotionModel => "ChoiceSrcPotion", _ => null
        };
        var prefs = selection.Prefs;
        var row = new Element("Choice") {
            owner = owner, model = selection.Source is { } sourceModel ? Model(sourceModel) : null,
            detail = combat ? [purpose is null ? null : I("ChoicePurpose", purpose), null, I("PileKind", selection.Pile)]
                : [purpose is null ? null : I("ChoicePurpose", purpose), purpose is null ? null : I("EffectId", purpose)]
        }.N("minimum", prefs.MinSelect).N("maximum", prefs.MaxSelect).N("selected_count", selection.Selected.Length)
            .N("owner_seat", snapshot.seat)
            .F("Optional", prefs.MinSelect == 0 || prefs.Cancelable)
            // The training encoder marks one-card hand picks single even though the hand UI confirms them.
            .F("SingleSelect", prefs.MaxSelect == 1 && (selection.Pile == "Hand" || !selection.ManualConfirmation))
            .F("Ordered", !combat || selection.Pile == "Hand")
            .F("CanConfirm", selection.Selected.Length >= prefs.MinSelect);
        if (combat) row.N("resolver_stage", ResolverStage(selection.Source));
        int parent = snapshot.Add(row);
        screen.Choice = parent;
        screen.Actions[AiAdvisor.Options.Confirm] = "ConfirmChoice";
        if (options.Any(option => ReferenceEquals(option.Option, AiAdvisor.Options.Skip))) {
            handles[AiAdvisor.Options.Skip] = parent;
            screen.Actions[AiAdvisor.Options.Skip] = "ConfirmChoice";
        }
        if (selection.Enchantment is { } enchantment)
            snapshot.Add(new Element("Modifier") { owner = owner, parent = parent,
                model = I("EnchantmentId", enchantment.Id.Entry) }.N("amount", selection.Amount));
        foreach (var (option, index) in options.Select((option, index) => (option, index))) {
            if (option.Option is not CardModel card) continue;
            int source = combat && selection.Pile != "Hand"
                ? Card(snapshot, card, owner, parent, selection.Pile, null)
                : handles.TryGetValue(card, out int existing) ? existing : Card(snapshot, card, owner, parent, "None", null);
            handles[card] = source;
            int selectedAt = Array.IndexOf(selection.Selected, card);
            var choiceRow = new Element("ChoiceOption") {
                owner = owner, parent = parent, model = Model(card)
            }.N("offer_order", index).F("Selected", selectedAt >= 0);
            if (selectedAt >= 0) choiceRow.N("selected_order", selectedAt).N("retrieval_order", selectedAt);
            int target = snapshot.Add(choiceRow);
            snapshot.Edge(target, source, "Source");
            screen.Targets[card] = target;
        }
    }
    static void EventRows(Snapshot snapshot, RunState run, int seat, int owner,
        IReadOnlyList<AiAdvisor.LiveOption> options, Dictionary<object, int> handles, ScreenRows screen)
    {
        var model = RunManager.Instance.EventSynchronizer.GetEventForPlayer(run.Players[seat]);
        LiveModelData.Item(model, "Event", () => {
            EventContentRows(snapshot, model, run, owner, options, handles, screen);
            return true;
        });
    }
    static void EventContentRows(Snapshot snapshot, EventModel model, RunState run, int owner,
        IReadOnlyList<AiAdvisor.LiveOption> options, Dictionary<object, int> handles, ScreenRows screen)
    {
        var eventId = I("EventId", model.Id.Entry);
        int parent = snapshot.Add(new Element("Event") { owner = owner, model = eventId, detail = [eventId] });
        string pageKey = (model.Description ?? model.InitialDescription).LocEntryKey;
        if (pageKey.EndsWith(".description", StringComparison.Ordinal)) pageKey = pageKey[..^".description".Length];
        var pageId = I("EventPageId", EventKey(pageKey));
        int page = snapshot.Add(new Element("EventPage") { owner = owner, parent = parent, model = pageId, detail = [pageId] });
        foreach (var (option, index) in options.Select((option, index) => (option, index))) {
            FrameCaptureBudget.Check();
            if (option.Option is not EventOption offer) continue;
            LiveModelData.Item(model, "EventOption." + offer.TextKey, () => {
            var input = option.Input;
            int order = input.index ?? index;
            bool neow = model.Id.Entry == "NEOW" && offer.Relic is not null;
            var id = neow ? I("EventOptionId", "NeowPageNameRelicIdEntry") : Model(offer);
            var row = new Element("EventOption") {
                owner = owner, parent = page, model = offer.Relic is { } relic ? Model(relic) : id, detail = [id]
            }.N("option_order", order).F("Locked", offer.IsLocked);
            if (neow) row.N("parameter", order);
            int handle = snapshot.Add(row);
            handles[offer] = handle;
            screen.Models[offer] = neow && run.Players.Count > 1 ? Model(offer.Relic!) : id;
            if (neow && run.Players.Count > 1) screen.Actions[offer] = "ChooseNeow";
            if (neow && run.Players.Count == 1) screen.Parameters[offer] = order;
            if (offer.Relic is { } offered) {
                snapshot.Add(new Element("EffectParameter") { owner = owner, parent = handle,
                    model = I("PublicVariable", "RelicsGain"),
                    detail = [I("EffectId", "EventEffectAddRelic"), I("PublicEventTiming", "Immediate")]
                }.N("amount", 1));
                Relic(snapshot, offered, owner, handle);
            }
            if (model.Id.Entry == "SELF_HELP_BOOK") {
                var enchantment = offer.TextKey switch {
                    "SELF_HELP_BOOK.pages.INITIAL.options.READ_THE_BACK" => ("Sharp", "Enchantment1Amount"),
                    "SELF_HELP_BOOK.pages.INITIAL.options.READ_PASSAGE" => ("Nimble", "Enchantment2Amount"),
                    "SELF_HELP_BOOK.pages.INITIAL.options.READ_ENTIRE_BOOK" => ("Swift", "Enchantment3Amount"),
                    _ => default
                };
                if (enchantment.Item1 is not null)
                    foreach (var (name, amount) in new[] {
                        ("CardsEnchant", 1), ("EnchantmentAmount", decimal.ToInt32(model.DynamicVars[enchantment.Item2].BaseValue))
                    })
                        snapshot.Add(new Element("EffectParameter") { owner = owner, parent = handle,
                            model = I("PublicVariable", name),
                            detail = [null, I("PublicEventTiming", "Immediate"), I("EnchantmentId", enchantment.Item1)]
                        }.N("amount", amount));
            } else if (!neow) {
                foreach (var (name, value) in offer.Description.Variables) {
                    FrameCaptureBudget.Check();
                    decimal? amount = value switch {
                        DynamicVar variable => variable.BaseValue, decimal number => number, int number => number, _ => null
                    };
                    if (amount is not { } numericValue) continue;
                    var parameter = new Element("Modifier") {
                        parent = handle, owner = owner, detail = [I("PublicVariable", name)]
                    };
                    Decimal(parameter, numericValue);
                    snapshot.Add(parameter);
                }
            }
            return true;
            });
        }
    }
    static void RewardRows(Snapshot snapshot, Player player, int owner,
        IReadOnlyList<AiAdvisor.LiveOption> options, Dictionary<object, int> handles, ScreenRows screen)
    {
        var focused = options.Select(option => option.Reward).OfType<CardReward>().FirstOrDefault();
        if (focused is null) return;
        var rewards = rewardLedgers.TryGetValue(player, out var ledger) && ledger.Rewards.Contains(focused)
            ? ledger.Rewards.Where(reward => !reward.SuccessfullySelected).ToArray() : [focused];
        int display = 0;
        foreach (var reward in rewards) {
            string? kind = reward switch { CardReward => "Card", PotionReward => "Potion", RelicReward => "Relic", _ => null };
            if (kind is null) continue;
            bool active = ReferenceEquals(reward, focused);
            var row = new Element("Reward") { owner = owner, detail = [I("RewardKind", kind)] }
                .N("reward_order", display++).F("Selected", active)
                .F("Skippable", reward is not CardReward cards || cards.CanSkip)
                .F("SkipCompletes", reward is not CardReward);
            if (reward is CardReward cardReward)
                row.N("remaining_rerolls", cardReward.CanReroll && player.Relics.Any(r => r.Id.Entry == "DRIFTWOOD" && r.Status.ToString() != "Disabled") ? 1 : 0)
                    .N("remaining_sacrifices", cardReward.CanReroll && player.Relics.Any(r => r.Id.Entry == "PAELS_WING" && r.Status.ToString() != "Disabled") ? 1 : 0);
            int parent = snapshot.Add(row);
            if (reward is PotionReward { Potion: { } potion })
                Potion(snapshot, potion, owner, parent, 0, player.PotionSlots.Count);
            if (reward is RelicReward { Relic: { } relic }) {
                snapshot.Add(new Element("Modifier") { owner = owner, parent = parent, model = Model(relic) }.N("amount", 1));
                Relic(snapshot, relic, owner, parent);
            }
            if (!active) continue;
            screen.FocusedReward = parent;
            foreach (var card in focused.Cards)
                handles[card] = Card(snapshot, card, owner, parent, "None", null);
        }
    }
    // The game empties a relic or potion slot once it is bought (MerchantRelicEntry and MerchantPotionEntry
    // ClearAfterPurchase set Model = null), but the simulator's sold slot keeps naming the item it held and the
    // policy was trained on that offer row. What the game still lists is this visit's purchases
    // (PlayerMapPointHistoryEntry.BoughtRelics / BoughtPotions), in purchase order rather than slot order, and a
    // slot seen while stocked is remembered by slot. A sold slot is named only when those say which item it held.
    static readonly ConditionalWeakTable<MerchantEntry, ModelId> heldOffers = new();

    // Each slot's item, `Known` being a stocked slot's own or a sold slot's remembered one. A sold slot nobody
    // remembers is named by the purchases left over once the remembered ones are taken out, which is only
    // certain when they are all one item (a single such slot included); a restocked slot buys more than it
    // leaves sold, so any other count is ambiguous too.
    internal static ModelId[] SlotIdentities(IReadOnlyList<(bool Sold, ModelId? Known)> slots, IReadOnlyList<ModelId> bought)
    {
        int unnamed = slots.Count(slot => slot.Sold && slot.Known is null);
        var left = bought.ToList();
        foreach (var slot in slots)
            if (slot.Sold && slot.Known is { } remembered) left.Remove(remembered);
        if (unnamed > 0 && (left.Count != unnamed || left.Distinct().Count() != 1))
            throw new AdviceUnavailableException(AdviceReason.StateNotExposed, SoldOfferUnavailable);
        return slots.Select(slot => slot.Known ?? left[0]).ToArray();
    }
    // The one approved reason for a shop whose list cannot be taken yet; a shop whose sold relic or potion cannot be
    // named is refused with it too, so the badge's tooltip never shows wording that was not approved.
    internal const string ShopListUnavailable = "추천 불가: 해당 플레이어의 상점 목록을 동기화하는 중입니다.";
    internal const string SoldOfferUnavailable = ShopListUnavailable;

    static AbstractModel[] Offered<TEntry, TModel>(IReadOnlyList<TEntry> entries, Func<TEntry, TModel?> stocked,
        IReadOnlyList<ModelId> bought) where TEntry : MerchantEntry where TModel : AbstractModel
    {
        foreach (var entry in entries)
            if (stocked(entry) is { } model) heldOffers.AddOrUpdate(entry, model.Id);
        var ids = SlotIdentities([.. entries.Select(entry =>
            (Sold: !entry.IsStocked, Known: heldOffers.TryGetValue(entry, out var held) ? held : null))], bought);
        return [.. entries.Select((entry, index) => (AbstractModel)(stocked(entry) ?? ModelDb.GetById<TModel>(ids[index])))];
    }
    static void ScreenState(Snapshot snapshot, RunState run, int seat, int owner,
        IReadOnlyList<AiAdvisor.LiveOption> options, Dictionary<object, int> handles, ScreenRows screen, bool combat)
    {
        if (options.Any(option => option.Input.kind is "Choose" or "ConfirmChoice")) {
            if (!combat) SelectionRows(snapshot, run.Players[seat], owner, options, handles, screen, false);
            return;
        }
        if (options.Any(option => option.Option is IReadOnlyList<CardModel>)) {
            int parent = run.Players.Count == 1
                ? snapshot.Add(new Element("Reward") { owner = owner, detail = [I("RewardKind", "CardBundles")] }
                    .N("reward_order", 0).F("Selected", true).F("Skippable", false).F("SkipCompletes", true)) : 0;
            foreach (var (option, index) in options.Select((option, index) => (option, index)))
                if (option.Option is IReadOnlyList<CardModel> bundle)
                    handles[bundle] = Bundle(snapshot, bundle, owner, parent, index);
        } else if (options.Any(option => option.Reward is CardReward))
            RewardRows(snapshot, run.Players[seat], owner, options, handles, screen);
        else if (run.CurrentRoom is MerchantRoom merchant) {
            if (seat >= merchant.Inventories.Count)
                throw new AdviceUnavailableException(AdviceReason.ShopSyncing, ShopListUnavailable);
            var inventory = merchant.Inventories[seat];
            void Offer(MerchantEntry entry, int index, object source)
            {
                FrameCaptureBudget.Check();
                var row = new Element("Offer") { owner = owner, model = Model(source) }
                    .N("offer_order", index).F("Sold", !entry.IsStocked)
                    .F("OnSale", entry is MerchantCardEntry { IsOnSale: true });
                if (entry.IsStocked) row.N("price", entry.Cost);
                int parent = snapshot.Add(row);
                handles[source] = parent;
                if (source is CardModel card) {
                    int child = Card(snapshot, card, owner, parent, "None", null);
                    snapshot.Edge(parent, child, "Member");
                    screen.Targets[card] = child;
                } else if (source is RelicModel relic) Relic(snapshot, relic, owner, parent);
                else if (source is PotionModel potion) Potion(snapshot, potion, owner, parent, 0, 0);
            }
            foreach (var (entry, index) in inventory.CardEntries.Select((entry, index) => (entry, index)))
                Offer(entry, index, entry.CreationResult?.Card ?? (object)entry);
            var purchases = run.CurrentMapPointHistoryEntry?.PlayerStats.FirstOrDefault(e => e.PlayerId == run.Players[seat].NetId);
            var relics = Offered(inventory.RelicEntries, entry => entry.Model, purchases?.BoughtRelics ?? []);
            foreach (var (entry, index) in inventory.RelicEntries.Select((entry, index) => (entry, index)))
                Offer(entry, index, relics[index]);
            var potions = Offered(inventory.PotionEntries, entry => entry.Model, purchases?.BoughtPotions ?? []);
            foreach (var (entry, index) in inventory.PotionEntries.Select((entry, index) => (entry, index)))
                Offer(entry, index, potions[index]);
            if (inventory.CardRemovalEntry is { } removal) Offer(removal, 0, removal);
        } else if (options.Any(option => option.Option is EventOption))
            EventRows(snapshot, run, seat, owner, options, handles, screen);
        else if (!combat) RewardRows(snapshot, run.Players[seat], owner, options, handles, screen);
    }
}
