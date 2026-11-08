//! r7 colorless cards plus the Neow/ancient cards exercised by Ironclad runs.

use crate::card::{CId, KRef};
use crate::choice::{ChoiceSrc, PendingChoice, Resolver};
use crate::combat::{Combat, MAX_HAND_SIZE, PLAYER, PileKind, PilePos, Props};
use crate::hooks::Ev;
use crate::tables::*;
use arrayvec::ArrayVec;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{rng::Rng, run::{RunManager, RunOptions}};

    #[test]
    fn maul_buffs_every_owned_copy_across_piles_beyond_thirty_two() {
        for seats in 1..=4 {for owner in 0..seats {for upgraded in [false,true] {
            let mut run=RunManager::new(1,RunOptions::default()).run_ctx().clone();
            run.deck.clear();run.relics.clear();
            let mut c=Combat::new(run,1);
            for _ in 1..seats {c.add_player(50,50,3,0,&[]);}
            c.setup_encounter("SLUDGE_SPINNER_WEAK",&mut Rng::new(1));
            c.start_combat();
            let enemy=c.enemies[0];
            c.cr_mut(enemy).hp=10000;c.cr_mut(enemy).max_hp=10000;
            let mut mauls=Vec::new();
            for i in 0..40 {
                let k=c.new_card(owner,CardId::Maul,upgraded).unwrap();
                mauls.push(k);
                match i%4 {
                    0=>c.players[owner].hand.push(k),
                    1=>c.players[owner].draw.push(k),
                    2=>c.players[owner].discard.push(k),
                    _=>c.players[owner].exhaust.push(k),
                }
            }
            let unpiled=c.new_card(owner,CardId::Maul,upgraded).unwrap();
            let mut teammates=Vec::new();
            for seat in 0..seats {
                if seat==owner {continue;}
                let k=c.new_card(seat,CardId::Maul,false).unwrap();
                c.players[seat].draw.push(k);teammates.push(k);
            }
            let strike=c.new_card(owner,CardId::StrikeIronclad,false).unwrap();
            c.players[owner].draw.push(strike);
            c.players[owner].energy=30;
            assert!(c.play_card(owner,0,Some(0)));
            assert_eq!(c.fault,None);
            // Original Maul: base5 (+1 upgraded), Increase2 (+1 upgraded).
            let expected=if upgraded {9}else{7};
            for k in mauls {
                assert_eq!(c.cards[k as usize].damage,Some(expected));
                assert_eq!(c.cards[k as usize].var(VarKey::Damage),Some(expected));
            }
            assert_eq!(c.cards[unpiled as usize].damage,Some(if upgraded {6}else{5}));
            for k in teammates {assert_eq!(c.cards[k as usize].damage,Some(5));}
            assert_eq!(c.cards[strike as usize].damage,Some(6));
        }}}
    }
}

// Lexical CardId.name rank from the frozen r7 CardId table. Card-pile selectors sort by
// (rarity, card_id.name); this keeps that exact order without string work in a combat step.
pub(crate) const CARD_NAME_RANK: [u16; CardId::COUNT] = [
    519, 146, 32, 13, 18, 50, 49, 54, 64, 95, 264, 266, 297, 347, 388, 398, 462, 475, 533, 555,
    564, 565, 568, 21, 33, 51, 70, 74, 156, 164, 167, 174, 193, 194, 205, 209, 215, 227, 250, 273,
    281, 290, 291, 293, 303, 392, 419, 422, 446, 455, 500, 509, 512, 513, 540, 576, 578, 583, 590,
    9, 31, 62, 87, 104, 109, 124, 125, 132, 157, 203, 208, 272, 288, 302, 326, 367, 370, 376, 405,
    415, 511, 539, 541, 553, 575, 63, 118, 522, 149, 357, 530, 518, 145, 600, 175, 521, 148, 199,
    582, 520, 147, 53, 574, 596, 135, 483, 73, 584, 20, 101, 143, 169, 294, 360, 378, 382, 434,
    468, 598, 472, 289, 38, 4, 15, 25, 44, 99, 128, 129, 136, 151, 166, 220, 312, 391, 396, 403,
    439, 482, 486, 524, 577, 3, 26, 52, 61, 66, 82, 134, 191, 195, 196, 212, 216, 219, 222, 224,
    261, 265, 276, 292, 315, 333, 344, 363, 372, 389, 394, 400, 401, 402, 432, 479, 497, 516, 537,
    580, 589, 0, 2, 6, 7, 22, 45, 69, 75, 117, 178, 188, 200, 249, 306, 325, 329, 352, 359, 461,
    467, 465, 488, 515, 548, 558, 561, 529, 597, 28, 30, 35, 58, 92, 97, 102, 107, 115, 221, 248,
    257, 279, 280, 313, 318, 348, 531, 566, 579, 59, 68, 85, 90, 94, 106, 131, 168, 182, 206, 210,
    230, 233, 241, 242, 258, 298, 319, 365, 375, 433, 442, 449, 451, 466, 480, 485, 514, 523, 526,
    535, 536, 542, 544, 556, 592, 5, 12, 67, 112, 114, 122, 152, 179, 214, 239, 270, 283, 284, 285,
    322, 336, 346, 351, 420, 428, 470, 478, 498, 527, 563, 587, 41, 416, 8, 48, 150, 153, 171, 202,
    218, 252, 251, 296, 354, 397, 411, 425, 427, 450, 453, 487, 495, 594, 57, 60, 76, 81, 86, 98,
    120, 130, 139, 137, 138, 140, 154, 161, 173, 183, 207, 229, 263, 278, 314, 316, 332, 364, 377,
    384, 412, 414, 423, 440, 464, 474, 476, 481, 505, 581, 29, 83, 155, 160, 180, 181, 190, 244,
    262, 345, 353, 356, 366, 424, 426, 447, 454, 460, 469, 493, 499, 507, 549, 557, 562, 572, 225,
    409, 23, 39, 89, 100, 103, 119, 123, 126, 236, 245, 246, 255, 275, 309, 386, 390, 430, 489,
    503, 599, 11, 43, 71, 91, 93, 110, 113, 159, 232, 234, 243, 268, 304, 305, 308, 311, 321, 327,
    350, 371, 379, 383, 385, 393, 408, 417, 418, 431, 437, 445, 471, 496, 510, 525, 528, 543, 19,
    37, 42, 56, 72, 105, 121, 144, 177, 226, 238, 254, 259, 267, 269, 299, 324, 349, 358, 444, 459,
    463, 534, 551, 569, 585, 335, 550, 24, 40, 88, 116, 133, 162, 172, 189, 201, 211, 213, 217,
    235, 282, 287, 295, 301, 317, 339, 368, 380, 381, 404, 406, 407, 410, 413, 438, 458, 473, 501,
    517, 538, 546, 552, 554, 570, 571, 586, 10, 14, 34, 36, 55, 80, 187, 192, 247, 260, 277, 300,
    307, 328, 331, 338, 361, 421, 436, 443, 448, 452, 456, 457, 547, 441, 16, 17, 65, 330, 355,
    435, 591, 593, 78, 84, 96, 165, 176, 184, 186, 197, 204, 271, 323, 334, 373, 387, 429, 506,
    508, 559, 231, 240, 320, 341, 342, 343, 491, 494, 532, 163, 340, 484, 588, 141, 228, 490, 560,
    27, 127, 142, 185, 223, 253, 256, 399, 504, 77, 310, 502, 158, 237, 595, 47, 337, 362, 374,
    545, 1, 170, 46, 79, 108, 111, 198, 274, 286, 369, 395, 477, 492, 567, 573,
];


