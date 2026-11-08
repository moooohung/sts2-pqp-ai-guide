//! Frozen-r7 card enchantment behavior from cards/enchantments.py.

use crate::card::{CId, Card, KRef};
use crate::combat::{Combat, Props};
use crate::hooks::Ev;
use crate::tables::*;

const ADROIT: EnchantmentId = EnchantmentId::Adroit;
const CORRUPTED: EnchantmentId = EnchantmentId::Corrupted;
const FAVORED: EnchantmentId = EnchantmentId::Favored;
const GLAM: EnchantmentId = EnchantmentId::Glam;
const GOOPY: EnchantmentId = EnchantmentId::Goopy;
const IMBUED: EnchantmentId = EnchantmentId::Imbued;
const INKY: EnchantmentId = EnchantmentId::Inky;
const MOMENTUM: EnchantmentId = EnchantmentId::Momentum;
const NIMBLE: EnchantmentId = EnchantmentId::Nimble;
const PERFECT_FIT: EnchantmentId = EnchantmentId::PerfectFit;
const SHARP: EnchantmentId = EnchantmentId::Sharp;
const SLITHER: EnchantmentId = EnchantmentId::Slither;
const SLUMBERING_ESSENCE: EnchantmentId = EnchantmentId::SlumberingEssence;
const SOWN: EnchantmentId = EnchantmentId::Sown;
const SPIRAL: EnchantmentId = EnchantmentId::Spiral;
const SWIFT: EnchantmentId = EnchantmentId::Swift;
const VIGOROUS: EnchantmentId = EnchantmentId::Vigorous;

#[inline]
fn amount(card: &Card, name: EnchantmentId) -> i32 {
    card.enchantments
        .iter()
        .find_map(|&(n, v)| (n == name).then_some(v))
        .unwrap_or(0)
}

#[inline]
fn has(card: &Card, name: EnchantmentId) -> bool {
    card.enchantments.iter().any(|&(n, _)| n == name)
}

fn set_amount(card: &mut Card, name: EnchantmentId, value: i32) {
    if let Some((_, amount)) = card.enchantments.iter_mut().find(|(n, _)| *n == name) {
        *amount = value;
    }
}

/// r7 apply_static_enchantment. Run-layer callers can use this when an enchantment is added.
#[allow(dead_code)]
pub(crate) fn apply_static_enchantment(card: &mut Card, name: impl TryInto<EnchantmentId>, value: i32) -> bool {
    let Ok(name) = name.try_into() else { return false; };
    if let Some((_, amount)) = card.enchantments.iter_mut().find(|(n, _)| *n == name) {
        *amount = value;
    } else if card.enchantments.try_push((name, value)).is_err() {
        return false;
    }
    match name {
        EnchantmentId::RoyallyApproved => card.keywords |= KW_INNATE | KW_RETAIN,
        EnchantmentId::Steady => card.keywords |= KW_RETAIN,
        EnchantmentId::TezcatarasEmber => {
            card.keywords |= KW_ETERNAL;
            card.set_base_energy_cost(0);
        }
        EnchantmentId::SoulsPower => card.keywords &= !KW_EXHAUST,
        // Models/Enchantments/Instinct.cs:7-21 has no ModifyCard/OnEnchant:
        // only powered attack damage is doubled, never the energy cost.
        // Frozen r7 cards/enchantments.py:53-57 instead lowers base cost by one.
        GOOPY => card.keywords |= KW_EXHAUST,
        _ => {}
    }
    true
}

/// r7 reset_combat_enchantments: only enchantment combat vars are reset.
pub(crate) fn reset_combat(card: &mut Card) {
    card.cv.enchant_momentum_bonus = 0;
    card.cv.enchant_used_vigorous = false;
    card.cv.enchant_used_glam = false;
    card.cv.enchant_used_swift = false;
    card.cv.enchant_used_sown = false;
}

#[inline]
pub(crate) fn damage_additive(card: &Card, props: Props) -> i32 {
    if !props.is_powered_attack() {
        return 0;
    }
    amount(card, SHARP)
        // C# TezcatarasEmber.cs:13,21-27: fixed +3, not Amount-scaled,
        // before powered-attack multipliers (including Vulnerable).
        + if has(card, EnchantmentId::TezcatarasEmber) { 3 } else { 0 }
        + card.cv.enchant_momentum_bonus
        + if card.cv.enchant_used_vigorous {
            0
        } else {
            amount(card, VIGOROUS)
        }
}

