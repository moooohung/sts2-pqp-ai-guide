//! Frozen-r7 merchant inventory and run-side merchant relic hooks.
use crate::character::CardPool;
use crate::character::Character;

use arrayvec::ArrayVec;

use crate::card::Card;
use crate::combat::RunCtx;
use crate::relic::Relic;
use crate::rng::Rng;
use crate::tables::*;

use super::manager::RelicBags;

pub(super) const SOLD_OUT: i32 = 999_999;

#[derive(Clone, Debug)]
pub(super) struct ShopCardEntry {
    pub card: Option<Card>,
    pub card_type: &'static str,
    pub on_sale: bool,
    pub price: i32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ShopRelicEntry {
    pub relic_id: RelicId,
    pub rarity: RelicRarity,
    pub price: i32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ShopPotionEntry {
    pub potion_id: PotionId,
    pub rarity: PotionRarity,
    pub price: i32,
}

#[derive(Clone, Debug)]
pub(super) struct ShopState {
    pub character: crate::character::Character,
    pub multiplayer: bool,
    pub cards: ArrayVec<ShopCardEntry, 5>,
    pub colorless_cards: ArrayVec<ShopCardEntry, 2>,
    pub relics: ArrayVec<ShopRelicEntry, 3>,
    pub potions: ArrayVec<ShopPotionEntry, 3>,
    pub removal_cost: i32,
    pub removal_used: bool,
}

impl Default for ShopState {
    fn default() -> Self {
        Self {
            character: Character::Ironclad,
            multiplayer: false,
            cards: ArrayVec::new(),
            colorless_cards: ArrayVec::new(),
            relics: ArrayVec::new(),
            potions: ArrayVec::new(),
            removal_cost: 75,
            removal_used: false,
        }
    }
}

#[inline]
fn py_round(x: f64) -> i32 {
    x.round_ties_even() as i32
}

#[inline]
fn ranged(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * rng.next_float(1.0)
}

fn merchant_price_modifiers(run: &RunCtx, mut price: i32) -> i32 {
    for relic in run.owned_relics(crate::combat::PLAYER).filter(|r| r.enabled) {
        price = match relic.id {
            RelicId::MembershipCard => py_round(price as f64 * 0.50),
            RelicId::TheCourier => py_round(price as f64 * 0.80),
            _ => price,
        };
    }
    price.max(0)
}

fn card_price(
    run: &RunCtx,
    rarity: CardRarity,
    rng: &mut Rng,
    is_colorless: bool,
    on_sale: bool,
) -> i32 {
    let mut base = match rarity {
        CardRarity::Rare => 150,
        CardRarity::Uncommon => 75,
        _ => 50,
    };
    if is_colorless {
        base = py_round(base as f64 * 1.15);
    }
    let mut price = py_round(base as f64 * ranged(rng, 0.95, 1.05));
    if on_sale {
        price /= 2;
    }
    merchant_price_modifiers(run, price)
}

fn relic_price(run: &RunCtx, id: RelicId, rarity: RelicRarity, rng: &mut Rng) -> i32 {
    let base = if id == RelicId::Circlet {
        1
    } else {
        match rarity {
            RelicRarity::Common => 175,
            RelicRarity::Uncommon => 225,
            RelicRarity::Rare => 275,
            RelicRarity::Shop => 200,
            _ => 200,
        }
    };
    merchant_price_modifiers(run, py_round(base as f64 * ranged(rng, 0.85, 1.15)))
}

fn potion_price(run: &RunCtx, rarity: PotionRarity, rng: &mut Rng) -> i32 {
    let base = match rarity {
        PotionRarity::Rare => 100,
        PotionRarity::Uncommon => 75,
        _ => 50,
    };
    merchant_price_modifiers(run, py_round(base as f64 * ranged(rng, 0.95, 1.05)))
}

pub(super) fn shop_card_rarity(run: &mut RunCtx, offset: f64) -> CardRarity {
    // C# CardRarityOdds.ShopRareOdds / RollWithoutChangingFutureOdds(Shop).
    let roll = run.rngs.rewards.next_float(1.0) as f32;
    shop_card_rarity_with_roll(run.ascension, offset as f32, roll)
}

fn shop_card_rarity_with_roll(ascension: i32, offset: f32, roll: f32) -> CardRarity {
    let (uncommon, rare) = if ascension >= 7 {
        (0.37, 0.045)
    } else {
        (0.37, 0.09)
    };
    let rare_threshold = rare + offset;
    if roll < rare_threshold {
        CardRarity::Rare
    } else if roll < uncommon + rare_threshold {
        CardRarity::Uncommon
    } else {
        CardRarity::Common
    }
}

pub(super) fn upgrade_template(run: &mut RunCtx, card: &mut Card) {
    if card.upgraded {
        return;
    }
    // C# CardCmd.Upgrade preserves the card object. The r7 wire identity
    // model constructs an upgraded template (run_state.py::upgrade_card_instance):
    // one factory allocation, plus the reference factory on a cold cache miss.
    // capture_self_mutating_card_progress runs before upgrading: its key is
    // (id, false), shared with Smith / RunCtx::upgrade_deck_card.
    if run.ref_cache_miss(card.id, false) {
        run.new_card_id();
    }
    run.new_card_id();
    // r7 allocates the attempted upgraded factory even for an unupgradable card.
    // C# EggRelicHelper/SilverCrucible skip such cards; these extra identities are
    // wire-format compatibility only, not additional C# card objects.
    if let Some(up) = Card::from_factory(card.id, true, card.instance_id)
        && up.upgraded
    {
        // MadScience.OnUpgrade only adds Innate; its selected type/rider and
        // dynamic vars are saved instance state, not the canonical Attack.
        if card.id == CardId::MadScience {
            card.upgraded = true;
            card.keywords |= KW_INNATE;
            return;
        }
        let iid = card.instance_id;
        let floor = card.floor_added_to_deck;
        *card = up;
        card.instance_id = iid;
        card.floor_added_to_deck = floor;
    }
}

fn apply_card_creation_hooks(run: &mut RunCtx, card: &mut Card) {
    let relic_ids: ArrayVec<RelicId, 64> = run.owned_relics(card.owner)
        .filter(|r| r.enabled).map(|r| r.id).collect();
    for id in relic_ids {
        let upgrades = match id {
            RelicId::FrozenEgg => card.card_type == CardType::Power,
            RelicId::MoltenEgg => card.card_type == CardType::Attack,
            RelicId::ToxicEgg => card.card_type == CardType::Skill,
            _ => false,
        };
        if upgrades {
            upgrade_template(run, card);
        }
        // FresnelLens.ModifyMerchantCardCreationResults calls Nimble.CanEnchant,
        // whose predicate is GainsBlock (Nimble.cs:7-14), not every shop card.
        if id == RelicId::FresnelLens
            && super::manager::RunManager::event_can_enchant(card, "Nimble")
        {
            let _ = crate::card::enchant::apply_static_enchantment(card, "Nimble", 2);
        }
    }
}

#[cfg(test)]
mod fresnel_review_tests {
    use super::*;

    #[test]
    fn merchant_rarity_threshold_equality_is_float32_not_double() {
        // Initial pity offset: C# uncommon+rareThreshold equals float32(.41).
        // The former double expression is larger and incorrectly yields Uncommon.
        let offset = -0.05f32;
        let roll = 0.41f32;
        assert_eq!(0.37f32 + (0.09f32 + offset), roll);
        assert!(0.37f64 + 0.09f64 + offset as f64 > roll as f64);
        assert_eq!(shop_card_rarity_with_roll(0, offset, roll), CardRarity::Common);
    }

    #[test]
    fn merchant_lens_respects_block_and_existing_enchantment_predicates() {
        let mut run = super::super::manager::RunManager::new(
            4100150, super::super::manager::RunOptions::default()).run_ctx().clone();
        run.relics.push(crate::relic::Relic::new(RelicId::FresnelLens));
        let mut attack = Card::from_factory(CardId::Rampage, false, 1).unwrap();
        apply_card_creation_hooks(&mut run, &mut attack);
        assert!(attack.enchantments.is_empty());
        let mut block = Card::from_factory(CardId::IronWave, false, 2).unwrap();
        apply_card_creation_hooks(&mut run, &mut block);
        assert!(block.enchantments.contains(&(crate::identity::EnchantmentId::Nimble, 2)));
        let mut enchanted = Card::from_factory(CardId::DefendIronclad, false, 3).unwrap();
        crate::card::enchant::apply_static_enchantment(&mut enchanted, "Glam", 1);
        apply_card_creation_hooks(&mut run, &mut enchanted);
        assert!(!enchanted.enchantments.iter().any(|(name, _)| *name == crate::identity::EnchantmentId::Nimble));
    }
}

pub(super) fn make_character_card(
    run: &mut RunCtx,
    character: crate::character::CardPool,
    multiplayer: bool,
    rarity_offset: f64,
    card_type: CardType,
    type_name: &'static str,
    stocked: &[CardId],
    on_sale: bool,
) -> Result<ShopCardEntry, &'static str> {
    // C# CardFactory.CreateForMerchant uses Player.Character.CardPool's
    // unlocked cards, not FilterForCombat (which excludes e.g. NotYet).
    let pool = super::card_pools::cards(character, multiplayer)?;
    let mut rarity = shop_card_rarity(run, rarity_offset);
    let next = |r| match r {
        CardRarity::Common => CardRarity::Uncommon,
        CardRarity::Uncommon => CardRarity::Rare,
        _ => CardRarity::Common,
    };
    let mut candidates: ArrayVec<CardId, 64> = ArrayVec::new();
    for _ in 0..3 {
        candidates.clear();
        candidates.extend(pool.iter().copied().filter(|id| {
            !stocked.contains(id)
                && card_info(*id, false).is_some_and(|info| info.card_type == card_type)
                && card_info(*id, false).is_some_and(|info| info.rarity != CardRarity::Basic)
                && card_info(*id, false).is_some_and(|info| info.rarity == rarity)
        }));
        if !candidates.is_empty() {
            break;
        }
        rarity = next(rarity);
    }
    if candidates.is_empty() {
        return Err("shop card pool exhausted");
    }
    let idx = run.rngs.shops.choice_index(candidates.len());
    let card_id = candidates[idx];
    let iid = run.new_card_id();
    let mut card = Card::from_factory(card_id, false, iid).ok_or("shop card factory")?;
    run.rngs.rewards.next_float(1.0);
    apply_card_creation_hooks(run, &mut card);
    let price = {
        let mut rng = run.rngs.shops.clone();
        let price = card_price(run, card.rarity, &mut rng, false, false);
        run.rngs.shops = rng;
        price
    };
    let mut entry = ShopCardEntry {
        card: Some(card),
        card_type: type_name,
        on_sale: false,
        price,
    };
    if on_sale {
        entry.on_sale = true;
        let mut rng = run.rngs.shops.clone();
        entry.price = card_price(
            run,
            entry.card.as_ref().unwrap().rarity,
            &mut rng,
            false,
            true,
        );
        run.rngs.shops = rng;
    }
    Ok(entry)
}

pub(super) fn make_colorless_card(
    run: &mut RunCtx,
    multiplayer: bool,
    rarity: CardRarity,
    stocked: &[CardId],
) -> Result<ShopCardEntry, &'static str> {
    let pool = super::card_pools::cards(CardPool::Colorless, multiplayer)?;
    let candidates: ArrayVec<CardId, 64> = pool
        .iter()
        .copied()
        .filter(|id| !stocked.contains(id)
            && card_info(*id, false).is_some_and(|info| info.rarity == rarity))
        .collect();
    if candidates.is_empty() {
        return Err("colorless shop pool exhausted");
    }
    let id = candidates[run.rngs.shops.choice_index(candidates.len())];
    let iid = run.new_card_id();
    let mut card = Card::from_factory(id, false, iid).ok_or("colorless shop factory")?;
    run.rngs.rewards.next_float(1.0);
    apply_card_creation_hooks(run, &mut card);
    let mut rng = run.rngs.shops.clone();
    let price = card_price(run, card.rarity, &mut rng, true, false);
    run.rngs.shops = rng;
    Ok(ShopCardEntry {
        card: Some(card),
        card_type: "Colorless",
        on_sale: false,
        price,
    })
}

