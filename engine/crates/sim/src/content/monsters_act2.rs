//! r7 monsters/act2.py + encounters/act2.py (Hive).
use crate::identity::EnchantmentId;

use super::{DEADLY_ASC, TOUGH_ASC, asc_value, state_index};
use crate::card::{CId, KRef};
use crate::combat::{Combat, PileKind, PilePos};
use crate::creature::Side;
use crate::monster::MonsterAi;
use crate::rng::Rng;
use crate::tables::*;

// Factory arguments encoded in MonsterAi::flags.
const EXO_SLOT_MASK: u32 = 0b11;
const EXO_FIRST: u32 = 0;
const EXO_SECOND: u32 = 1;
const EXO_THIRD: u32 = 2;
const EXO_FOURTH: u32 = 3;
const CHOMPER_SCREAM_FIRST: u32 = 1;
const WRIGGLER_SLOT_MASK: u32 = 0b11;
const WRIGGLER_START_STUNNED: u32 = 1 << 2;
const MYTE_SECOND: u32 = 1;
const DECIMILLIPEDE_START_MASK: u32 = 0b11;

// Runtime-only flag state. ImbalancedPower's AfterDamageGiven hook is not wired by the
// current engine packet; the starting unported Imbalanced power therefore fail-closes first.
const ROCK_WAS_FULLY_BLOCKED: u32 = 1 << 31;
const KNOWLEDGE_CURSE_COUNTER_MASK: u32 = 0b11;

#[cfg(test)]
const ACT2_KINDS: &[MonsterKind] = &[
    MonsterKind::BowlbugEgg,
    MonsterKind::BowlbugNectar,
    MonsterKind::BowlbugRock,
    MonsterKind::BowlbugSilk,
    MonsterKind::Chomper,
    MonsterKind::Crusher,
    MonsterKind::DecimillipedeSegment,
    MonsterKind::DecimillipedeSegmentBack,
    MonsterKind::DecimillipedeSegmentFront,
    MonsterKind::DecimillipedeSegmentMiddle,
    MonsterKind::Entomancer,
    MonsterKind::Exoskeleton,
    MonsterKind::HunterKiller,
    MonsterKind::InfestedPrism,
    MonsterKind::KnowledgeDemon,
    MonsterKind::LouseProgenitor,
    MonsterKind::Myte,
    MonsterKind::Ovicopter,
    MonsterKind::Rocket,
    MonsterKind::SlumberingBeetle,
    MonsterKind::SpinyToad,
    MonsterKind::TheInsatiable,
    MonsterKind::TheObscura,
    MonsterKind::ThievingHopper,
    MonsterKind::ToughEgg,
    MonsterKind::Tunneler,
    MonsterKind::Wriggler,
];


/// `CardPileCmd.AddToCombatAndPreview<id>(targets, pile, count)` for every living player.
fn status(c: &mut Combat, id: CardId, count: i32, pile: PileKind, pos: PilePos) {
    for p in c.living_players() {
        for _ in 0..count {
            c.create_card_in_pile(p, id, false, pile, pos);
        }
    }
}

#[inline]
fn add(c: &mut Combat, kind: MonsterKind, flags: u32) -> CId {
    let (cr, ai) = c.create_monster(kind, flags);
    c.add_enemy(cr, ai)
}

fn add_in_slot(c: &mut Combat, kind: MonsterKind, flags: u32, slot: &'static str) -> CId {
    let (cr, ai) = c.create_monster(kind, flags);
    c.add_enemy_in_slot(cr, ai, Some(slot))
}

pub(crate) fn spawn_infested_wrigglers(c: &mut Combat) {
    // core/combat.py spawn_infested_wrigglers: four Wrigglers, fixed slot semantics, each
    // entering through SPAWNED_MOVE so its first enemy turn is the stunned/no-op move.
    for (index, slot) in ["wriggler1", "wriggler2", "wriggler3", "wriggler4"].into_iter().enumerate() {
        // r7 constructs each Wriggler with Rng(combat.rng.next_int(0, INT_MAX)).
        // MonsterAi uses the shared monster_ai stream in Rust, but the combat RNG draw is
        // still observable in parity and must be consumed.
        let _ = c.rng.next_int(0, crate::rng::INT_MAX);
        let flags = index as u32 | WRIGGLER_START_STUNNED;
        add_in_slot(c, MonsterKind::Wriggler, flags, slot);
    }
}