#[inline]
fn owner(c: &Combat, k: KRef) -> CId {
    c.cards[k as usize].owner
}

#[inline]
fn var(c: &Combat, k: KRef, key: VarKey, default: i32) -> i32 {
    c.cards[k as usize].var(key).unwrap_or(default)
}

#[inline]
fn attack_target(target: Option<CId>) -> CId {
    target.expect("ANY_ENEMY card without target")
}

fn gain_energy(c: &mut Combat, target: CId, amount: i32) {
    if !c.players.is_player(target) {
        c.unported("colorless ally energy");
    } else if amount > 0 && !c.is_over_or_ending() {
        c.players[target].energy += amount;
    }
}

fn gain_stars(c: &mut Combat, target: CId, amount: i32) {
    if !c.players.is_player(target) {
        c.unported("Solar Strike ally stars");
    } else {
        c.gain_stars(target, amount);
    }
}

fn calculated_block(c: &mut Combat, k: KRef, target: CId, base: i32) {
    // C# CreatureCmd.GainBlock: shared guards for dead targets/combat end.
    super::kit::block_target(c, k, target, base);
}

fn dynamic_damage(c: &mut Combat, k: KRef, target: CId, base: i32, props: Props) -> i32 {
    let who = owner(c, k);
    if c.cr(who).is_dead() {
        return 0;
    }
    let block_before = c.cr(target).block;
    let damage = c.calculate_damage(base, Some(who), target, props);
    c.apply_damage_with_card_source(target, damage, props, Some(who), Some(k));
    let blocked = (block_before - c.cr(target).block).clamp(0, damage.max(0));
    (damage - blocked).max(0)
}

fn move_to_hand(c: &mut Combat, k: KRef) {
    if c.is_over_or_ending() {
        return;
    }
    let who = owner(c, k);
    let old = c.card_pile(k);
    if let Some(i) = c.players[who].draw.iter().position(|&q| q == k) {
        c.players[who].draw.remove(i);
    } else if let Some(i) = c.players[who].discard.iter().position(|&q| q == k) {
        c.players[who].discard.remove(i);
    } else if let Some(i) = c.players[who].exhaust.iter().position(|&q| q == k) {
        c.players[who].exhaust.remove(i);
    } else if let Some(i) = c.players[who].hand.iter().position(|&q| q == k) {
        c.players[who].hand.remove(i);
    }
    if c.players[who].hand.len() < MAX_HAND_SIZE {
        c.players[who].hand.push(k);
    } else {
        c.players[who].discard.push(k);
    }
    if old != c.card_pile(k) { c.fire_after_card_changed_piles(k, old, None); }
}

fn exhaust_from_hand(c: &mut Combat, k: KRef) {
    if c.is_over_or_ending() {
        return;
    }
    let who = owner(c, k);
    if let Some(i) = c.players[who].hand.iter().position(|&q| q == k) {
        c.players[who].hand.remove(i);
        c.players[who].exhaust.push(k);
        c.fire_after_card_changed_piles(k, PileKind::Hand, None);
        c.fire_after_card_exhausted(k);
    }
}

fn increase_damage(c: &mut Combat, k: KRef, amount: i32) {
    let card = &mut c.cards[k as usize];
    card.damage = Some(card.damage.unwrap_or(0) + amount);
    if let Some((_, value)) = card.vars.iter_mut().find(|(key, _)| *key == VarKey::Damage) {
        *value += amount;
    }
}

/// `combat.lose_max_hp(owner, amount)`: the creature (r7 core/creature.py:350-357) then the run
/// player state (run/run_state.py:440-442).
fn lose_max_hp(c: &mut Combat, owner: CId, amount: i32) {
    // `combat_player_state_for(owner)` is None for every non-player creature.
    if owner != PLAYER || amount <= 0 {
        return;
    }
    let before = c.cr(owner).hp;
    let max_hp = (c.cr(owner).max_hp - amount).max(1);
    c.cr_mut(owner).max_hp = max_hp;
    c.cr_mut(owner).hp = before.min(max_hp);
    if c.cr(owner).hp != before && !c.is_over {
        c.fire(Hook::AfterCurrentHpChanged, Ev::HpChanged { creature: owner, delta: c.cr(owner).hp - before }, None);
    }
    c.run.max_hp = (c.run.max_hp - amount).max(1);
    c.run.hp = c.run.hp.min(c.run.max_hp);
}

#[inline]
fn rarity_rank(r: CardRarity) -> u8 {
    match r {
        CardRarity::Basic => 1,
        CardRarity::Common => 2,
        CardRarity::Uncommon => 3,
        CardRarity::Rare => 4,
        CardRarity::Ancient => 5,
        CardRarity::Event => 6,
        CardRarity::Status => 8,
        CardRarity::Curse => 9,
        CardRarity::Quest => 10,
    }
}

pub(super) fn sort_draw_selector(arena: &[crate::card::Card], cards: &mut [KRef]) {
    cards.sort_by(|&a, &b| {
        let ca = &arena[a as usize];
        let cb = &arena[b as usize];
        (rarity_rank(ca.rarity), CARD_NAME_RANK[ca.id.index()])
            .cmp(&(rarity_rank(cb.rarity), CARD_NAME_RANK[cb.id.index()]))
    });
}