fn bucket_order(rarity: RelicRarity) -> &'static [usize] {
    match rarity {
        RelicRarity::Shop => &[3, 0, 1, 2],
        RelicRarity::Common => &[0, 1, 2],
        RelicRarity::Uncommon => &[1, 2],
        RelicRarity::Rare => &[2],
        _ => &[],
    }
}

pub(super) fn pull_shop_relic(bags: &mut RelicBags, rarity: RelicRarity, excluded: &[RelicId]) -> RelicId {
    let mut chosen = None;
    for &bucket in bucket_order(rarity) {
        chosen = bags.player[0][bucket]
            .iter()
            .rev()
            .copied()
            .find(|id| !excluded.contains(id) && RELIC_INFO[id.index()].shops);
        if chosen.is_some() {
            break;
        }
    }
    let id = chosen.unwrap_or(RelicId::Circlet);
    for bag in &mut bags.player[0] {
        bag.retain(|x| *x != id);
    }
    for bag in &mut bags.shared {
        bag.retain(|x| *x != id);
    }
    id
}

pub(super) fn roll_relic_rarity(rng: &mut Rng) -> RelicRarity {
    let roll = rng.next_float(1.0);
    if roll < 0.50 {
        RelicRarity::Common
    } else if roll < 0.829_999_983_310_699_5 {
        RelicRarity::Uncommon
    } else {
        RelicRarity::Rare
    }
}