fn attack_spec(kind: MonsterKind, state: &str, band: usize) -> Option<(i32, i32)> {
    let hi = band != 0;
    let v = |base, high| if hi { high } else { base };
    match (kind, state) {
        (MonsterKind::FakeMerchantMonster, "SWIPE_MOVE") => Some((v(13, 15), 1)),
        (MonsterKind::FakeMerchantMonster, "SPEW_COINS_MOVE") => Some((2, 8)),
        (MonsterKind::FakeMerchantMonster, "THROW_RELIC_MOVE") => Some((v(9, 10), 1)),
        (MonsterKind::BowlbugEgg, "BITE_MOVE") => Some((v(7, 8), 1)),
        (MonsterKind::BowlbugNectar, "THRASH_MOVE" | "THRASH2_MOVE") => Some((3, 1)),
        (MonsterKind::BowlbugRock, "HEADBUTT_MOVE") => Some((v(15, 16), 1)),
        (MonsterKind::BowlbugSilk, "THRASH_MOVE") => Some((v(4, 5), 2)),
        (MonsterKind::Chomper, "CLAMP_MOVE") => Some((v(8, 9), 2)),
        (MonsterKind::Crusher, "THRASH_MOVE") => Some((v(12, 14), 1)),
        (MonsterKind::Crusher, "ENLARGING_STRIKE_MOVE") => Some((4, 1)),
        (MonsterKind::Crusher, "BUG_STING_MOVE") => Some((v(6, 7), 2)),
        (MonsterKind::Crusher, "GUARDED_STRIKE_MOVE") => Some((v(12, 14), 1)),
        (
            MonsterKind::DecimillipedeSegment
            | MonsterKind::DecimillipedeSegmentFront
            | MonsterKind::DecimillipedeSegmentMiddle
            | MonsterKind::DecimillipedeSegmentBack,
            "WRITHE_MOVE",
        ) => Some((v(5, 6), 2)),
        (
            MonsterKind::DecimillipedeSegment
            | MonsterKind::DecimillipedeSegmentFront
            | MonsterKind::DecimillipedeSegmentMiddle
            | MonsterKind::DecimillipedeSegmentBack,
            "CONSTRICT_MOVE",
        ) => Some((v(8, 9), 1)),
        (
            MonsterKind::DecimillipedeSegment
            | MonsterKind::DecimillipedeSegmentFront
            | MonsterKind::DecimillipedeSegmentMiddle
            | MonsterKind::DecimillipedeSegmentBack,
            "BULK_MOVE",
        ) => Some((v(6, 7), 1)),
        (MonsterKind::Entomancer, "BEES_MOVE") => Some((3, v(7, 8))),
        (MonsterKind::Entomancer, "SPEAR_MOVE") => Some((v(18, 20), 1)),
        (MonsterKind::Exoskeleton, "SKITTER_MOVE") => Some((1, v(3, 4))),
        (MonsterKind::Exoskeleton, "MANDIBLES_MOVE") => Some((v(8, 9), 1)),
        (MonsterKind::HunterKiller, "BITE_MOVE") => Some((v(17, 19), 1)),
        (MonsterKind::HunterKiller, "PUNCTURE_MOVE") => Some((v(7, 8), 3)),
        (MonsterKind::InfestedPrism, "JAB_MOVE") => Some((v(15, 17), 1)),
        (MonsterKind::InfestedPrism, "RADIATE_MOVE") => Some((v(11, 13), 1)),
        (MonsterKind::InfestedPrism, "WHIRLWIND_MOVE") => Some((v(5, 6), 3)),
        (MonsterKind::InfestedPrism, "PULSATE_MOVE") => Some((v(8, 10), 1)),
        (MonsterKind::KnowledgeDemon, "SLAP_MOVE") => Some((v(17, 18), 1)),
        (MonsterKind::KnowledgeDemon, "KNOWLEDGE_OVERWHELMING_MOVE") => Some((v(8, 9), 3)),
        (MonsterKind::KnowledgeDemon, "PONDER_MOVE") => Some((v(11, 13), 1)),
        (MonsterKind::LouseProgenitor, "WEB_CANNON_MOVE") => Some((v(9, 10), 1)),
        (MonsterKind::LouseProgenitor, "POUNCE_MOVE") => Some((v(14, 16), 1)),
        (MonsterKind::Myte, "BITE_MOVE") => Some((v(13, 15), 1)),
        (MonsterKind::Myte, "SUCK_MOVE") => Some((v(4, 6), 1)),
        (MonsterKind::Ovicopter, "SMASH_MOVE") => Some((v(16, 17), 1)),
        (MonsterKind::Ovicopter, "TENDERIZER_MOVE") => Some((v(7, 8), 1)),
        (MonsterKind::Rocket, "TARGETING_RETICLE_MOVE") => Some((v(3, 4), 1)),
        (MonsterKind::Rocket, "PRECISION_BEAM_MOVE") => Some((v(18, 20), 1)),
        (MonsterKind::Rocket, "LASER_MOVE") => Some((v(31, 35), 1)),
        (MonsterKind::SlumberingBeetle, "ROLL_OUT_MOVE") => Some((v(16, 18), 1)),
        (MonsterKind::SpinyToad, "SPIKE_EXPLOSION_MOVE") => Some((v(23, 25), 1)),
        (MonsterKind::SpinyToad, "TONGUE_LASH_MOVE") => Some((v(17, 19), 1)),
        (MonsterKind::TheInsatiable, "THRASH_MOVE" | "THRASH_MOVE_2") => Some((v(8, 9), 2)),
        (MonsterKind::TheInsatiable, "LUNGING_BITE_MOVE") => Some((v(28, 31), 1)),
        (MonsterKind::TheObscura, "PIERCING_GAZE_MOVE") => Some((v(10, 11), 1)),
        (MonsterKind::TheObscura, "HARDENING_STRIKE_MOVE") => Some((v(6, 7), 1)),
        (MonsterKind::ThievingHopper, "THIEVERY_MOVE") => Some((v(17, 19), 1)),
        (MonsterKind::ThievingHopper, "HAT_TRICK_MOVE") => Some((v(21, 23), 1)),
        (MonsterKind::ThievingHopper, "NAB_MOVE") => Some((v(14, 16), 1)),
        (MonsterKind::ToughEgg, "NIBBLE_MOVE") => Some((v(4, 5), 1)),
        (MonsterKind::Tunneler, "BITE_MOVE") => Some((v(13, 15), 1)),
        (MonsterKind::Tunneler, "BELOW_MOVE") => Some((v(23, 26), 1)),
        (MonsterKind::Wriggler, "NASTY_BITE_MOVE") => Some((v(6, 7), 1)),
        _ => None,
    }
}

#[inline]
fn attack(c: &mut Combat, e: CId, kind: MonsterKind, state: &str) {
    let band = usize::from(c.run.ascension >= DEADLY_ASC);
    let (damage, hits) = attack_spec(kind, state, band).expect("missing act2 attack spec");
    c.monster_attack_players(e, damage, hits);
}

/// Initial state chosen by factory arguments; None = table default (or not ours).
pub(crate) fn initial_state(kind: MonsterKind, flags: u32, def: &MonsterDef) -> Option<u8> {
    let id = match kind {
        MonsterKind::Exoskeleton => match flags & EXO_SLOT_MASK {
            EXO_FIRST => "SKITTER_MOVE",
            EXO_SECOND => "MANDIBLES_MOVE",
            EXO_THIRD => "ENRAGE_MOVE",
            _ => "RAND",
        },
        MonsterKind::Chomper if flags & CHOMPER_SCREAM_FIRST != 0 => "SCREECH_MOVE",
        MonsterKind::Chomper => "CLAMP_MOVE",
        MonsterKind::Wriggler if flags & WRIGGLER_START_STUNNED != 0 => "SPAWNED_MOVE",
        MonsterKind::Wriggler => "INIT_MOVE",
        MonsterKind::Myte if flags & MYTE_SECOND != 0 => "SUCK_MOVE",
        MonsterKind::Myte => "TOXIC_MOVE",
        MonsterKind::DecimillipedeSegment
        | MonsterKind::DecimillipedeSegmentFront
        | MonsterKind::DecimillipedeSegmentMiddle
        | MonsterKind::DecimillipedeSegmentBack => match flags & DECIMILLIPEDE_START_MASK {
            1 => "BULK_MOVE",
            2 => "CONSTRICT_MOVE",
            _ => "WRITHE_MOVE",
        },
        _ => return None,
    };
    Some(state_index(def, id))
}

/// Act 2 has no callable branch weights; all exported random branches use fixed weight 1.
pub(crate) fn weight(_c: &Combat, _e: CId, _ai: &MonsterAi, _target: u8) -> Option<f64> {
    None
}

/// Conditional branches in r7 order.
pub(crate) fn condition(c: &Combat, e: CId, ai: &MonsterAi, targets: &'static [u8]) -> Option<u8> {
    match ai.kind {
        MonsterKind::BowlbugRock => Some(if ai.flags & ROCK_WAS_FULLY_BLOCKED != 0 {
            targets[0]
        } else {
            targets[1]
        }),
        MonsterKind::Wriggler => Some(match ai.flags & WRIGGLER_SLOT_MASK {
            0 | 2 => targets[0],
            1 | 3 => targets[1],
            _ => targets[2],
        }),
        MonsterKind::Ovicopter => Some(if c.alive_enemies().len() <= 3 {
            targets[0]
        } else {
            targets[1]
        }),
        MonsterKind::SlumberingBeetle => Some(if c.has_power(e, PowerId::Slumber) {
            targets[0]
        } else {
            targets[1]
        }),
        MonsterKind::KnowledgeDemon => Some(if ai.flags & KNOWLEDGE_CURSE_COUNTER_MASK < 3 {
            targets[0]
        } else {
            targets[1]
        }),
        _ => None,
    }
}

#[allow(dead_code)]
pub(crate) fn act2_note_bowlbug_rock_fully_blocked(c: &mut Combat, e: CId) {
    if c.cr(e).monster_id == "BOWLBUG_ROCK"
        && let Some(ai) = c.cr_mut(e).ai.as_mut()
    {
        ai.flags |= ROCK_WAS_FULLY_BLOCKED;
    }
}

pub(crate) fn bowlbug_rock_regain_balance(c: &mut Combat, e: CId) {
    if let Some(ai) = c.cr_mut(e).ai.as_mut() {
        ai.flags &= !ROCK_WAS_FULLY_BLOCKED;
    }
}