#[inline]
pub(crate) fn damage_multiplicative(card: &Card, props: Props) -> f64 {
    if !props.is_powered_attack() {
        return 1.0;
    }
    // Models/Enchantments/Instinct.cs:13-20: multiplier 2 for powered attacks.
    if has(card, FAVORED) || has(card, EnchantmentId::Instinct) {
        2.0
    } else if has(card, CORRUPTED) {
        1.5
    } else {
        1.0
    }
}

#[inline]
pub(crate) fn block_additive(card: &Card, props: Props) -> i32 {
    if !props.is_powered_block() {
        return 0;
    }
    amount(card, NIMBLE)
        + if has(card, GOOPY) {
            (amount(card, GOOPY) - 1).max(0)
        } else {
            0
        }
}

#[inline]
pub(crate) fn play_count_bonus(card: &Card) -> i32 {
    amount(card, SPIRAL)
        + if has(card, GLAM) && !card.cv.enchant_used_glam {
            amount(card, GLAM)
        } else {
            0
        }
}

pub(crate) fn on_card_drawn(c: &mut Combat, k: KRef) {
    if has(&c.cards[k as usize], SLITHER) {
        let cost = c.run.rngs.combat_energy_costs.next_int(0, 3) as i32;
        c.cards[k as usize].set_energy_cost(cost, crate::card::CostExpiry::Combat, false);
    }
}

fn auto_play_card(c: &mut Combat, k: KRef) {
    if c.is_over_or_ending() {
        return;
    }
    let owner = c.cards[k as usize].owner;
    if c.cards[k as usize].is_unplayable() {
        c.move_card_to_pile(k, crate::combat::PileKind::Discard, crate::combat::PilePos::Bottom);
        return;
    }
    let id = c.cards[k as usize].id;
    if card_info(id, c.cards[k as usize].upgraded).is_some_and(|info| info.custom_should_play) {
        c.unported("auto-play should_play hook");
        return;
    }
    let target_type = c.cards[k as usize].target;
    let target = match target_type {
        TargetType::Self_ | TargetType::None => Some(owner),
        TargetType::AnyEnemy => {
            let enemies = c.hittable_enemies();
            if enemies.is_empty() {
                None
            } else {
                Some(enemies[c.run.rngs.combat_targets.choice_index(enemies.len())])
            }
        }
        TargetType::AnyAlly => c.random_ally_target(owner),
        _ => None,
    };
    if matches!(target_type, TargetType::AnyEnemy | TargetType::AnyAlly) && target.is_none() {
        c.move_card_to_pile(k, crate::combat::PileKind::Discard, crate::combat::PilePos::Bottom);
        return;
    }
    c.execute_card_play(k, target, false, false);
    c.check_combat_end();
}

/// Imbued cards of `owner`'s draw pile auto-play on the first turn.
pub(crate) fn after_player_turn_start(c: &mut Combat, owner: CId) {
    if c.round != 1 {
        return;
    }
    let imbued: Vec<KRef> = c.players[owner]
        .draw
        .iter()
        .copied()
        .filter(|&k| has(&c.cards[k as usize], IMBUED))
        .collect();
    for k in imbued {
        auto_play_card(c, k);
        if c.is_over || c.pending_choice.is_some() {
            break;
        }
    }
}

pub(crate) fn before_flush(c: &mut Combat, owner: CId) {
    for i in 0..c.players[owner].hand.len() {
        let k = c.players[owner].hand[i];
        let card = &mut c.cards[k as usize];
        if has(card, SLUMBERING_ESSENCE) && !card.x_cost {
            card.add_energy_cost(-1, crate::card::CostExpiry::Played);
        }
    }
}

pub(crate) fn modify_shuffle_order(c: &mut Combat, owner: CId) {
    let cards = &c.cards;
    let draw = &mut c.players[owner].draw;
    let mut first_non_fit = 0;
    for i in 0..draw.len() {
        if has(&cards[draw[i] as usize], PERFECT_FIT) {
            draw[first_non_fit..=i].rotate_right(1);
            first_non_fit += 1;
        }
    }
}