pub(super) fn roll_potion_rarity(rng: &mut Rng) -> PotionRarity {
    let roll = rng.next_float(1.0);
    if roll <= 0.100_000_001_490_116_12 {
        PotionRarity::Rare
    } else if roll <= 0.349_999_994_039_535_5 {
        PotionRarity::Uncommon
    } else {
        PotionRarity::Common
    }
}

fn potion_pool() -> Vec<PotionId> {
    let mut character = Vec::new();
    let mut shared = Vec::new();
    for id in PotionId::ALL {
        let info = &POTION_INFO[id.index()];
        if !matches!(
            info.rarity,
            PotionRarity::Common | PotionRarity::Uncommon | PotionRarity::Rare
        ) {
            continue;
        }
        if info.character_specific {
            if info.character_pool == Some("Ironclad") {
                character.push(id);
            }
        } else {
            shared.push(id);
        }
    }
    character.extend(shared);
    character
}

pub(super) fn generate(
    state: &mut ShopState,
    run: &mut RunCtx,
    character: crate::character::Character,
    multiplayer: bool,
    bags: &mut RelicBags,
    rarity_offset: f64,
    removals_used: u32,
) -> Result<(), &'static str> {
    *state = ShopState::default();
    state.character = character;
    state.multiplayer = multiplayer;
    let sale_index = run.rngs.shops.next_int(0, 4) as usize;
    let types = [
        (CardType::Attack, "Attack"),
        (CardType::Attack, "Attack"),
        (CardType::Skill, "Skill"),
        (CardType::Skill, "Skill"),
        (CardType::Power, "Power"),
    ];
    let mut stocked: ArrayVec<CardId, 7> = ArrayVec::new();
    for (i, (ty, name)) in types.into_iter().enumerate() {
        let entry = make_character_card(run, character.into(), multiplayer, rarity_offset, ty, name, &stocked, i == sale_index)?;
        stocked.push(entry.card.as_ref().unwrap().id);
        state.cards.push(entry);
    }
    for rarity in [CardRarity::Uncommon, CardRarity::Rare] {
        let entry = make_colorless_card(run, multiplayer, rarity, &stocked)?;
        stocked.push(entry.card.as_ref().unwrap().id);
        state.colorless_cards.push(entry);
    }

    // MerchantInventory rolls both reward-stream rarities before any relic pricing.
    let rarities = [
        roll_relic_rarity(&mut run.rngs.rewards),
        roll_relic_rarity(&mut run.rngs.rewards),
        RelicRarity::Shop,
    ];
    let mut excluded: Vec<RelicId> = run.owned_relics(crate::combat::PLAYER).map(|r| r.id).collect();
    for rarity in rarities {
        let id = pull_shop_relic(bags, rarity, &excluded);
        excluded.push(id);
        let actual = RELIC_INFO[id.index()].rarity;
        let mut rng = run.rngs.shops.clone();
        let price = relic_price(run, id, actual, &mut rng);
        run.rngs.shops = rng;
        state.relics.push(ShopRelicEntry {
            relic_id: id,
            rarity: actual,
            price,
        });
    }

    // PotionFactory.GetPotionOptions: owning character's unlocked epoch,
    // then SharedPotionPool, in source order (not PotionId enum order).
    let mut pool = super::potion_pools::cards(character)
        .ok_or("merchant potion owner pool missing")?;
    let mut selected: ArrayVec<PotionId, 3> = ArrayVec::new();
    for _ in 0..3 {
        let rarity = roll_potion_rarity(&mut run.rngs.shops);
        let candidates: Vec<usize> = pool
            .iter()
            .enumerate()
            .filter_map(|(i, id)| (POTION_INFO[id.index()].rarity == rarity).then_some(i))
            .collect();
        let pick = candidates[run.rngs.shops.choice_index(candidates.len())];
        selected.push(pool.remove(pick));
    }
    for id in selected {
        let rarity = POTION_INFO[id.index()].rarity;
        let mut rng = run.rngs.shops.clone();
        let price = potion_price(run, rarity, &mut rng);
        run.rngs.shops = rng;
        state.potions.push(ShopPotionEntry {
            potion_id: id,
            rarity,
            price,
        });
    }

    let removal = if run.ascension >= 6 {
        100 + 50 * removals_used as i32
    } else {
        75 + 25 * removals_used as i32
    };
    state.removal_cost = merchant_price_modifiers(run, removal);
    Ok(())
}