fn escape_enemy(c: &mut Combat, e: CId) {
    if !c.cr(e).is_alive() {
        return;
    }
    let before=crate::between::PublicState::of(c.cr(e));
    {
        let cr = c.cr_mut(e);
        cr.escaped = true;
        cr.block = 0;
        cr.in_combat = false;
    }
    if let Some(i) = c.enemies.iter().position(|&x| x == e) {
        c.enemies.remove(i);
    }
    c.sort_enemies_by_slot();
    c.record_creature_lifecycle(e,"escape",before);
    c.check_combat_end();
}

fn raw_remove_primary_power(c: &mut Combat, e: CId, id: PowerId) {
    let Some(p) = c.find_power(e, id) else { return };
    if let Some(i) = c.cr(e).powers.iter().position(|&q| q == p) {
        c.cr_mut(e).powers.remove(i);
    }
}

/// KnowledgeDemon.CurseOfKnowledgeMove: every living target player picks one card of the
/// current set (ChooseCurse, a choose-a-card screen; the game awaits them together, answered here
/// in `Players` order) and its OnChosen applies the curse power to that player; then
/// CurseOfKnowledgeCounter advances while the combat is live.
fn knowledge_curse_next(c: &mut Combat, demon: CId, from: usize) {
    const DISINTEGRATION_DAMAGE: [i32; 3] = [6, 7, 8];
    const SECOND: [CardId; 3] = [CardId::MindRot, CardId::SlothStatus, CardId::WasteAway];
    let flags = c.cr(demon).ai.as_ref().expect("monster without AI").flags;
    let counter = (flags & KNOWLEDGE_CURSE_COUNTER_MASK) as usize;
    let Some(i) = (from..c.players.len()).find(|&i| c.cr(i as CId).is_alive()) else {
        if !c.is_over_or_ending() {
            c.cr_mut(demon).ai.as_mut().unwrap().flags = (flags & !KNOWLEDGE_CURSE_COUNTER_MASK) | (counter as u32 + 1);
        }
        return;
    };
    if counter >= SECOND.len() {
        c.unported("Knowledge Demon curse set index");
        return;
    }
    let player = i as CId;
    let mut options: arrayvec::ArrayVec<KRef, 2> = arrayvec::ArrayVec::new();
    for id in [CardId::Disintegration, SECOND[counter]] {
        let Some(k) = c.new_card(player, id, false) else { return };
        if id == CardId::Disintegration
            && let Some(v) = c.cards[k as usize].vars.iter_mut().find(|(key, _)| *key == VarKey::DisintegrationPower)
        {
            v.1 = DISINTEGRATION_DAMAGE[counter];
        }
        options.push(k);
    }
    let resolver = crate::choice::Resolver {
        src: crate::choice::ChoiceSrc::Monster(MonsterKind::KnowledgeDemon),
        k: demon as KRef,
        target: Some(player),
        step: 0,
        arg: 0,
    };
    c.request_card_choice_for_player(player, &options, PileKind::None, resolver, false);
}

/// Continue a monster move paused on a player's card choice.
pub(crate) fn resolve(c: &mut Combat, r: crate::choice::Resolver, selected: &[KRef]) -> bool {
    let crate::choice::ChoiceSrc::Monster(MonsterKind::KnowledgeDemon) = r.src else {
        return false;
    };
    if let Some(&k) = selected.first() {
        super::cards_ironclad_skills::on_chosen(c, k);
    }
    let player = r.target.expect("curse choice without a player") as usize;
    knowledge_curse_next(c, r.k as CId, player + 1);
    true
}

fn spawn_tough_egg(c: &mut Combat, ovicopter: CId) {
    let slot = c
        .encounter_slots
        .iter()
        .rev()
        .copied()
        .find(|slot| !c.enemies.iter().any(|&x| c.cr(x).slot == Some(*slot)));
    let (cr, ai) = c.create_monster(MonsterKind::ToughEgg, 0);
    let egg = c.add_enemy_in_slot(cr, ai, slot);
    if c.side == Side::Enemy
        && let Some(p) = c.find_power(egg, PowerId::Hatch)
    {
        c.powers[p as usize].amount = 2;
    }
    // Ovicopter.LayEggsMove: CreatureCmd.Add<ToughEgg> (Hatch in AfterAddedToRoom), then
    // PowerCmd.Apply<MinionPower> from the Ovicopter, so Minion follows Hatch.
    raw_remove_primary_power(c, egg, PowerId::Minion);
    c.creature_apply_power(egg, PowerId::Minion, 1, Some(ovicopter), None, false, false);
}

/// C# ThievingHopper.ThieveryMove: select/remove one card per living target in player order,
/// then SwipePower.Steal removes each DeckVersion and binds a fresh power to that owner.
fn thieving_hopper_swipe(c: &mut Combat, hopper: CId) {
    let mut stolen = Vec::new();
    for owner in c.living_players() {
        let mut candidates: Vec<KRef> = c.players[owner].draw.iter()
            .chain(c.players[owner].discard.iter()).copied()
            .filter(|&k| c.cards[k as usize].deck_version.is_some()).collect();
        let priority = |k: KRef| {
            let card = &c.cards[k as usize];
            if card.enchantments.iter().any(|&(name, _)| name == EnchantmentId::Imbued) {
                return 3;
            }
            match card.rarity {
                CardRarity::Uncommon => 0,
                CardRarity::Common | CardRarity::Rare | CardRarity::Event => 1,
                CardRarity::Basic | CardRarity::Quest => 2,
                CardRarity::Ancient => 3,
                _ => 4,
            }
        };
        let Some(best) = candidates.iter().map(|&k| priority(k)).min() else { continue };
        candidates.retain(|&k| priority(k) == best);
        let picked = candidates[c.run.rngs.combat_card_generation.choice_index(candidates.len())];
        let old_pile = c.card_pile(picked);
        c.remove_card_from_piles(picked);
        c.fire_after_card_changed_piles(picked, old_pile, None);
        stolen.push(picked);
    }
    for picked in stolen {
        let owner = c.cards[picked as usize].owner;
        // SwipePower.Steal removes card.DeckVersion from its OWN pile, not
        // card.Owner's deck. TheBall changes combat ownership, not DeckVersion.
        let deck_owner = c.cards[picked as usize].deck_owner;
        let Some(deck_index) = c.cards[picked as usize].deck_version else { continue };
        let deck_index = deck_index as usize;
        if deck_index >= c.player_deck(deck_owner).len() {
            c.unported("Swipe missing player deck card");
            return;
        }
        let card = c.remove_player_deck_card(deck_owner,deck_index);
        // DeckVersion is an index here, rather than the C# object reference. Reindex only
        // this owner's cards, preserving every other player's independent deck indices.
        for card in c.cards.iter_mut().filter(|card| card.deck_owner == deck_owner) {
            match card.deck_version {
                Some(i) if i as usize == deck_index => card.deck_version = None,
                Some(i) if i as usize > deck_index => card.deck_version = Some(i - 1),
                _ => {}
            }
        }
        let stored = c.players[owner].stolen_deck_cards.len();
        c.players[owner].stolen_deck_cards.push(Some(card));
        if let Some(p) = c.apply_targeted_power(hopper, PowerId::Swipe, 1, Some(hopper), owner) {
            c.powers[p as usize].vars[0] = stored as i32 + 1;
        }
    }
}