#[inline]
fn hidden_gem_candidate(c: &Combat, k: KRef, preferred: bool) -> bool {
    let card = &c.cards[k as usize];
    if card.is_unplayable() || matches!(card.card_type, CardType::Curse | CardType::Quest) {
        return false;
    }
    !preferred
        || matches!(
            card.card_type,
            CardType::Attack | CardType::Skill | CardType::Power
        )
}

/// Open a single-card choice whose options are already in the reusable combat scratch.
fn request_scratch_choice(c: &mut Combat, source: PileKind, resolver: Resolver, allow_skip: bool) {
    if c.pending_choice.is_some() {
        c.unported("choice continuation would overwrite an unresolved choice");
        return;
    }
    if c.is_over_or_ending() {
        c.choice_options.clear();
        return;
    }
    if !allow_skip && c.choice_options.len() <= 1 {
        let picked = c.choice_options.first().copied();
        c.choice_options.clear();
        c.resolve_choice_effect(resolver, picked.as_slice());
        return;
    }
    c.choice_selected.clear();
    let orb_context = c.current_orb_context();
    c.pending_choice_orb_spans = c.suspend_orb_context(orb_context);
    c.pending_choice_orb_context = orb_context;
    c.pending_choice = Some(PendingChoice {
        player: c.cards[resolver.k as usize].owner,
        resolver,
        source,
        allow_skip,
        min: if allow_skip { 0 } else { 1 },
        max: 1,
        single: true,
    });
}

fn effective_power_type(id: PowerId, amount: i32) -> PowerType {
    let meta = power_meta(id);
    if meta.stack_type == PowerStackType::Counter && meta.allow_negative && amount < 0 {
        PowerType::Debuff
    } else if !meta.allow_negative && meta.power_type == PowerType::Debuff && amount < 0 {
        PowerType::Buff
    } else {
        meta.power_type
    }
}

fn hand_of_greed_fatal_allowed(c: &mut Combat, target: CId) -> bool {
    if c.has_power(target, PowerId::Minion) {
        return false;
    }
    if c.has_power(target, PowerId::Reattach) {
        c.unported("Hand of Greed Reattach fatal predicate");
        return false;
    }
    true
}