fn upgrade_random(run: &mut RunCtx, ty: CardType, count: usize) {
    let mut candidates = Vec::new();
    for index in 0..run.deck.len() {
        if run.is_deck_card_upgradable(index) && run.deck[index].card_type == ty {
            candidates.push(index as crate::card::KRef);
        }
    }
    crate::flow::dotnet_sort_cards(&mut candidates, &run.deck);
    run.rngs.niche.shuffle(&mut candidates);
    for index in candidates.into_iter().take(count) {
        let _ = run.upgrade_deck_card(index as usize);
    }
}

pub(super) fn obtained_relic(run: &RunCtx, id: RelicId) -> Relic {
    // C# RelicCmd.Obtain records this floor before invoking pickup hooks.
    let mut relic=Relic::new(id);
    relic.floor_added_to_deck=run.total_floor;
    relic
}

pub(super) fn attach_relic(run: &mut RunCtx, id: RelicId) -> usize {
    let index=run.relics.len();
    let relic=obtained_relic(run,id);
    run.relics.push(relic);
    index
}

pub(super) fn add_relic(run: &mut RunCtx, id: RelicId) -> Result<(), &'static str> {
    attach_relic(run,id);
    match id {
        RelicId::Strawberry => {
            run.max_hp += 7;
            run.hp += 7;
        }
        RelicId::Pear => {
            run.max_hp += 10;
            run.hp += 10;
        }
        RelicId::Mango => {
            run.max_hp += 14;
            run.hp += 14;
        }
        RelicId::LeesWaffle => {
            run.max_hp += 7;
            run.hp = run.max_hp;
        }
        RelicId::OldCoin => { super::treasure::gain_gold(run, 300); }
        RelicId::PotionBelt => run.max_potion_slots += 2,
        RelicId::WarPaint => upgrade_random(run, CardType::Skill, 2),
        RelicId::Whetstone => upgrade_random(run, CardType::Attack, 2),
        _ if RELIC_INFO[id.index()].upon_pickup => {
            return Err("relic pickup followup");
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn post_purchase(run: &mut RunCtx, gold_spent: i32) -> bool {
    let mut refill = false;
    for relic in run.owned_relics_mut(crate::combat::PLAYER) {
        match relic.id {
            RelicId::MawBank if gold_spent > 0 => relic.flag1 = true,
            RelicId::TheCourier => refill = true,
            _ => {}
        }
    }
    refill
}

pub(super) fn procure_potion(run: &mut RunCtx, id: PotionId) -> bool {
    if run.has_owned_relic(crate::combat::PLAYER, RelicId::Sozu) {
        return false;
    }
    if let Some(slot) = run.potions.iter_mut().find(|x| x.is_none()) {
        *slot = Some(id);
        return true;
    }
    if run.potions.len() < run.max_potion_slots as usize {
        run.potions.push(Some(id));
        return true;
    }
    false
}

pub(super) fn random_potion(run: &mut RunCtx) -> Option<PotionId> {
    let pool = potion_pool();
    if pool.is_empty() {
        return None;
    }
    let rarity = roll_potion_rarity(&mut run.rngs.combat_potion_generation);
    let candidates: Vec<PotionId> = pool
        .iter()
        .copied()
        .filter(|id| POTION_INFO[id.index()].rarity == rarity)
        .collect();
    let source = if candidates.is_empty() {
        &pool
    } else {
        &candidates
    };
    Some(source[run.rngs.combat_potion_generation.choice_index(source.len())])
}

pub(super) fn refill_card(
    state: &mut ShopState,
    index: usize,
    run: &mut RunCtx,
    rarity_offset: f64,
) -> Result<(), &'static str> {
    let stocked: Vec<CardId> = state
        .cards
        .iter()
        .chain(state.colorless_cards.iter())
        .filter_map(|entry| entry.card.as_ref().map(|card| card.id))
        .collect();
    if index < state.cards.len() {
        let old_type = state.cards[index].card_type;
        let ty = match old_type {
            "Attack" => CardType::Attack,
            "Skill" => CardType::Skill,
            "Power" => CardType::Power,
            _ => return Err("invalid shop card type"),
        };
        state.cards[index] =
            make_character_card(run, state.character.into(), state.multiplayer, rarity_offset, ty, old_type, &stocked, false)?;
        return Ok(());
    }
    let colorless = index
        .checked_sub(state.cards.len())
        .ok_or("invalid shop card index")?;
    let old = state
        .colorless_cards
        .get(colorless)
        .ok_or("invalid shop colorless index")?;
    let rarity = old
        .card
        .as_ref()
        .map_or(CardRarity::Uncommon, |card| card.rarity);
    state.colorless_cards[colorless] = make_colorless_card(run, state.multiplayer, rarity, &stocked)?;
    Ok(())
}

pub(super) fn refill_relic(
    state: &mut ShopState,
    index: usize,
    run: &mut RunCtx,
    bags: &mut RelicBags,
) -> Result<(), &'static str> {
    if index >= state.relics.len() {
        return Err("invalid shop relic index");
    }
    let rarity = roll_relic_rarity(&mut run.rngs.rewards);
    let mut excluded: Vec<RelicId> = run.owned_relics(crate::combat::PLAYER).map(|r| r.id).collect();
    excluded.extend(
        state
            .relics
            .iter()
            .filter(|entry| entry.price < SOLD_OUT)
            .map(|entry| entry.relic_id),
    );
    let id = pull_shop_relic(bags, rarity, &excluded);
    let actual = RELIC_INFO[id.index()].rarity;
    let mut rng = run.rngs.shops.clone();
    let price = relic_price(run, id, actual, &mut rng);
    run.rngs.shops = rng;
    state.relics[index] = ShopRelicEntry {
        relic_id: id,
        rarity: actual,
        price,
    };
    Ok(())
}

pub(super) fn refill_potion(
    state: &mut ShopState,
    index: usize,
    run: &mut RunCtx,
) -> Result<(), &'static str> {
    if index >= state.potions.len() {
        return Err("invalid shop potion index");
    }
    let rarity = roll_potion_rarity(&mut run.rngs.shops);
    // MerchantPotionEntry.RestockAfterPurchase passes an empty blacklist.
    let pool = super::potion_pools::cards(state.character)
        .ok_or("merchant potion owner pool missing")?;
    let candidates: Vec<PotionId> = pool
        .into_iter()
        .filter(|id| POTION_INFO[id.index()].rarity == rarity)
        .collect();
    let id = candidates[run.rngs.shops.choice_index(candidates.len())];
    let mut rng = run.rngs.shops.clone();
    let price = potion_price(run, rarity, &mut rng);
    run.rngs.shops = rng;
    state.potions[index] = ShopPotionEntry {
        potion_id: id,
        rarity,
        price,
    };
    Ok(())
}