pub(crate) fn move_effect(c: &mut Combat, e: CId, kind: MonsterKind, state: &'static str) -> bool {
    let asc = c.run.ascension;
    match (kind, state) {
        (MonsterKind::FakeMerchantMonster, "SWIPE_MOVE" | "SPEW_COINS_MOVE") => attack(c, e, kind, state),
        (MonsterKind::FakeMerchantMonster, "THROW_RELIC_MOVE") => {
            attack(c, e, kind, state);
            c.monster_power_to_players(e, PowerId::Frail, 1);
        }
        (MonsterKind::FakeMerchantMonster, "ENRAGE_MOVE") => {
            c.apply_power_to(e, PowerId::Strength, 2, Some(e));
        }
        (MonsterKind::ThievingHopper, "THIEVERY_MOVE") => {
            thieving_hopper_swipe(c, e);
            attack(c, e, kind, state);
        }
        (MonsterKind::ThievingHopper, "FLUTTER_MOVE") => {
            c.apply_power_to(e, PowerId::Flutter, 5, Some(e));
        }
        (MonsterKind::ThievingHopper, "HAT_TRICK_MOVE" | "NAB_MOVE") => attack(c, e, kind, state),
        (MonsterKind::ThievingHopper, "ESCAPE_MOVE") => escape_enemy(c, e),

        (MonsterKind::Tunneler, "BITE_MOVE" | "BELOW_MOVE") => attack(c, e, kind, state),
        (MonsterKind::Tunneler, "BURROW_MOVE") => {
            c.apply_power_to(e, PowerId::Burrowed, 1, Some(e));
            c.gain_move_block(e, asc_value(asc, TOUGH_ASC, 37, 32));
        }
        (MonsterKind::Tunneler, "DIZZY_MOVE") => {}

        (MonsterKind::BowlbugEgg, "BITE_MOVE") => {
            attack(c, e, kind, state);
            c.gain_move_block(e, asc_value(asc, DEADLY_ASC, 8, 7));
        }
        (MonsterKind::BowlbugNectar, "THRASH_MOVE" | "THRASH2_MOVE") => attack(c, e, kind, state),
        (MonsterKind::BowlbugNectar, "BUFF_MOVE") => {
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 16, 15),
                Some(e),
            );
        }
        (MonsterKind::BowlbugRock, "HEADBUTT_MOVE") => {
            attack(c, e, kind, state);
            // BowlbugRock.HeadbuttMove: knocked off balance by a fully blocked hit, it stuns
            // itself (`CreatureCmd.Stun(Creature, DizzyMove)`: a STUNNED move that regains balance
            // and then continues with Headbutt), so the POST_HEADBUTT branch never sees the flag.
            let off_balance = c.cr(e).ai.as_ref().is_some_and(|ai| ai.flags & ROCK_WAS_FULLY_BLOCKED != 0);
            if off_balance {
                c.stun_enemy_with_move(e, crate::monster::StunMove::BowlbugRockDizzy, None);
            }
        }
        (MonsterKind::BowlbugRock, "DIZZY_MOVE") => bowlbug_rock_regain_balance(c, e),
        (MonsterKind::BowlbugSilk, "THRASH_MOVE") => attack(c, e, kind, state),
        (MonsterKind::BowlbugSilk, "TOXIC_SPIT_MOVE") => c.monster_power_to_players(e, PowerId::Weak, 1),

        (MonsterKind::Exoskeleton, "SKITTER_MOVE" | "MANDIBLES_MOVE") => attack(c, e, kind, state),
        (MonsterKind::Exoskeleton, "ENRAGE_MOVE") => {
            c.apply_power_to(e, PowerId::Strength, 2, Some(e));
        }

        (MonsterKind::Chomper, "CLAMP_MOVE") => attack(c, e, kind, state),
        (MonsterKind::Chomper, "SCREECH_MOVE") => {
            status(c, CardId::Dazed, 3, PileKind::Discard, PilePos::Bottom);
        }

        (MonsterKind::HunterKiller, "TENDERIZING_GOOP_MOVE") => {
            c.monster_power_to_players(e, PowerId::Tender, 1);
        }
        (MonsterKind::HunterKiller, "BITE_MOVE" | "PUNCTURE_MOVE") => attack(c, e, kind, state),

        (MonsterKind::Wriggler, "SPAWNED_MOVE") => {}
        (MonsterKind::Wriggler, "NASTY_BITE_MOVE") => attack(c, e, kind, state),
        (MonsterKind::Wriggler, "WRIGGLE_MOVE") => {
            status(c, CardId::Infection, 1, PileKind::Discard, PilePos::Bottom);
            c.apply_power_to(e, PowerId::Strength, 2, Some(e));
        }

        (MonsterKind::LouseProgenitor, "WEB_CANNON_MOVE") => {
            attack(c, e, kind, state);
            c.monster_power_to_players(e, PowerId::Frail, 2);
        }
        (MonsterKind::LouseProgenitor, "CURL_AND_GROW_MOVE") => {
            c.gain_move_block(e, asc_value(asc, TOUGH_ASC, 18, 14));
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 7, 5),
                Some(e),
            );
        }
        (MonsterKind::LouseProgenitor, "POUNCE_MOVE") => attack(c, e, kind, state),

        (MonsterKind::Myte, "TOXIC_MOVE") => {
            status(c, CardId::Toxic, 2, PileKind::Hand, PilePos::Bottom);
        }
        (MonsterKind::Myte, "BITE_MOVE") => attack(c, e, kind, state),
        (MonsterKind::Myte, "SUCK_MOVE") => {
            attack(c, e, kind, state);
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 3, 2),
                Some(e),
            );
        }

        (MonsterKind::ToughEgg, "HATCH_MOVE") => {
            let min_hp = asc_value(asc, TOUGH_ASC, 20, 19);
            let max_hp = asc_value(asc, TOUGH_ASC, 23, 22);
            let hp = c.run.rngs.niche.next_int(min_hp as i64, max_hp as i64) as i32;
            // ToughEgg hatch: ScaleHpForMultiplayer(rolled hatchling HP).
            let hp = c.scale_monster_hp(hp);
            let ids: Vec<PowerId> = c
                .cr(e)
                .powers
                .iter()
                .map(|&p| c.powers[p as usize].id)
                .collect();
            for id in ids {
                if id != PowerId::Minion {
                    raw_remove_primary_power(c, e, id);
                }
            }
            c.cr_mut(e).max_hp = hp;
            c.cr_mut(e).hp = hp;
        }
        (MonsterKind::ToughEgg, "NIBBLE_MOVE") => attack(c, e, kind, state),

        (MonsterKind::Ovicopter, "LAY_EGGS_MOVE") => {
            for _ in 0..3 {
                spawn_tough_egg(c, e);
            }
        }
        (MonsterKind::Ovicopter, "SMASH_MOVE") => attack(c, e, kind, state),
        (MonsterKind::Ovicopter, "TENDERIZER_MOVE") => {
            attack(c, e, kind, state);
            c.monster_power_to_players(e, PowerId::Vulnerable, 2);
        }
        (MonsterKind::Ovicopter, "NUTRITIONAL_PASTE_MOVE") => {
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 4, 3),
                Some(e),
            );
        }

        (MonsterKind::SlumberingBeetle, "SNORE_MOVE") => {}
        (MonsterKind::SlumberingBeetle, "ROLL_OUT_MOVE") => {
            attack(c, e, kind, state);
            c.apply_power_to(e, PowerId::Strength, 2, Some(e));
        }

        (MonsterKind::SpinyToad, "PROTRUDING_SPIKES_MOVE") => {
            c.apply_power_to(e, PowerId::Thorns, 5, Some(e));
        }
        (MonsterKind::SpinyToad, "SPIKE_EXPLOSION_MOVE") => {
            attack(c, e, kind, state);
            c.apply_power_to(e, PowerId::Thorns, -5, Some(e));
        }
        (MonsterKind::SpinyToad, "TONGUE_LASH_MOVE") => attack(c, e, kind, state),

        (MonsterKind::TheObscura, "ILLUSION_MOVE") => {
            add(c, MonsterKind::Parafright, 0);
        }
        (MonsterKind::TheObscura, "PIERCING_GAZE_MOVE") => attack(c, e, kind, state),
        (MonsterKind::TheObscura, "SAIL_MOVE") => {
            for ally in c.alive_enemies() {
                c.apply_power_to(ally, PowerId::Strength, 3, Some(e));
            }
        }
        (MonsterKind::TheObscura, "HARDENING_STRIKE_MOVE") => {
            attack(c, e, kind, state);
            c.gain_move_block(e, asc_value(asc, DEADLY_ASC, 7, 6));
        }

        (
            MonsterKind::DecimillipedeSegment
            | MonsterKind::DecimillipedeSegmentFront
            | MonsterKind::DecimillipedeSegmentMiddle
            | MonsterKind::DecimillipedeSegmentBack,
            "WRITHE_MOVE" | "CONSTRICT_MOVE" | "BULK_MOVE",
        ) => {
            attack(c, e, kind, state);
            match state {
                "CONSTRICT_MOVE" => c.monster_power_to_players(e, PowerId::Weak, 1),
                "BULK_MOVE" => {
                    c.apply_power_to(e, PowerId::Strength, 2, Some(e));
                }
                _ => {}
            }
        }
        (
            MonsterKind::DecimillipedeSegment
            | MonsterKind::DecimillipedeSegmentFront
            | MonsterKind::DecimillipedeSegmentMiddle
            | MonsterKind::DecimillipedeSegmentBack,
            "DEAD_MOVE",
        ) => {}
        (
            MonsterKind::DecimillipedeSegment
            | MonsterKind::DecimillipedeSegmentFront
            | MonsterKind::DecimillipedeSegmentMiddle
            | MonsterKind::DecimillipedeSegmentBack,
            "REATTACH_MOVE",
        ) => {
            // powers/remaining_c.py ReattachPower.do_reattach.
            let amount = c.power_amount(e, PowerId::Reattach);
            c.set_power_private_int(e, PowerId::Reattach, 0, 0);
            let cr = c.cr_mut(e);
            cr.hp = amount.max(1).min(cr.max_hp);
            cr.escaped = false;
            cr.death_processed = false;
            cr.in_combat = true;
        }

        (MonsterKind::Entomancer, "BEES_MOVE" | "SPEAR_MOVE") => attack(c, e, kind, state),
        (MonsterKind::Entomancer, "PHEROMONE_SPIT_MOVE") => {
            if c.power_amount(e, PowerId::PersonalHive) < 3 {
                c.apply_power_to(e, PowerId::PersonalHive, 1, Some(e));
                c.apply_power_to(e, PowerId::Strength, 1, Some(e));
            } else {
                c.apply_power_to(e, PowerId::Strength, 2, Some(e));
            }
        }

        (MonsterKind::InfestedPrism, "JAB_MOVE" | "WHIRLWIND_MOVE") => attack(c, e, kind, state),
        (MonsterKind::InfestedPrism, "RADIATE_MOVE") => {
            attack(c, e, kind, state);
            c.gain_move_block(e, asc_value(asc, DEADLY_ASC, 13, 11));
        }
        (MonsterKind::InfestedPrism, "PULSATE_MOVE") => {
            attack(c, e, kind, state);
            c.gain_move_block(e, asc_value(asc, TOUGH_ASC, 22, 20));
            c.apply_power_to(
                e,
                PowerId::VitalSpark,
                asc_value(asc, DEADLY_ASC, 3, 2),
                Some(e),
            );
        }

        (MonsterKind::TheInsatiable, "LIQUIFY_GROUND_MOVE") => {
            // C# TheInsatiable.LiquifyMove: create one instanced SandpitPower per target player,
            // then create that player's three draw-pile and three discard-pile Frantic Escapes.
            let targets = c.living_players();
            for &target in &targets {
                c.apply_targeted_power(e, PowerId::Sandpit, 4, Some(e), target);
            }
            for target in targets {
                for _ in 0..3 {
                    c.create_card_in_pile(
                        target,
                        CardId::FranticEscape,
                        false,
                        PileKind::Draw,
                        PilePos::Random,
                    );
                }
                for _ in 0..3 {
                    c.create_card_in_pile(
                        target,
                        CardId::FranticEscape,
                        false,
                        PileKind::Discard,
                        PilePos::Random,
                    );
                }
            }
        }
        (MonsterKind::TheInsatiable, "THRASH_MOVE" | "THRASH_MOVE_2" | "LUNGING_BITE_MOVE") => {
            attack(c, e, kind, state)
        }
        (MonsterKind::TheInsatiable, "SALIVATE_MOVE") => {
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 3, 2),
                Some(e),
            );
        }

        (MonsterKind::KnowledgeDemon, "CURSE_OF_KNOWLEDGE_MOVE") => knowledge_curse_next(c, e, 0),
        (MonsterKind::KnowledgeDemon, "SLAP_MOVE" | "KNOWLEDGE_OVERWHELMING_MOVE") => {
            attack(c, e, kind, state)
        }
        (MonsterKind::KnowledgeDemon, "PONDER_MOVE") => {
            attack(c, e, kind, state);
            if !c.is_over {
                // KnowledgeDemon.PonderMove: Heal(30 x CombatState.Players.Count).
                let players = c.players.len() as i32;
                c.heal(e, 30 * players);
                c.apply_power_to(
                    e,
                    PowerId::Strength,
                    asc_value(asc, DEADLY_ASC, 3, 2),
                    Some(e),
                );
            }
        }

        (MonsterKind::Crusher, "THRASH_MOVE" | "ENLARGING_STRIKE_MOVE") => {
            attack(c, e, kind, state)
        }
        (MonsterKind::Crusher, "BUG_STING_MOVE") => {
            attack(c, e, kind, state);
            c.monster_power_to_players(e, PowerId::Weak, 2);
            c.monster_power_to_players(e, PowerId::Frail, 2);
        }
        (MonsterKind::Crusher, "ADAPT_MOVE") => {
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 3, 2),
                Some(e),
            );
        }
        (MonsterKind::Crusher, "GUARDED_STRIKE_MOVE") => {
            attack(c, e, kind, state);
            c.gain_move_block(e, 18);
        }

        (MonsterKind::Rocket, "TARGETING_RETICLE_MOVE" | "PRECISION_BEAM_MOVE" | "LASER_MOVE") => {
            attack(c, e, kind, state)
        }
        (MonsterKind::Rocket, "CHARGE_UP_MOVE") => {
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 3, 2),
                Some(e),
            );
        }
        (MonsterKind::Rocket, "RECHARGE_MOVE") => {}

        _ => return false,
    }
    true
}

