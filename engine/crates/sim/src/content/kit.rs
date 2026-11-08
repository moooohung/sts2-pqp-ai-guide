//! Shared card-effect toolkit. Every character's card file uses these instead of keeping its own
//! copies (r7 repeats `_owner`, `_gain_resolved_block`, the hit loop... in every module).

use crate::card::{CId, KRef};
use crate::combat::{Combat, MAX_HAND_SIZE, PileKind, PilePos, Props};
use crate::hooks::Ev;
use crate::tables::*;

#[inline]
pub fn owner(c: &Combat, k: KRef) -> CId {
    c.cards[k as usize].owner
}

#[inline]
pub fn var(c: &Combat, k: KRef, key: VarKey, default: i32) -> i32 {
    c.cards[k as usize].var(key).unwrap_or(default)
}

#[inline]
pub fn target(t: Option<CId>) -> CId {
    t.expect("enemy-targeted card without target")
}

/// `calculate_damage(base, owner, t, MOVE)` + `apply_damage(t, ..., MOVE, owner)`; returns HP lost.
pub fn hit_base(c: &mut Combat, k: KRef, t: CId, base: i32) -> i32 {
    let who = owner(c, k);
    let dmg = c.calculate_damage(base, Some(who), t, Props::MOVE);
    c.apply_damage(t, dmg, Props::MOVE, Some(who))
}

/// One hit with the card's damage.
pub fn hit(c: &mut Combat, k: KRef, t: CId) -> i32 {
    let base = c.cards[k as usize].damage.unwrap_or(0);
    hit_base(c, k, t, base)
}

/// `n` hits, stopping once the owner or the target is dead.
pub fn hit_n(c: &mut Combat, k: KRef, t: CId, n: i32) {
    let who = owner(c, k);
    for _ in 0..n.max(0) {
        if c.cr(who).is_dead() || c.cr(t).is_dead() {
            break;
        }
        hit(c, k, t);
    }
}

/// One hit on every hittable enemy (snapshot taken first).
pub fn hit_all(c: &mut Combat, k: KRef) {
    c.deal_card_damage_all(k);
}

/// `_gain_resolved_block(owner, calculate_block(base, owner, MOVE, card_source=card))`.
pub fn block_base(c: &mut Combat, k: KRef, base: i32) -> i32 {
    let who = owner(c, k);
    block_target(c, k, who, base)
}

/// C# CreatureCmd.GainBlock(target, card.DynamicVars.Block, cardPlay).
#[cfg(test)]
mod block_tests {
    use super::*;
    use crate::run::{RunManager, RunOptions};

    #[test]
    fn targeted_card_block_refuses_dead_targets_and_combat_end_without_callbacks() {
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        c.add_player(80, 80, 3, 0, &[]);
        c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut crate::rng::Rng::new(7));
        let k = c.new_card(0, CardId::Constellation, false).unwrap();
        c.apply_power_to(1, PowerId::BeaconOfHope, 1, Some(1));
        c.creatures[1].hp = 0;
        assert_eq!(block_target(&mut c, k, 1, 12), 0);
        assert_eq!([c.cr(0).block, c.cr(1).block], [0, 0]);
        c.creatures[1].hp = 80;
        c.is_over = true;
        assert_eq!(block_target(&mut c, k, 1, 12), 0);
        assert_eq!([c.cr(0).block, c.cr(1).block], [0, 0]);
    }
}

/// C# CreatureCmd.GainBlock(target, card.DynamicVars.Block, cardPlay).
pub fn block_target(c: &mut Combat, k: KRef, who: CId, base: i32) -> i32 {
    // C# CreatureCmd.GainBlock refuses dead targets and combat-end reentrancy.
    if c.is_over_or_ending() || c.cr(who).is_dead() {
        return 0;
    }
    let amount = c.calculate_block(base, who, Props::MOVE, Some(k));
    let before = c.cr(who).block;
    c.gain_recorded_block(who,amount);
    let gained = c.cr(who).block - before;
    if gained > 0 {
        c.record_block_gained_event(who, Props::MOVE, c.active_card);
        c.fire(Hook::AfterBlockGained, Ev::BlockGained { creature: who, amount: gained }, None);
    }
    gained
}

/// Block with the card's block.
pub fn block(c: &mut Combat, k: KRef) -> i32 {
    let base = c.cards[k as usize].block.unwrap_or(0);
    block_base(c, k, base)
}