/// Returns `false` for cards this group does not own.
pub(crate) fn play(c: &mut Combat, k: KRef, target: Option<CId>) -> bool {
    let id = c.cards[k as usize].id;
    let who = owner(c, k);
    match id {
        // Event cards use their visual pools only; these effects are owner-scoped.
        // FeedingFrenzy.OnPlay applies its canonical Strength 5/7 through
        // FeedingFrenzyPower, which inherits TemporaryStrengthPower.
        CardId::FeedingFrenzyCard => {
            c.apply_power_to(who, PowerId::FeedingFrenzy, var(c, k, VarKey::Strength, 5), Some(who));
        }
        // Caltrops.OnPlay: canonical ThornsPower 3/5.
        CardId::Caltrops => {
            c.apply_power_to(who, PowerId::Thorns, var(c, k, VarKey::ThornsPower, 3), Some(who));
        }
        // Stack.CalculatedBlock: CalculationBase (0/3) + discard count * 1,
        // then the ordinary powered block pipeline (Dexterity/Frail, etc.).
        CardId::Stack => {
            let base = i32::from(c.cards[k as usize].upgraded) * 3 + c.players[who].discard.len() as i32;
            super::kit::block_base(c, k, base);
        }
        // Original NeowsFury.OnPlay awaits an optional discard selection after
        // damage, bounded by the remaining hand capacity. It consumes no RNG.
        CardId::NeowsFury => {
            let t = attack_target(target);
            c.deal_card_damage(k, t);
            let count = (var(c, k, VarKey::Cards, 2).max(0) as usize)
                .min(MAX_HAND_SIZE.saturating_sub(c.players[who].hand.len()));
            if count > 0 && !c.players[who].discard.is_empty() {
                let cards = c.players[who].discard.to_vec();
                let resolver = Resolver { src: ChoiceSrc::Card(id), k, target, step: 0, arg: 0 };
                c.request_multi_card_choice_for_player(who, &cards, PileKind::Discard,
                    resolver, 0, Some(count as u16), true);
            }
        }
        // r7 cards/status.py:475-485.
        CardId::RipAndTear => {
            for _ in 0..2 {
                if c.cr(who).is_dead() {
                    break;
                }
                let hittable = c.hittable_enemies();
                if hittable.is_empty() {
                    break;
                }
                let t = hittable[c.run.rngs.combat_targets.choice_index(hittable.len())];
                c.deal_card_damage(k, t);
            }
        }
        // r7 cards/status.py:530-537.
        CardId::BrightestFlame => {
            gain_energy(c, who, var(c, k, VarKey::Energy, 2));
            c.draw_cards(who, var(c, k, VarKey::Cards, 2), false);
            lose_max_hp(c, who, var(c, k, VarKey::MaxHp, 1));
        }
        // r7 cards/status.py:554-565 (MAUL is in SELF_MUTATING_DAMAGE).
        CardId::Maul => {
            let t = attack_target(target);
            for _ in 0..2 {
                c.deal_card_damage(k, t);
                if c.cr(t).is_dead() {
                    break;
                }
            }
            let increase = var(c, k, VarKey::Increase, 1);
            // Original AllCards has no copy limit; snapshot the owner's piles.
            for m in c.pile_cards(who) {
                if c.cards[m as usize].id == CardId::Maul {
                    increase_damage(c, m, increase);
                }
            }
        }
        // r7 cards/status.py:604-609.
        CardId::Relax => {
            c.gain_card_block(k);
            let cards = var(c, k, VarKey::Cards, 2);
            let energy = var(c, k, VarKey::Energy, 2);
            c.apply_power_to(who, PowerId::DrawCardsNextTurn, cards, Some(who));
            c.apply_power_to(who, PowerId::EnergyNextTurn, energy, Some(who));
        }
        // r7 cards/status.py:719-732.
        CardId::Distraction => {
            if let Some(&g) = c.generate_character_cards(who,Some(CardType::Skill), None, 1, true).first() {
                c.set_temporary_free_this_turn(g);
                c.add_generated_card(g, PileKind::Hand, PilePos::Bottom, true);
            }
        }
        // r7 cards/status.py:815-819 (`_deal_damage_all` per repeat, no owner-death check).
        CardId::Exterminate => {
            for _ in 0..var(c, k, VarKey::Repeat, 4) {
                c.deal_card_damage_all(k);
            }
        }
        // r7 cards/status.py:1030-1035.
        CardId::Squash => {
            let t = attack_target(target);
            c.deal_card_damage(k, t);
            c.apply_power_to(t, PowerId::Vulnerable, var(c, k, VarKey::Vulnerable, 2), Some(who));
        }
        // r7 cards/status.py:1112-1115.
        CardId::GiantRock => {
            let t = attack_target(target);
            c.deal_card_damage(k, t);
        }
        // r7 cards/regent.py:321-325. Ancient/shop acquisition can put it in an Ironclad run.
        CardId::SolarStrike => {
            let t = attack_target(target);
            c.deal_card_damage(k, t);
            gain_stars(c, who, var(c, k, VarKey::Stars, 1));
        }
        CardId::Automation => {
            c.apply_power_to(who, PowerId::Automation, 1, Some(who));
        }
        CardId::BelieveInYou => {
            gain_energy(c, target.unwrap_or(who), var(c, k, VarKey::Energy, 2));
        }
        CardId::Catastrophe => {
            // Catastrophe.OnPlay awaits each autoplay before sampling again.
            let count = var(c, k, VarKey::Cards, 2).max(0) as usize;
            c.autoplay_random_draw(who, count);
        }
        CardId::CoordinateCard => {
            // C# Coordinate.OnPlay; CoordinatePower inherits TemporaryStrengthPower.
            let Some(t) = target else {
                c.unported("Coordinate requires an ally target");
                return true;
            };
            c.apply_power_to(
                t,
                PowerId::Coordinate,
                var(c, k, VarKey::Strength, 5),
                Some(who),
            );
        }
        CardId::DarkShackles => {
            c.apply_power_to(
                attack_target(target),
                PowerId::DarkShackles,
                var(c, k, VarKey::StrengthLoss, 9),
                Some(who),
            );
        }
        // C# Abundance.OnPlay upgrades all generated options before selection.
        // Only an explicitly all-unlocked run has an audited complete pool.
        CardId::Abundance => {
            let generated = c.generate_unlocked_owner_cards(who, Some(CardType::Power), 3);
            for &q in &generated { c.upgrade_card(q); }
            if !generated.is_empty() {
                c.request_card_choice(&generated, PileKind::None,
                    Resolver { src: ChoiceSrc::Card(id), k, target, step: 0, arg: 0 }, false);
            }
        }
        CardId::Discovery => {
            // Models/Cards/Discovery.cs:22 OnPlay: Owner.Character.CardPool,
            // unlocked and player-count filtered, CombatCardGeneration RNG.
            let generated = c.generate_unlocked_owner_cards(who, None, 3);
            if !generated.is_empty() {
                let r = Resolver {
                    src: ChoiceSrc::Card(CardId::Discovery),
                    k,
                    target,
                    step: 0,
                    arg: 0,
                };
                c.request_card_choice(&generated, PileKind::None, r, true);
            }
        }
        // C# Metamorphosis.OnPlay: replacement sampling, combat-free, random draw inserts.
        CardId::Metamorphosis => {
            let cards = c.generate_owner_cards(who, CardType::Attack, var(c, k, VarKey::Cards, 3).max(0) as usize, false);
            for q in cards {
                c.cards[q as usize].set_energy_cost(0, crate::card::CostExpiry::Combat, false);
                c.cards[q as usize].cv.combat_star_cost_override = Some(0);
                c.add_generated_card(q, PileKind::Draw, PilePos::Random, true);
            }
        }
        CardId::DramaticEntrance => {
            c.deal_card_damage_all(k);
        }
        CardId::Equilibrium => {
            c.gain_card_block(k);
            c.apply_power_to(
                who,
                PowerId::RetainHand,
                var(c, k, VarKey::Equilibrium, 1),
                Some(who),
            );
        }
        CardId::Fasten => {
            c.apply_power_to(
                who,
                PowerId::Fasten,
                var(c, k, VarKey::ExtraBlock, 4),
                Some(who),
            );
        }
        CardId::Finesse => {
            c.gain_card_block(k);
            c.draw_cards_at_end_of_play(who, var(c, k, VarKey::Cards, 1));
        }
        CardId::Fisticuffs => {
            let t = attack_target(target);
            // r7 colorless.py fisticuffs: block from result.total_damage + overkill_damage.
            let base = c.cards[k as usize].damage.unwrap_or(0);
            c.last_damage = Default::default();
            dynamic_damage(c, k, t, base, Props::MOVE);
            if !c.is_over {
                let r = c.last_damage;
                calculated_block(c, k, who, r.total() + r.overkill);
            }
        }
        CardId::FlashOfSteel => {
            c.deal_card_damage(k, attack_target(target));
            c.draw_cards(who, var(c, k, VarKey::Cards, 1), false);
        }
        CardId::GangUp => {
            // C# GangUp.CanonicalVars counts this turn's powered hits from other
            // same-side dealers, including fully blocked damage entries.
            let t = attack_target(target);
            let hits = c.hist.damage[c.hist.damage_turn_start..].iter().filter(|event| {
                event.target == t && event.props.is_powered_attack()
                    && event.dealer.is_some_and(|dealer| {
                        dealer != who && c.cr(dealer).side == c.cr(who).side
                    })
            }).count() as i32;
            let base = var(
                c,
                k,
                VarKey::CalcBase,
                c.cards[k as usize].damage.unwrap_or(5),
            );
            let base = base + var(c, k, VarKey::ExtraDamage, 5) * hits;
            dynamic_damage(c, k, t, base, Props::MOVE);
        }
        CardId::HuddleUp => {
            c.draw_team_without_blocking(var(c, k, VarKey::Cards, 2));
        }
        CardId::Impatience => {
            if !c.players[who]
                .hand
                .iter()
                .any(|&h| c.cards[h as usize].card_type == CardType::Attack)
            {
                c.draw_cards_at_end_of_play(who, var(c, k, VarKey::Cards, 2));
            }
        }
        CardId::InterceptCard => {
            // C# Intercept.OnPlay: block belongs to the caster, Covered to the target.
            let Some(t) = target else {
                c.unported("Intercept requires an ally target");
                return true;
            };
            let base = c.cards[k as usize].block.unwrap_or(0);
            calculated_block(c, k, who, base);
            c.apply_power_to(t, PowerId::Covered, 1, Some(who));
        }
        CardId::JackOfAllTrades => {
            // C# JackOfAllTrades.OnPlay / CardFactory.GetDistinctForCombat:
            // use the owner's player-count pool, excluding Jack itself.
            let Some(pool) = crate::cardgen::character_pool(crate::character::CardPool::Colorless, c.players.len() > 1, None, None) else {
                c.unported("colorless card pool");
                return true;
            };
            let ids: Vec<CardId> = pool
                .iter()
                .copied()
                .filter(|&id| id != CardId::JackOfAllTrades)
                .collect();
            let count = var(c, k, VarKey::Cards, 1).max(0) as usize;
            for generated in c.generate_pool_cards(who,&ids, count, true) {
                c.add_generated_card(generated, PileKind::Hand, PilePos::Bottom, true);
            }
        }
        CardId::Lift => {
            if let Some(t) = target {
                let base = c.cards[k as usize].block.unwrap_or(0);
                calculated_block(c, k, t, base);
            }
        }
        CardId::MindBlast => {
            let t = attack_target(target);
            let base = var(c, k, VarKey::CalcBase, 0)
                + var(c, k, VarKey::ExtraDamage, 1) * c.players[who].draw.len() as i32;
            dynamic_damage(c, k, t, base, Props::MOVE);
        }
        CardId::Omnislice => {
            let t = attack_target(target);
            let base = c.cards[k as usize]
                .damage
                .unwrap_or_else(|| var(c, k, VarKey::Damage, 8));
            // C# Omnislice.OnPlay: splash = TotalDamage (blocked + unblocked) + OverkillDamage.
            c.last_damage = Default::default();
            dynamic_damage(c, k, t, base, Props::MOVE);
            let r = c.last_damage;
            let splash = r.total() + r.overkill;
            if splash > 0 {
                let teammates: ArrayVec<CId, 8> = c
                    .enemies
                    .iter()
                    .copied()
                    .filter(|&e| e != t && c.cr(e).is_alive())
                    .collect();
                let props = Props::MOVE.or(Props::UNPOWERED);
                for enemy in teammates {
                    let damage = c.calculate_damage(splash, Some(who), enemy, props);
                    c.apply_damage_with_card_source(enemy, damage, props, Some(who), Some(k));
                }
            }
        }
        CardId::PanacheCard => {
            c.apply_power_to(
                who,
                PowerId::Panache,
                var(c, k, VarKey::PanacheDamage, 10),
                Some(who),
            );
        }
        CardId::PanicButton => {
            c.gain_card_block(k);
            c.apply_power_to(
                who,
                PowerId::NoBlock,
                var(c, k, VarKey::Turns, 2),
                Some(who),
            );
        }
        CardId::PrepTime => {
            c.apply_power_to(
                who,
                PowerId::PrepTime,
                var(c, k, VarKey::PrepTime, 4),
                Some(who),
            );
        }
        CardId::Production => gain_energy(c, who, var(c, k, VarKey::Energy, 2)),
        CardId::Prolong => {
            let block = c.cr(who).block;
            if block > 0 {
                c.apply_power_to(who, PowerId::BlockNextTurn, block, Some(who));
            }
        }
        CardId::Prowess => {
            c.apply_power_to(
                who,
                PowerId::Strength,
                var(c, k, VarKey::Strength, 1),
                Some(who),
            );
            c.apply_power_to(
                who,
                PowerId::Dexterity,
                var(c, k, VarKey::Dexterity, 1),
                Some(who),
            );
        }
        CardId::Purity => {
            if !c.players[who].hand.is_empty() {
                let candidates: ArrayVec<KRef, MAX_HAND_SIZE> = c.players[who].hand.iter().copied().collect();
                let max = var(c, k, VarKey::Cards, 3).max(0) as u16;
                let r = Resolver {
                    src: ChoiceSrc::Card(CardId::Purity),
                    k,
                    target,
                    step: 0,
                    arg: 0,
                };
                c.request_multi_card_choice(&candidates, PileKind::Hand, r, 0, Some(max), true);
            }
        }
        CardId::Restlessness => {
            if c.players[who].hand.is_empty() {
                c.draw_cards(who, var(c, k, VarKey::Cards, 2), false);
                gain_energy(c, who, var(c, k, VarKey::Energy, 2));
            }
        }
        CardId::SeekerStrike => {
            let t = attack_target(target);
            c.deal_card_damage(k, t);
            if !c.players[who].draw.is_empty() {
                let count = var(c, k, VarKey::Cards, 3).max(0) as usize;
                c.choice_options.clear();
                c.choice_options.extend(c.players[who].draw.iter().copied());
                // fire: core/combat.py:3020-3022
                // stable_shuffle_cards canonicalizes by (CardId.name, upgraded) before
                // consuming the combat-card-selection RNG.
                c.choice_options.sort_by_key(|&q| {
                    let card = &c.cards[q as usize];
                    (CARD_NAME_RANK[card.id.index()], card.upgraded)
                });
                c.run
                    .rngs
                    .combat_card_selection
                    .shuffle(&mut c.choice_options);
                c.choice_selected.clear();
                for i in 0..count.min(c.choice_options.len()) {
                    c.choice_selected.push(c.choice_options[i]);
                }
                c.choice_options.clear();
                for &q in &c.players[who].draw{
                    if c.choice_selected.contains(&q) {
                        c.choice_options.push(q);
                    }
                }
                c.choice_selected.clear();
                if c.choice_options.len() > 1 {
                    sort_draw_selector(&c.cards, &mut c.choice_options);
                }
                let r = Resolver {
                    src: ChoiceSrc::Card(CardId::SeekerStrike),
                    k,
                    target,
                    step: 0,
                    arg: 0,
                };
                request_scratch_choice(c, PileKind::Draw, r, false);
            }
        }
        CardId::Shockwave => {
            let amount = var(c, k, VarKey::Power, 3);
            for enemy in c.hittable_enemies() {
                c.apply_power_to(enemy, PowerId::Weak, amount, Some(who));
                c.apply_power_to(enemy, PowerId::Vulnerable, amount, Some(who));
            }
        }
        CardId::Splash => {
            // Splash.OnPlay removes the owner's pool, not always Ironclad's,
            // from UnlockState.CharacterCardPools, preserving canonical order.
            let Some(owner_character) = c.players[who].character else {
                c.unported("Splash owner character");
                return true;
            };
            let mut ids = Vec::new();
            use crate::character::Character;
            for character in [Character::Ironclad, Character::Silent, Character::Regent, Character::Necrobinder, Character::Defect] {
                if character == owner_character { continue; }
                let Some(pool) = crate::cardgen::character_pool(character.into(), false, Some(CardType::Attack), None) else {
                    c.unported("Splash character card pool");
                    return true;
                };
                ids.extend_from_slice(pool);
            }
            let generated = c.generate_pool_cards(who,&ids, 3, true);
            if c.cards[k as usize].upgraded {
                for &g in &generated {
                    c.upgrade_card(g);
                }
            }
            if !generated.is_empty() {
                let r = Resolver {
                    src: ChoiceSrc::Card(CardId::Splash),
                    k,
                    target,
                    step: 0,
                    arg: 0,
                };
                c.request_card_choice(&generated, PileKind::None, r, true);
            }
        }
        CardId::Stratagem => {
            c.apply_power_to(who, PowerId::Stratagem, 1, Some(who));
        }
        CardId::TagTeam => {
            let t = attack_target(target);
            c.deal_card_damage(k, t);
            c.apply_power_to(t, PowerId::TagTeam, 1, Some(who));
        }
        CardId::Midnight => {
            // C# Midnight.OnPlay.
            c.deal_card_damage(k, attack_target(target));
        }
        CardId::TheBall => {
            c.deal_card_damage(k, attack_target(target));
            increase_damage(c, k, var(c, k, VarKey::Increase, 10));
        }
        CardId::TheBombCard => {
            // fire: cards/colorless.py:622-629
            if let Some(p) = c.apply_power_to(
                who,
                PowerId::TheBomb,
                var(c, k, VarKey::Turns, 3),
                Some(who),
            ) {
                c.powers[p as usize].payload = var(c, k, VarKey::BombDamage, 40);
            }
        }
        CardId::ThinkingAhead => {
            c.draw_cards(who, var(c, k, VarKey::Cards, 2), false);
            if !c.players[who].hand.is_empty() {
                let candidates: ArrayVec<KRef, MAX_HAND_SIZE> = c.players[who].hand.iter().copied().collect();
                let r = Resolver {
                    src: ChoiceSrc::Card(CardId::ThinkingAhead),
                    k,
                    target,
                    step: 0,
                    arg: 0,
                };
                c.request_card_choice(&candidates, PileKind::Hand, r, false);
            }
        }
        CardId::ThrummingHatchet => {
            c.deal_card_damage(k, attack_target(target));
            c.cards[k as usize].cv.return_before_hand_draw_round = Some(c.round + 1);
        }
        CardId::UltimateDefend => c.gain_card_block(k),
        CardId::UltimateStrike => c.deal_card_damage(k, attack_target(target)),
        CardId::Volley => {
            let hits = c.cards[k as usize].energy_spent.max(0);
            for _ in 0..hits {
                if c.cr(who).is_dead() {
                    break;
                }
                let enemies = c.hittable_enemies();
                if enemies.is_empty() {
                    break;
                }
                let i = c.run.rngs.combat_targets.choice_index(enemies.len());
                c.deal_card_damage(k, enemies[i]);
            }
        }
        CardId::Alchemize => super::potions::alchemize(c, who),
        CardId::Anointed => {
            let mut i = 0;
            while i < c.players[who].draw.len() {
                let q = c.players[who].draw[i];
                if c.cards[q as usize].rarity == CardRarity::Rare {
                    move_to_hand(c, q);
                } else {
                    i += 1;
                }
            }
        }
        CardId::BeaconOfHope => {
            c.apply_power_to(who, PowerId::BeaconOfHope, 1, Some(who));
        }
        CardId::BeatDown => {
            // C# BeatDown.OnPlay: StableShuffle once, then await every selected card.
            let count = var(c, k, VarKey::Cards, 3).max(0) as usize;
            let mut candidates: Vec<KRef> = c.players[who]
                .discard
                .iter()
                .copied()
                .filter(|&q| {
                    let card = &c.cards[q as usize];
                    card.card_type == CardType::Attack && !card.is_unplayable()
                })
                .collect();
            // combat.stable_shuffle_cards: stable sort by (card_id.name, upgraded), then shuffle.
            candidates.sort_by_key(|&q| {
                let card = &c.cards[q as usize];
                (CARD_NAME_RANK[card.id.index()], card.upgraded)
            });
            c.run.rngs.shuffle.shuffle(&mut candidates);
            c.autoplay_card_snapshot(candidates.into_iter().take(count).collect(), true);
        }
        CardId::Bolas => {
            c.deal_card_damage(k, attack_target(target));
            c.cards[k as usize].cv.return_before_hand_draw_round = Some(c.round + 1);
        }
        CardId::CalamityCard => {
            c.apply_power_to(who, PowerId::Calamity, 1, Some(who));
        }
        CardId::Entropy => {
            c.apply_power_to(who, PowerId::Entropy, 1, Some(who));
        }
        CardId::EternalArmor => {
            c.apply_power_to(
                who,
                PowerId::Plating,
                var(c, k, VarKey::Plating, 9),
                Some(who),
            );
        }
        CardId::GoldAxe => {
            let t = attack_target(target);
            // GoldAxe.cs:21 counts the entire combat's finished card plays,
            // including other players, rather than only this card's owner.
            let count = c.hist.played_combat.len() as i32;
            let base = var(c, k, VarKey::CalcBase, 0) + var(c, k, VarKey::ExtraDamage, 1) * count;
            dynamic_damage(c, k, t, base, Props::MOVE);
        }
        CardId::HandOfGreed => {
            let t = attack_target(target);
            let fatal = hand_of_greed_fatal_allowed(c, t);
            c.deal_card_damage(k, t);
            if fatal && c.cr(t).is_dead() {
                c.change_player_gold(who, var(c, k, VarKey::Gold, 20));
            }
        }
        CardId::HiddenGem => {
            let preferred_count = c.players[who]
                .draw
                .iter()
                .filter(|&&q| hidden_gem_candidate(c, q, true))
                .count();
            let (preferred, candidate_count) = if preferred_count > 0 {
                (true, preferred_count)
            } else {
                (
                    false,
                    c.players[who].draw
                        .iter()
                        .filter(|&&q| hidden_gem_candidate(c, q, false))
                        .count(),
                )
            };
            if candidate_count > 0 {
                let index = c
                    .run
                    .rngs
                    .combat_card_selection
                    .choice_index(candidate_count);
                let selected = c.players[who]
                    .draw
                    .iter()
                    .copied()
                    .filter(|&q| hidden_gem_candidate(c, q, preferred))
                    .nth(index)
                    .expect("Hidden Gem candidate count");
                c.cards[selected as usize].replay += var(c, k, VarKey::Replay, 2);
            }
        }
        // C# Clash / r7 cards/status.py clash_effect (playability: every hand card is an attack).
        CardId::Clash => {
            c.deal_card_damage(k, attack_target(target));
        }
        // C# MadScience.OnPlay / ExecuteAttack / ExecuteSkill / ExecutePower / ExecuteRider.
        // The snapshot loader restores TinkerTimeType and the saved rider var.
        CardId::MadScience => {
            let rider = var(c, k, VarKey::Rider, 0);
            match c.cards[k as usize].card_type {
                CardType::Attack => {
                    let t = attack_target(target);
                    let hits = if rider == 2 { var(c, k, VarKey::ViolenceHits, 1) } else { 1 };
                    for _ in 0..hits {
                        c.deal_card_damage(k, t);
                    }
                    if rider == 1 {
                        let weak = var(c, k, VarKey::SappingWeak, 2);
                        let vuln = var(c, k, VarKey::SappingVulnerable, 2);
                        c.apply_power_to(t, PowerId::Weak, weak, Some(who));
                        c.apply_power_to(t, PowerId::Vulnerable, vuln, Some(who));
                    } else if rider == 3 {
                        // C# MadScience.ExecuteRider(Choking), after the attack.
                        let n = var(c, k, VarKey::ChokingDamage, 6);
                        c.apply_power_to(t, PowerId::Strangle, n, Some(who));
                    }
                }
                CardType::Skill => {
                    c.gain_card_block(k);
                    if rider == 4 {
                        let n = var(c, k, VarKey::EnergizedEnergy, 2);
                        if n > 0 && !c.is_over_or_ending() {
                            c.players[who].energy+= n;
                        }
                    } else if rider == 5 {
                        let n = var(c, k, VarKey::WisdomCards, 3);
                        c.draw_cards(who, n, false);
                    } else if rider == 6 {
                        for q in c.generate_unlocked_owner_cards(who, None, 1) {
                            c.set_temporary_free_this_turn(q);
                            c.add_generated_card(q, PileKind::Hand, PilePos::Bottom, true);
                        }
                    }
                }
                _ => match rider {
                    7 => {
                        let s = var(c, k, VarKey::ExpertiseStrength, 2);
                        let d = var(c, k, VarKey::ExpertiseDexterity, 2);
                        c.apply_power_to(who, PowerId::Strength, s, Some(who));
                        c.apply_power_to(who, PowerId::Dexterity, d, Some(who));
                    }
                    8 => {
                        let n = var(c, k, VarKey::CuriousReduction, 1);
                        c.apply_power_to(who, PowerId::Curious, n, Some(who));
                    }
                    9 => {
                        c.apply_power_to(who, PowerId::Improvement, 1, Some(who));
                    }
                    _ => {}
                },
            }
        }
        CardId::Jackpot => {
            let t = attack_target(target);
            c.deal_card_damage(k, t);
            let count = var(c, k, VarKey::Cards, 3).max(0) as usize;
            // Jackpot.OnPlay uses the owner's unlocked canonical-zero-cost pool,
            // sampling with replacement and allocating every card before upgrades.
            let generated=c.generate_unlocked_zero_cost_owner_cards(who,count);
            for g in generated {
                if c.cards[k as usize].upgraded {
                    c.upgrade_card(g);
                }
                c.add_generated_card(g, PileKind::Hand, PilePos::Bottom, true);
            }
        }
        CardId::Knockdown => {
            let t = attack_target(target);
            c.deal_card_damage(k, t);
            c.apply_power_to(
                t,
                PowerId::Knockdown,
                var(c, k, VarKey::Knockdown, 2),
                Some(who),
            );
        }
        CardId::MasterOfStrategy => {
            c.draw_cards_at_end_of_play(who, var(c, k, VarKey::Cards, 3));
        }
        CardId::MayhemCard => {
            c.apply_power_to(who, PowerId::Mayhem, 1, Some(who));
        }
        CardId::Mimic => {
            if let Some(t) = target {
                let base = var(c, k, VarKey::CalcBase, 0)
                    + var(c, k, VarKey::CalcExtra, 1) * c.cr(t).block;
                calculated_block(c, k, who, base);
            }
        }
        CardId::NostalgiaCard => {
            c.apply_power_to(who, PowerId::Nostalgia, 1, Some(who));
        }
        CardId::Rally => {
            // C# Rally.OnPlay: every living player teammate (including the owner).
            let base = c.cards[k as usize].block.unwrap_or(0);
            for t in 0..c.players.len() as CId {
                if c.cr(t).is_alive() {
                    calculated_block(c, k, t, base);
                }
            }
        }
        CardId::Rend => {
            let t = attack_target(target);
            let debuffs = c
                .cr(t)
                .powers
                .iter()
                .filter(|&&p| {
                    let power = &c.powers[p as usize];
                    let meta = power_meta(power.id);
                    !meta.temporary
                        && effective_power_type(power.id, power.amount) == PowerType::Debuff
                })
                .count() as i32;
            let base = var(
                c,
                k,
                VarKey::CalcBase,
                c.cards[k as usize].damage.unwrap_or(10),
            ) + var(c, k, VarKey::ExtraDamage, 5) * debuffs;
            dynamic_damage(c, k, t, base, Props::MOVE);
        }
        CardId::RollingBoulder => {
            c.apply_power_to(
                who,
                PowerId::RollingBoulder,
                var(c, k, VarKey::RollingBoulder, 5),
                Some(who),
            );
        }
        CardId::Salvo => {
            c.deal_card_damage(k, attack_target(target));
            c.apply_power_to(who, PowerId::RetainHand, 1, Some(who));
        }
        CardId::Scrawl => {
            let to_draw = MAX_HAND_SIZE.saturating_sub(c.players[who].hand.len()) as i32;
            if to_draw > 0 {
                c.draw_cards_at_end_of_play(who, to_draw);
            }
        }
        // C#: Wish.OnPlay selects any draw-pile card and moves it to hand.
        CardId::Wish | CardId::SecretTechnique | CardId::SecretWeapon => {
            let wanted = if id == CardId::SecretTechnique {
                CardType::Skill
            } else {
                CardType::Attack
            };
            c.choice_options.clear();
            for &q in &c.players[who].draw{
                if id == CardId::Wish || c.cards[q as usize].card_type == wanted {
                    c.choice_options.push(q);
                }
            }
            if !c.choice_options.is_empty() {
                sort_draw_selector(&c.cards, &mut c.choice_options);
                let r = Resolver {
                    src: ChoiceSrc::Card(id),
                    k,
                    target,
                    step: 0,
                    arg: 0,
                };
                request_scratch_choice(c, PileKind::Draw, r, false);
            }
        }
        // C# Whistle.OnPlay: hit then stun, even if the damage leaves no living target.
        CardId::Whistle => {
            let t = attack_target(target);
            c.deal_card_damage(k, t);
            c.stun_enemy(t);
        }
        // C#: ByrdSwoop.OnPlay; the pet only supplies the attack animation.
        CardId::ByrdSwoop => {
            c.deal_card_damage(k, attack_target(target));
        }
        // C# Enlightenment.OnPlay: local, reduce-only, energy cost (never star or X spend).
        CardId::Enlightenment => {
            let expiry = if c.cards[k as usize].upgraded {
                crate::card::CostExpiry::Combat
            } else {
                crate::card::CostExpiry::TurnOrPlayed
            };
            for i in 0..c.players[who].hand.len() {
                let q = c.players[who].hand[i];
                c.cards[q as usize].set_energy_cost(1, expiry, true);
            }
        }
        // C# Apparition.OnPlay.
        CardId::Apparition => {
            c.apply_power_to(who, PowerId::Intangible,
                var(c, k, VarKey::IntangiblePower, 1), Some(who));
        }
        // C# Apotheosis.OnPlay / PlayerCombatState.AllCards: only actual owned piles,
        // not every card in the arena. upgrade_card leaves non-upgradable models alone;
        // its reference templates preserve the existing fixture identity bookkeeping.
        CardId::Apotheosis => {
            for q in c.pile_cards(who) {
                if q != k {
                    c.upgrade_card(q);
                }
            }
        }
        CardId::TheGambit => {
            c.gain_card_block(k);
            c.apply_power_to(who, PowerId::TheGambit, 1, Some(who));
        }
        _ => return false,
    }
    true
}