fn decimillipede_room_setup(c: &mut Combat, e: CId) {
    let mut hp = c.cr(e).max_hp;
    if hp % 2 == 1 {
        hp += 1;
    }
    let (min_hp, max_hp) = if c.run.ascension >= TOUGH_ASC {
        (46, 52)
    } else {
        (40, 46)
    };
    // DecimillipedeSegment.AfterAddedToRoom wraps within the multiplayer-scaled HP range.
    let (min_hp, max_hp) = (c.scale_monster_hp(min_hp), c.scale_monster_hp(max_hp));
    loop {
        let collision = c
            .enemies
            .iter()
            .copied()
            .any(|other| other != e && c.cr(other).max_hp == hp);
        if !collision {
            break;
        }
        hp += 2;
        if hp > max_hp {
            hp = min_hp;
        }
    }
    c.cr_mut(e).max_hp = hp;
    c.cr_mut(e).hp = hp;
    c.apply_power_to(e, PowerId::Reattach, 25, Some(e));
}

pub(crate) fn setup_encounter(c: &mut Combat, encounter_id: crate::identity::EncounterId, rng: &mut Rng) -> bool {
    use crate::identity::EncounterId as E;
    match encounter_id {
        E::FakeMerchantEventEncounter => {
            c.configure_encounter_slots(&["merchant"]);
            add_in_slot(c, MonsterKind::FakeMerchantMonster, 0, "merchant");
        }
        E::DenseVegetationEvent => {
            // C# DenseVegetationEventEncounter.GenerateMonsters: four ordinary
            // Wrigglers, StartStunned=false, with the normal C# ascension scaling.
            for (index, slot) in ["wriggler1", "wriggler2", "wriggler3", "wriggler4"]
                .into_iter().enumerate()
            {
                add_in_slot(c, MonsterKind::Wriggler, index as u32, slot);
            }
        }
        E::ThievingHopperWeak => {
            add(c, MonsterKind::ThievingHopper, 0);
        }
        E::TunnelerWeak => {
            add(c, MonsterKind::Tunneler, 0);
        }
        E::BowlbugsWeak => {
            let workers = [MonsterKind::BowlbugEgg, MonsterKind::BowlbugNectar];
            let worker = workers[rng.choice_index(workers.len())];
            add(c, MonsterKind::BowlbugRock, 0);
            add(c, worker, 0);
        }
        E::ExoskeletonsWeak => {
            add(c, MonsterKind::Exoskeleton, EXO_FIRST);
            add(c, MonsterKind::Exoskeleton, EXO_SECOND);
            add(c, MonsterKind::Exoskeleton, EXO_THIRD);
        }

        E::BowlbugsNormal => {
            let mut workers = vec![
                MonsterKind::BowlbugEgg,
                MonsterKind::BowlbugSilk,
                MonsterKind::BowlbugNectar,
            ];
            let first = workers.remove(rng.choice_index(workers.len()));
            let second = workers[rng.choice_index(workers.len())];
            add(c, MonsterKind::BowlbugRock, 0);
            add(c, first, 0);
            add(c, second, 0);
        }
        E::ChompersNormal => {
            add(c, MonsterKind::Chomper, 0);
            add(c, MonsterKind::Chomper, CHOMPER_SCREAM_FIRST);
        }
        E::ExoskeletonsNormal => {
            add(c, MonsterKind::Exoskeleton, EXO_FIRST);
            add(c, MonsterKind::Exoskeleton, EXO_SECOND);
            add(c, MonsterKind::Exoskeleton, EXO_THIRD);
            add(c, MonsterKind::Exoskeleton, EXO_FOURTH);
        }
        E::HunterKillerNormal => {
            add(c, MonsterKind::HunterKiller, 0);
        }
        E::LouseProgenitorNormal => {
            add(c, MonsterKind::LouseProgenitor, 0);
        }
        E::MytesNormal => {
            add(c, MonsterKind::Myte, 0);
            add(c, MonsterKind::Myte, MYTE_SECOND);
        }
        E::OvicopterNormal => {
            c.configure_encounter_slots(&["egg1", "egg2", "egg3", "egg4", "egg5", "ovicopter"]);
            add_in_slot(c, MonsterKind::Ovicopter, 0, "ovicopter");
        }
        E::SlumberingBeetleNormal => {
            add(c, MonsterKind::BowlbugRock, 0);
            add(c, MonsterKind::BowlbugSilk, 0);
            add(c, MonsterKind::SlumberingBeetle, 0);
        }
        E::SpinyToadNormal => {
            add(c, MonsterKind::SpinyToad, 0);
        }
        E::TheObscuraNormal => {
            c.configure_encounter_slots(&["illusion", "obscura"]);
            add_in_slot(c, MonsterKind::TheObscura, 0, "obscura");
        }
        E::TunnelerNormal => {
            let bugs = [MonsterKind::BowlbugEgg, MonsterKind::BowlbugSilk];
            add(c, bugs[rng.choice_index(bugs.len())], 0);
            add(c, MonsterKind::Tunneler, 0);
        }

        E::DecimillipedeElite => {
            let starter = rng.next_int_exclusive(0, 3) as u32;
            let front = add(c, MonsterKind::DecimillipedeSegmentFront, starter);
            let middle = add(
                c,
                MonsterKind::DecimillipedeSegmentMiddle,
                (starter + 1) % 3,
            );
            let back = add(c, MonsterKind::DecimillipedeSegmentBack, (starter + 2) % 3);
            decimillipede_room_setup(c, front);
            decimillipede_room_setup(c, middle);
            decimillipede_room_setup(c, back);
        }
        E::EntomancerElite => {
            add(c, MonsterKind::Entomancer, 0);
        }
        E::InfestedPrismsElite => {
            add(c, MonsterKind::InfestedPrism, 0);
        }

        E::TheInsatiableBoss => {
            add(c, MonsterKind::TheInsatiable, 0);
        }
        E::KnowledgeDemonBoss => {
            add(c, MonsterKind::KnowledgeDemon, 0);
        }
        E::KaiserCrabBoss => {
            add(c, MonsterKind::Crusher, 0);
            let rocket = add(c, MonsterKind::Rocket, 0);
            // Rocket.cs: SurroundedPower on `GetOpponentsOf(Creature)` (every player).
            c.monster_power_to_players(rocket, PowerId::Surrounded, 1);
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::Card;
    use crate::combat::PLAYER;
    use crate::creature::Creature;
    use crate::relic::Relic;
    use crate::run::{RunManager, RunOptions};

    #[test]
    fn fake_merchant_encounter_and_all_moves_match_csharp_at_both_ascensions() {
        for (asc,hp,swipe,throw) in [(0,165,13,9),(10,175,15,10)] {
            for (state,damage) in [("SWIPE_MOVE",swipe),("SPEW_COINS_MOVE",16),
                ("THROW_RELIC_MOVE",throw),("ENRAGE_MOVE",0)] {
                let mgr = RunManager::new(4100000,RunOptions { ascension: asc,..Default::default() });
                let mut c = Combat::new(mgr.run_ctx().clone(),17);
                c.setup_encounter("FAKE_MERCHANT_EVENT_ENCOUNTER",&mut Rng::new(12));
                assert_eq!(c.enemies.len(),1);
                let e = c.enemies[0];
                assert_eq!(c.cr(e).max_hp,hp);
                let ai = c.cr(e).ai.as_ref().unwrap();
                assert_eq!(ai.state_id(ai.current),"SWIPE_MOVE");
                c.start_combat();
                let before = c.cr(PLAYER).hp;
                assert!(move_effect(&mut c,e,MonsterKind::FakeMerchantMonster,state));
                assert_eq!(c.cr(PLAYER).hp,before-damage);
                if state == "THROW_RELIC_MOVE" {
                    let p = c.find_power(PLAYER,PowerId::Frail).unwrap();
                    assert_eq!(c.powers[p as usize].amount,1);
                }
                if state == "ENRAGE_MOVE" {
                    let p = c.find_power(e,PowerId::Strength).unwrap();
                    assert_eq!(c.powers[p as usize].amount,2);
                }
                assert!(c.fault.is_none(),"{:?}",c.fault);
            }
        }
    }

    fn sandpit_combat(players: usize) -> (Combat, CId) {
        let mut run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        run.act_index = 1;
        run.room_type = RoomType::Boss;
        let deck = run.deck.clone();
        let mut c = Combat::new(run, 1);
        for _ in 1..players {
            c.add_player(80, 80, 3, 0, &deck);
        }
        c.setup_encounter("THE_INSATIABLE_BOSS", &mut Rng::new(1));
        c.start_combat();
        let e = c.enemies[0];
        assert!(move_effect(&mut c, e, MonsterKind::TheInsatiable, "LIQUIFY_GROUND_MOVE"));
        assert!(c.fault.is_none(), "{:?}", c.fault);
        (c, e)
    }

    #[test]
    fn sandpit_liquify_binds_each_player_and_generates_their_escapes() {
        for players in 1..=4 {
            let (c, e) = sandpit_combat(players);
            let powers = c.cr(e).power_list();
            assert_eq!(powers.len(), players);
            for (who, &p) in powers.iter().enumerate() {
                assert_eq!(c.powers[p as usize].id, PowerId::Sandpit);
                assert_eq!(c.powers[p as usize].target, Some(who as CId));
                assert_eq!(c.powers[p as usize].amount, 4);
                for pile in [&c.players[who as CId].draw, &c.players[who as CId].discard] {
                    let escapes: Vec<_> = pile.iter().filter(|&&k| c.cards[k as usize].id == CardId::FranticEscape).collect();
                    assert_eq!(escapes.len(), 3);
                    assert!(escapes.iter().all(|&&k| c.cards[k as usize].owner == who as CId));
                }
            }
        }
    }

    #[test]
    fn sandpit_escape_extends_only_its_owner_and_extra_instances_expire() {
        let (mut c, e) = sandpit_combat(4);
        // Extend the primary instance so the three extra instances must expire independently.
        let escape = c.new_card(PLAYER, CardId::FranticEscape, false).unwrap();
        assert!(super::super::cards_ironclad_skills::play(&mut c, escape, None));
        assert_eq!(c.cards[escape as usize].cost, 2);
        let powers = c.cr(e).power_list();
        c.fire_after_side_turn_start(Side::Player);
        assert_eq!(c.powers[powers[0] as usize].amount, 5);
        for &p in &powers[1..] {
            assert_eq!(c.powers[p as usize].amount, 4);
        }
        for _ in 0..4 {
            c.fire_after_side_turn_start(Side::Enemy);
        }
        assert!(c.cr(PLAYER).is_alive());
        for who in 1..4 {
            assert!(c.cr(who).is_dead());
        }
        assert_eq!(c.cr(e).power_list().as_slice(), &[powers[0]]);
        assert_eq!(c.powers[powers[0] as usize].amount, 1);
        c.fire_after_side_turn_start(Side::Enemy);
        assert!(c.cr(PLAYER).is_dead());
        assert!(c.cr(e).power_list().is_empty());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }

    #[test]
    fn sandpit_escape_selects_a_teammates_instance() {
        let (mut c, e) = sandpit_combat(2);
        let escape = c.new_card(1, CardId::FranticEscape, false).unwrap();
        assert!(super::super::cards_ironclad_skills::play(&mut c, escape, None));
        let powers = c.cr(e).power_list();
        assert_eq!(c.powers[powers[0] as usize].amount, 4);
        assert_eq!(c.powers[powers[1] as usize].amount, 5);
        assert_eq!(c.cards[escape as usize].cost, 2);
    }

    #[test]
    fn sandpit_forces_death_past_revivers_and_kills_only_the_targets_osty() {
        let (mut c, e) = sandpit_combat(2);
        c.run.relics.push(Relic::new(RelicId::LizardTail));
        c.run.potions = vec![Some(PotionId::Fairyinabottle)];
        let mut pets = Vec::new();
        for (owner, name) in [(PLAYER, "OSTY"), (1, "OSTY"), (PLAYER, "BYRDPIP")] {
            let mut pet = Creature::player(20, 20);
            pet.is_player = false;
            pet.is_pet = true;
            pet.pet_owner = Some(owner);
            pet.monster_id = name;
            let id = c.creatures.len() as CId;
            c.creatures.push(pet);
            c.allies.push(id);
            pets.push(id);
        }
        let p = c.cr(e).power_list()[0];
        c.remove_power(e, p);
        assert!(c.cr(PLAYER).is_dead());
        assert!(c.cr(pets[0]).is_dead());
        assert!(c.cr(1).is_alive());
        assert!(c.cr(pets[1]).is_alive());
        assert!(c.cr(pets[2]).is_alive());
        assert_eq!(c.run.potions, vec![Some(PotionId::Fairyinabottle)]);
        assert!(!c.run.relics.last().unwrap().flag1);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }

    #[test]
    fn sandpit_does_not_kill_after_owner_or_target_death() {
        let (mut c, e) = sandpit_combat(2);
        let p = c.cr(e).power_list()[0];
        c.kill_creature(PLAYER);
        c.remove_power(e, p);
        assert!(c.cr(1).is_alive());
        assert!(c.fault.is_none(), "{:?}", c.fault);

        let (mut c, e) = sandpit_combat(2);
        c.kill_creature(e);
        assert!(c.cr(PLAYER).is_alive());
        assert!(c.cr(1).is_alive());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }

    fn hopper_combat(players: usize) -> (Combat, CId) {
        let mut run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        run.relics.clear();
        run.deck = [CardId::DefendIronclad, CardId::Footwork, CardId::Footwork]
            .into_iter().enumerate()
            .map(|(i, id)| Card::from_factory(id, false, i as u32 + 1).unwrap()).collect();
        let deck = run.deck.clone();
        let mut c = Combat::new(run, 1);
        for _ in 1..players {
            c.add_player(80, 80, 3, 0, &deck);
        }
        for owner in 0..players as CId {
            c.players[owner].draw = c.players[owner].starting_deck.clone();
        }
        let (creature, ai) = c.create_monster(MonsterKind::ThievingHopper, 0);
        let hopper = c.add_enemy(creature, ai);
        (c, hopper)
    }

    #[test]
    fn hopper_preserves_retained_guiltys_mutated_deck_object_when_stolen() {
        let (mut c,hopper)=hopper_combat(2);
        for owner in 0..2u8 {
            let mut guilty=Card::from_factory(CardId::Guilty,false,100+owner as u32).unwrap();
            guilty.owner=owner;
            c.player_deck_mut(owner).clear();
            c.push_player_deck_card(owner,guilty.clone());
            let mut relic=crate::relic::Relic::new(RelicId::BingBong);relic.owner=owner;
            relic.bing_bong_cards_to_skip=Some(Default::default());
            relic.bing_bong_cards_to_skip.as_mut().unwrap().insert(guilty.clone());
            c.run.relics.push(relic);
            // Prior combats mutate the original, not the acquisition snapshot.
            c.player_deck_mut(owner)[0].vars.iter_mut().find(|(k,_)|*k==VarKey::Combats).unwrap().1=2;
            let k=c.new_card(owner,CardId::Guilty,false).unwrap();
            c.cards[k as usize].deck_version=Some(0);c.cards[k as usize].deck_owner=owner;
            c.players[owner].draw.clear();c.players[owner].discard.clear();
            c.players[owner].draw.push(k);
        }
        thieving_hopper_swipe(&mut c,hopper);
        for owner in 0..2u8 {
            assert!(c.player_deck(owner).is_empty());
            let stored=c.run.relics.iter().find(|r|r.owner==owner).unwrap()
                .bing_bong_cards_to_skip.as_ref().unwrap().entries[0].as_ref().unwrap();
            assert_eq!(stored.var(VarKey::Combats),Some(2));
        }
    }

    #[test]
    fn hopper_swipe_queues_each_players_original_deck_card_without_returning_it() {
        for players in 1..=4 {
            let (mut c, hopper) = hopper_combat(players);
            let mut rng = c.run.rngs.combat_card_generation.clone();
            let expected: Vec<_> = (0..players as CId).map(|owner| {
                let k = c.players[owner].draw[1 + rng.choice_index(2)];
                // Combat-only changes must never leak into the returned DeckVersion.
                c.cards[k as usize].cost = 99;
                (k, c.cards[k as usize].deck_version.unwrap() as usize)
            }).collect();
            thieving_hopper_swipe(&mut c, hopper);
            let swipes: Vec<_> = c.cr(hopper).power_list().into_iter()
                .filter(|&p| c.powers[p as usize].id == PowerId::Swipe).collect();
            assert_eq!(swipes.len(), players);
            assert_eq!(c.run.rngs.combat_card_generation.counter, rng.counter);
            for (owner, &p) in swipes.iter().enumerate() {
                let owner = owner as CId;
                let (k, removed) = expected[owner as usize];
                assert_eq!(c.powers[p as usize].target, Some(owner));
                assert_eq!(c.powers[p as usize].amount, 1);
                assert!(!c.players[owner].draw.contains(&k));
                assert_eq!(c.player_deck(owner).len(), 2);
                for &other in &c.players[owner].draw {
                    let index = c.cards[other as usize].deck_version.unwrap() as usize;
                    assert_eq!(c.player_deck(owner)[index].id, c.cards[other as usize].id);
                }
                assert_eq!(c.players[owner].stolen_deck_cards[0].as_ref().unwrap().instance_id,
                    removed as u32 + 1);
            }
            c.kill_creature(hopper);
            for owner in 0..players as CId {
                let deck = c.player_deck(owner);
                assert_eq!(deck.len(), 2);
                assert!(c.players[owner].stolen_deck_cards[0].is_none());
                let returned = c.end_rewards.iter().find_map(|r| match r {
                    crate::combat::EndReward::SpecialCard { owner: target, card } if *target == owner => Some(card),
                    _ => None,
                }).unwrap();
                assert_eq!(returned.instance_id, expected[owner as usize].1 as u32 + 1);
                assert_eq!(returned.cost, card_info(CardId::Footwork, false).unwrap().cost);
                assert_eq!(returned.owner, owner);
            }
            assert_eq!(c.end_rewards.len(), players);
            assert_eq!(c.fault, None);
        }
    }

    #[test]
    fn hopper_swipe_tracks_given_cards_original_deck_and_returns_to_current_owner() {
        for original in 0..4u8 {
            let recipient = (original + 1) % 4;
            let (mut c, hopper) = hopper_combat(4);
            // A transferred deck copy can have a valid original index outside
            // the recipient's shorter deck (the wide census failure).
            let transferred = c.players[original].draw[2];
            for p in 0..4 { c.players[p].draw.clear(); c.players[p].discard.clear(); }
            c.cards[transferred as usize].owner = recipient;
            c.player_deck_mut(recipient).truncate(1);
            c.players[recipient].draw.push(transferred);
            thieving_hopper_swipe(&mut c, hopper);
            assert_eq!(c.fault, None);
            assert_eq!(c.player_deck(original).len(), 2);
            assert_eq!(c.player_deck(recipient).len(), 1);
            let stolen = c.players[recipient].stolen_deck_cards[0].as_ref().unwrap();
            assert_eq!(stolen.id, CardId::Footwork);
            c.kill_creature(hopper);
            let returned = c.end_rewards.iter().find_map(|r| match r {
                crate::combat::EndReward::SpecialCard { owner, card } if *owner == recipient => Some(card),
                _ => None,
            }).unwrap();
            assert_eq!((returned.id, returned.owner), (CardId::Footwork, recipient));
            assert_eq!(c.fault, None);
        }
    }

    #[test]
    fn hopper_swipe_skips_dead_players_and_ineligible_piles_without_rng_draws() {
        let (mut c, hopper) = hopper_combat(4);
        c.cr_mut(0).hp = 0;
        c.players[1].hand = c.players[1].draw.drain(..).collect();
        c.players[2].exhaust = std::mem::take(&mut c.players[2].draw);
        // A generated card in draw has no DeckVersion and cannot be stolen.
        let generated = c.new_card(1, CardId::Footwork, false).unwrap();
        c.players[1].draw.push(generated);
        let selected = c.players[3].draw[1];
        c.players[3].draw.clear();
        c.players[3].discard.push(selected);
        let mut rng = c.run.rngs.combat_card_generation.clone();
        rng.choice_index(1);
        thieving_hopper_swipe(&mut c, hopper);
        let swipes: Vec<_> = c.cr(hopper).power_list().into_iter()
            .filter(|&p| c.powers[p as usize].id == PowerId::Swipe).collect();
        assert_eq!(swipes.len(), 1);
        assert_eq!(c.powers[swipes[0] as usize].target, Some(3));
        assert_eq!(c.run.rngs.combat_card_generation.counter, rng.counter);
        for owner in 0..3 {
            assert_eq!(c.player_deck(owner).len(), 3);
        }
        assert_eq!(c.player_deck(3).len(), 2);
        assert!(c.players[3].discard.is_empty());
        assert_eq!(c.fault, None);
    }

    #[test]
    fn hopper_swipe_deprioritizes_imbued_and_keeps_cards_when_escaping() {
        let (mut c, hopper) = hopper_combat(2);
        for owner in 0..2 {
            let cards = c.players[owner].draw.clone();
            for &k in &cards[1..] {
                c.cards[k as usize].enchantments.push((crate::identity::EnchantmentId::Imbued, 1));
            }
        }
        thieving_hopper_swipe(&mut c, hopper);
        for owner in 0..2 {
            let stolen = c.players[owner].stolen_deck_cards[0].as_ref().unwrap();
            assert_eq!(stolen.id, CardId::DefendIronclad);
            assert_eq!(c.player_deck(owner).len(), 2);
        }
        escape_enemy(&mut c, hopper);
        for owner in 0..2 {
            assert_eq!(c.player_deck(owner).len(), 2);
        }
        assert_eq!(c.fault, None);
    }

    #[test]
    fn act2_attack_intents_match_monster_defs_a0_a10() {
        let mut checked = 0usize;
        for &kind in ACT2_KINDS {
            for band in 0..=1 {
                let def = monster_def(kind, band);
                for state in def.states {
                    let intent = match state.kind {
                        StateKind::Move { intents, .. } => intents.iter().find(|i| {
                            matches!(i.kind, IntentType::Attack | IntentType::MultiAttack)
                        }),
                        _ => None,
                    };
                    match intent {
                        Some(intent) => {
                            let spec = attack_spec(kind, state.id, band).unwrap_or_else(|| {
                                panic!("missing attack spec: {:?} {} band {}", kind, state.id, band)
                            });
                            assert_eq!(
                                spec,
                                (intent.damage, intent.hits),
                                "{:?} {} band {}",
                                kind,
                                state.id,
                                band
                            );
                            checked += 1;
                        }
                        None => assert!(
                            attack_spec(kind, state.id, band).is_none(),
                            "unexpected attack spec: {:?} {} band {}",
                            kind,
                            state.id,
                            band
                        ),
                    }
                }
            }
        }
        eprintln!("act2 attack audit: {checked} A0/A10 checks, 0 mismatches");
        assert!(checked > 0);
    }
}