/// `combat.gain_energy(owner, amount)`: refused once the combat is over or ending.
pub fn gain_energy(c: &mut Combat, who: CId, amount: i32) {
    if amount > 0 && !c.is_over_or_ending() {
        c.players[who].energy += amount;
    }
}

/// `combat.apply_power_to(owner, id, amount)` (no applier).
pub fn self_power(c: &mut Combat, k: KRef, id: PowerId, amount: i32) {
    let who = owner(c, k);
    c.apply_power_to(who, id, amount, None);
}

pub fn draw(c: &mut Combat, k: KRef, n: i32) {
    let who = owner(c, k);
    // These OnPlay methods end with an awaited Draw; the parent must wait
    // for nested Hellraiser plays or shuffle selection before finishing.
    if matches!(c.cards[k as usize].id,CardId::Soul|CardId::Fetch) {
        c.draw_cards_at_end_of_play(who, n);
    } else {
        c.draw_cards(who, n, false);
    }
}

/// `add_generated_card_to_creature_discard(owner, make_<id>())` (added by the player).
pub fn status_to_discard(c: &mut Combat, who: CId, id: CardId) {
    c.create_card_in_pile_by_player(who, id, false, PileKind::Discard, PilePos::Bottom);
}

/// `increase_base_damage(card, n)`: the card's damage and its `damage` var.
pub fn increase_damage(c: &mut Combat, k: KRef, n: i32) {
    let card = &mut c.cards[k as usize];
    card.damage = Some(card.damage.unwrap_or(0) + n);
    if let Some(v) = card.vars.iter_mut().find(|(key, _)| *key == VarKey::Damage) {
        v.1 += n;
    }
}

/// `increase_base_block(card, n)`: the card's block and its `block` var.
pub fn increase_block(c: &mut Combat, k: KRef, n: i32) {
    let card = &mut c.cards[k as usize];
    if card.id==CardId::GeneticAlgorithm {card.cv.genetic_algorithm_growth+=n;}
    card.block = Some(card.block.unwrap_or(0) + n);
    if let Some(v) = card.vars.iter_mut().find(|(key, _)| *key == VarKey::Block) {
        v.1 += n;
    }
}

/// `exhaust_card(card)` from whatever pile holds it.
pub fn exhaust(c: &mut Combat, k: KRef) {
    if c.is_over_or_ending() {
        return;
    }
    c.move_card_to_pile(k, PileKind::Exhaust, PilePos::Bottom);
    c.fire_after_card_exhausted(k);
}

/// `move_card_to_creature_hand(owner, card)` for a card already in combat.
pub fn move_to_hand(c: &mut Combat, k: KRef) {
    if c.is_over_or_ending() {
        return;
    }
    let who = owner(c, k);
    let pile = if c.players[who].hand.len() < MAX_HAND_SIZE { PileKind::Hand } else { PileKind::Discard };
    c.move_card_to_pile(k, pile, PilePos::Bottom);
}

/// `transform_card(old, create_card(id, upgraded))`: the new card takes the old one's place in
/// its pile, then entered-combat, changed-piles, and transformed callbacks (CardCmd.Transform).
pub fn transform_in_pile(c: &mut Combat, old: KRef, id: CardId, upgraded: bool) -> Option<KRef> {
    transform_cards_in_pile(c, &[old], id, upgraded).first().copied()
}