/// Choice continuations for this group's cards; `false` = not ours.
pub(crate) fn resolve(c: &mut Combat, r: Resolver, selected: &[KRef]) -> bool {
    match r.src {
        ChoiceSrc::Card(CardId::Stratagem) => {
            for &q in selected { move_to_hand(c, q); }
        }
        ChoiceSrc::Card(CardId::NeowsFury) => {
            // NeowsFury.cs:39 adds FromCombatPile's returned sequence. The
            // generic r7 discard selector sorts slots; this card preserves the
            // player's selection order, including a reversed pair of cards.
            let ordered: Vec<_> = c.choice_selected.iter()
                .map(|&i| c.choice_options[i as usize]).collect();
            for q in ordered { move_to_hand(c, q); }
        }
        ChoiceSrc::Card(CardId::Entropy) => {
            // C# EntropyPower transforms each selected card sequentially, not as a batch.
            for &picked in selected {
                entropy_transform(c, picked);
                if c.fault.is_some() { break; }
            }
        }
        ChoiceSrc::Card(CardId::Discovery | CardId::Abundance) => {
            if let Some(&picked) = selected.first() {
                c.set_temporary_free_this_turn(picked);
                c.add_generated_card(picked, PileKind::Hand, PilePos::Bottom, true);
            }
        }
        ChoiceSrc::Card(CardId::Purity) => {
            for &picked in selected {
                exhaust_from_hand(c, picked);
            }
        }
        ChoiceSrc::Card(CardId::SeekerStrike)
        | ChoiceSrc::Card(CardId::Wish)
        | ChoiceSrc::Card(CardId::SecretTechnique)
        | ChoiceSrc::Card(CardId::SecretWeapon) => {
            if let Some(&picked) = selected.first() {
                move_to_hand(c, picked);
            }
        }
        ChoiceSrc::Card(CardId::ThinkingAhead) => {
            if let Some(&picked) = selected.first() {
                let who = owner(c, picked);
                if let Some(i) = c.players[who].hand.iter().position(|&q| q == picked) {
                    c.players[who].hand.remove(i);
                    c.insert_pile(picked, PileKind::Draw, PilePos::Top);
                    c.fire_after_card_changed_piles(picked, PileKind::Hand, None);
                }
            }
        }
        ChoiceSrc::Card(CardId::Splash) => {
            if let Some(&picked) = selected.first() {
                // fire: cards/colorless.py:592-603
                c.set_temporary_free_this_turn(picked);
                c.add_generated_card(picked, PileKind::Hand, PilePos::Bottom, true);
            }
        }
        _ => return false,
    }
    true
}