/// Every player's Imbued cards go to the back of their opening draw pile.
pub(crate) fn move_initial_imbued_to_back(c: &mut Combat) {
    for owner in 0..c.players.len() as CId {
        let cards = &c.cards;
        let draw = &mut c.players[owner].draw;
        let len = draw.len();
        let mut i = 0;
        for _ in 0..len {
            if has(&cards[draw[i] as usize], IMBUED) {
                let k = draw.remove(i);
                draw.push(k);
            } else {
                i += 1;
            }
        }
    }
}

fn gain_block(c: &mut Combat, owner: u8, amount: i32, source: KRef) {
    let before = c.cr(owner).block;
    c.gain_recorded_block(owner,amount);
    let gained = c.cr(owner).block - before;
    if gained > 0 {
        c.record_block_gained_event(owner, Props::MOVE, Some(source));
        c.fire(
            Hook::AfterBlockGained,
            Ev::BlockGained {
                creature: owner,
                amount: gained,
            },
            None,
        );
    }
}

fn apply_weak_from_card(c: &mut Combat, k: KRef, owner: u8, target: u8) {
    let previous = c.active_card;
    c.active_card = Some(k);
    c.apply_power_to(target, PowerId::Weak, 1, Some(owner));
    c.active_card = previous;
}

pub(crate) fn on_card_played(c: &mut Combat, k: KRef) {
    let owner = c.cards[k as usize].owner;
    let adroit = amount(&c.cards[k as usize], ADROIT);
    if adroit != 0 {
        let block = c.calculate_block(adroit, owner, Props::MOVE, Some(k));
        gain_block(c, owner, block, k);
    }
    let swift = amount(&c.cards[k as usize], SWIFT);
    if swift != 0 && !c.cards[k as usize].cv.enchant_used_swift {
        c.draw_cards(owner, swift, false);
    }
    let sown = amount(&c.cards[k as usize], SOWN);
    if sown != 0
        && !c.cards[k as usize].cv.enchant_used_sown
        && c.cr(owner).is_alive()
        && !c.is_over_or_ending() // combat.gain_energy gate
    {
        c.players[owner].energy += sown;
    }
    if has(&c.cards[k as usize], CORRUPTED) {
        c.deal_damage(
            None,
            owner,
            2,
            Props::UNBLOCKABLE.or(Props::UNPOWERED).or(Props::MOVE),
        );
    }
    let momentum = amount(&c.cards[k as usize], MOMENTUM);
    if momentum != 0 {
        c.cards[k as usize].cv.enchant_momentum_bonus += momentum;
    }
    if has(&c.cards[k as usize], INKY) {
        let target_type = c.cards[k as usize].target;
        if target_type == TargetType::AllEnemies {
            for target in c.hittable_enemies() {
                if c.cr(target).is_alive() {
                    apply_weak_from_card(c, k, owner, target);
                }
            }
        } else if let Some(target) = c
            .pending_play
            .last()
            .filter(|p| p.card == k)
            .and_then(|p| p.target)
            && c.cr(target).is_alive()
        {
            apply_weak_from_card(c, k, owner, target);
        }
    }
}

pub(crate) fn after_card_played(c: &mut Combat, k: KRef) {
    let card = &mut c.cards[k as usize];
    if has(card, SWIFT) {
        card.cv.enchant_used_swift = true;
    }
    if has(card, SOWN) {
        card.cv.enchant_used_sown = true;
    }
    if has(card, VIGOROUS) {
        card.cv.enchant_used_vigorous = true;
    }
    if has(card, GLAM) {
        card.cv.enchant_used_glam = true;
    }
    if has(card, GOOPY) {
        let next = amount(card, GOOPY) + 1;
        set_amount(card, GOOPY, next);
        if let Some(deck_index) = card.deck_version {
            if let Some(deck_card) = c.run.deck.get_mut(deck_index as usize)
                && has(deck_card, GOOPY)
            {
                let deck_next = amount(deck_card, GOOPY) + 1;
                set_amount(deck_card, GOOPY, deck_next);
            }
        }
    }
}