/// CardCmd.Transform: remove all originals, add replacements in pile/index order,
/// finish every From/To callback, THEN notify generated-card listeners for the batch.
pub fn transform_cards_in_pile(c: &mut Combat, originals: &[KRef], id: CardId, upgraded: bool) -> Vec<KRef> {
    if c.is_over_or_ending() {
        return Vec::new();
    }
    let mut replacements = Vec::new();
    for &old in originals {
        let who = owner(c, old);
        let p = &c.players[who];
        let location = [(PileKind::Draw, &p.draw[..]), (PileKind::Hand, &p.hand[..]),
            (PileKind::Discard, &p.discard[..]), (PileKind::Exhaust, &p.exhaust[..]),
            (PileKind::Play, &p.play[..])].into_iter()
            .find_map(|(pile, cards)| cards.iter().position(|&q| q == old).map(|index| (pile, index)));
        let Some((pile, index)) = location else {
            c.unported("transform card without a combat pile");
            return Vec::new();
        };
        let Some(new) = c.new_card(who, id, upgraded) else { return Vec::new() };
        c.remove_card_from_piles(old);
        replacements.push((old, new, who, pile, index));
    }
    // C# PileType order differs from the sim's PileKind for Draw/Hand.
    replacements.sort_by_key(|&(_, _, _, pile, index)| {
        let rank = match pile { PileKind::Draw => 1, PileKind::Hand => 2,
            PileKind::Discard => 3, PileKind::Exhaust => 4, PileKind::Play => 5, PileKind::None => 0 };
        (rank, index)
    });
    for &(old, new, who, pile, index) in &replacements {
        let p = &mut c.players[who];
        match pile {
            PileKind::Draw => p.draw.insert(index, new),
            PileKind::Hand => p.hand.insert(index, new),
            PileKind::Discard => p.discard.insert(index, new),
            PileKind::Exhaust => p.exhaust.insert(index, new),
            PileKind::Play => p.play.insert(index, new),
            PileKind::None => unreachable!(),
        }
        c.hist.generated_combat += 1;
        c.hist.generated_by_player_combat += 1;
        c.hist.generated_by_creator[who as usize] += 1;
        c.fire(Hook::AfterCardEnteredCombat, Ev::Card(new), None);
        c.fire_card_after_card_entered_combat(new);
        c.fire_after_card_changed_piles(new, pile, None);
        c.fire_after_transformed(old, new);
    }
    let added: Vec<_> = replacements.iter().map(|&(_, new, _, _, _)| new).collect();
    for &new in &added {
        if c.card_pile(new) != PileKind::None {
            // CardCmd.Transform final loop: creator is the replacement's owner.
            c.fire_after_card_generated(new, true);
        }
    }
    added
}

/// The enemy's current move has an attack intent (`intent.is_attack`).
pub fn intends_to_attack(c: &Combat, t: CId) -> bool {
    let Some(ai) = c.cr(t).ai.as_ref() else { return false };
    let Some(mv) = ai.peek_move() else { return false };
    ai.intents(mv).iter().any(|i| matches!(i.kind, IntentType::Attack | IntentType::MultiAttack | IntentType::DeathBlow))
}

/// `stable_shuffle_cards(cards, shuffle_rng)`: sort by (id name, upgraded), then shuffle.
#[path="dotnet_sort.rs"]
mod dotnet_sort;

pub fn stable_shuffle(c: &mut Combat, cards: &mut [KRef]) {
    dotnet_sort::sort(cards,|a, b| {
        let (x, y) = (&c.cards[a as usize], &c.cards[b as usize]);
        (x.id.name(), x.upgraded).cmp(&(y.id.name(), y.upgraded))
    });
    c.run.rngs.shuffle.shuffle(cards);
}

/// `order_combat_pile_selector_cards(draw, "draw")` (CardSelectCmd.FromCombatPile order):
/// stable sort by rarity rank, then card id name, over `who`'s draw pile.
pub fn draw_selector_order(c: &Combat, who: CId) -> Vec<KRef> {
    let rank = |r: CardRarity| match r {
        CardRarity::Basic => 1u8,
        CardRarity::Common => 2,
        CardRarity::Uncommon => 3,
        CardRarity::Rare => 4,
        CardRarity::Ancient => 5,
        CardRarity::Event => 6,
        CardRarity::Status => 8,
        CardRarity::Curse => 9,
        CardRarity::Quest => 10,
    };
    let mut cards = c.players[who].draw.clone();
    cards.sort_by_key(|&k| {
        let card = &c.cards[k as usize];
        (rank(card.rarity), card.id.name())
    });
    cards
}

/// `insert_card_into_draw_pile(card, random_position=False)`: onto the top of the draw pile.
pub fn put_on_draw_top(c: &mut Combat, k: KRef) {
    if c.is_over_or_ending() {
        return;
    }
    c.move_card_to_pile(k, PileKind::Draw, PilePos::Top);
}

/// `count_cards_played_this_turn(owner, card_type)`.
pub fn plays_finished_of_type(c: &Combat, who: CId, t: CardType) -> i32 {
    c.hist.played_this_turn.iter().filter(|&&(actor, q)| actor == who && c.cards[q as usize].card_type == t).count() as i32
}

/// `count_card_plays_finished_this_turn(owner)`.
pub fn plays_finished(c: &Combat, who: CId) -> i32 {
    c.hist.played_this_turn.iter().filter(|&&(actor, _)| actor == who).count() as i32
}