fn csharp_card_name(id: CardId) -> &'static str {
    card_info(id, false).map_or(id.name(), |info| info.entry)
}

/// C# CardFactory.GetDefaultTransformationOptions and CardCmd.Transform:
/// candidates exported from the pinned original DLL, preserving pool/epoch/constraint order.
fn entropy_transform(c: &mut Combat, old: KRef) {
    if c.is_over_or_ending() { return; }
    if c.run.all_cards_unlocked != Some(true) {
        c.unported("Entropy requires explicit all-unlocked run configuration");
        return;
    }
    let original = c.cards[old as usize].id;
    let Some((_, _, options)) = CSHARP_TRANSFORMS.iter().find(|(mp, name, _)|
        *mp == (c.players.len() > 1) && *name == csharp_card_name(original)) else {
        c.unported("Entropy original card has no audited transform pool");
        return;
    };
    let mut ids = Vec::with_capacity(options.len());
    for &name in *options {
        let Some(id) = CardId::ALL.into_iter().find(|&id| csharp_card_name(id) == name) else {
            c.unported("Entropy transform candidate has no simulator identity");
            return;
        };
        ids.push(id);
    }
    if ids.is_empty() {
        c.unported("Entropy original DLL rejected empty transform pool");
        return;
    }
    let who = c.cards[old as usize].owner;
    if !c.players[who].hand.contains(&old) {
        c.unported("Entropy selected card is not in owner's hand");
        return;
    }
    let id = ids[c.run.rngs.combat_card_selection.choice_index(ids.len())];
    // CardCmd.Transform's lifecycle is shared with the other transformations:
    // preserve Entropy's per-selected-card RNG/order, then run the complete
    // changed-piles, transformed and generated callbacks for that one card.
    super::kit::transform_in_pile(c, old, id, false);
}
