//! r7 `monsters/act4.py` + `encounters/act4.py` (Underdocks, alternative act 1).

use super::{DEADLY_ASC, TOUGH_ASC, asc_value, state_index};
use crate::card::CId;
use crate::combat::{Combat, PileKind, PilePos};
use crate::monster::MonsterAi;
use crate::rng::Rng;
use crate::tables::*;
use arrayvec::ArrayVec;

// Factory arguments encoded in `MonsterAi::flags`. Meanings are per monster.
const TOADPOLE_NOT_FRONT: u32 = 1;
const PUNCH_STRONG_START: u32 = 1;
const PUNCH_EVENT_FAST_START: u32 = 1 << 31;
const PUNCH_REDUCTION_SHIFT: u32 = 1;
const PHANTASMAL_SLOT_MASK: u32 = 3;

// Two-tailed rat packed state: starter code [0..1], signed turns [2..9], call count [10..13].
// The high marker distinguishes the factory default (`turns=2`) from a raw zero flag.
const RAT_INIT: u32 = 1 << 31;
const RAT_START_MASK: u32 = 0x3;
const RAT_TURNS_SHIFT: u32 = 2;
const RAT_TURNS_MASK: u32 = 0xff << RAT_TURNS_SHIFT;
const RAT_COUNT_SHIFT: u32 = 10;
const RAT_COUNT_MASK: u32 = 0xf << RAT_COUNT_SHIFT;

// Waterfall Giant local closure state: current pressure-gun damage [0..7], saved eruption [8..23].
const WATER_GUN_MASK: u32 = 0xff;
const WATER_ERUPTION_SHIFT: u32 = 8;
const WATER_ERUPTION_MASK: u32 = 0xffff << WATER_ERUPTION_SHIFT;

impl Combat {
    /// C# WaterfallGiant's live CurrentPressureGunDamage intent delegate.
    pub(crate) fn waterfall_pressure_gun_damage(&self, enemy: CId) -> i32 {
        water_gun(self.cr(enemy).ai.as_ref().unwrap().flags, self.run.ascension)
    }

    pub(crate) fn waterfall_eruption_damage(&self, enemy: CId) -> i32 {
        water_saved(self.cr(enemy).ai.as_ref().unwrap().flags)
    }
}

/// `CardPileCmd.AddToCombatAndPreview<id>(targets, Discard, count)` for every living player.
fn discard_status(c: &mut Combat, id: CardId, count: i32) {
    for p in c.living_players() {
        for _ in 0..count {
            c.create_card_in_pile(p, id, false, PileKind::Discard, PilePos::Bottom);
        }
    }
}

#[inline]
fn punch_reduction(flags: u32) -> i32 {
    ((flags & !PUNCH_EVENT_FAST_START) >> PUNCH_REDUCTION_SHIFT) as i32
}

#[inline]
pub(crate) fn punch_construct_flags(
    starts_with_strong_punch: bool,
    starting_hp_reduction: i32,
) -> u32 {
    u32::from(starts_with_strong_punch)
        | ((starting_hp_reduction.max(0) as u32) << PUNCH_REDUCTION_SHIFT)
}

/// r7 `create_punch_construct` factory wrapper, including its post-HP-roll current-HP reduction.
pub(crate) fn add_punch_construct(
    c: &mut Combat,
    starts_with_strong_punch: bool,
    starting_hp_reduction: i32,
) -> CId {
    add(
        c,
        MonsterKind::PunchConstruct,
        punch_construct_flags(starts_with_strong_punch, starting_hp_reduction),
    )
}

#[inline]
fn add(c: &mut Combat, kind: MonsterKind, flags: u32) -> CId {
    let (cr, ai) = c.create_monster(kind, flags);
    let e = c.add_enemy(cr, ai);
    if kind == MonsterKind::PunchConstruct {
        let reduction = punch_reduction(flags);
        if reduction > 0 {
            let hp = c.cr(e).hp;
            c.cr_mut(e).hp = (hp - reduction).max(1);
        }
    }
    e
}

#[inline]
fn rat_factory_flags(starter_move_idx: i32) -> u32 {
    let starter = match starter_move_idx {
        0 => 1,
        1 => 2,
        2 => 3,
        _ => 0,
    };
    RAT_INIT | starter | (2 << RAT_TURNS_SHIFT)
}

#[inline]
fn rat_turns(flags: u32) -> i32 {
    if flags & RAT_INIT == 0 {
        2
    } else {
        (((flags & RAT_TURNS_MASK) >> RAT_TURNS_SHIFT) as u8 as i8) as i32
    }
}

#[inline]
fn rat_count(flags: u32) -> i32 {
    if flags & RAT_INIT == 0 {
        0
    } else {
        ((flags & RAT_COUNT_MASK) >> RAT_COUNT_SHIFT) as i32
    }
}

#[inline]
fn rat_set_turns(flags: u32, turns: i32) -> u32 {
    (flags | RAT_INIT) & !RAT_TURNS_MASK | (((turns as i8 as u8) as u32) << RAT_TURNS_SHIFT)
}

#[inline]
fn rat_set_count(flags: u32, count: i32) -> u32 {
    (flags | RAT_INIT) & !RAT_COUNT_MASK | (((count as u32) & 0xf) << RAT_COUNT_SHIFT)
}

fn rat_can_summon(c: &Combat, e: CId, ai: &MonsterAi) -> bool {
    if rat_turns(ai.flags) > 0 || rat_count(ai.flags) >= 3 {
        return false;
    }
    let mut alive = 0;
    for &enemy in &c.enemies {
        let cr = c.cr(enemy);
        if cr.monster_id != "TWO_TAILED_RAT" || !cr.is_alive() {
            continue;
        }
        alive += 1;
        if enemy != e
            && cr
                .ai
                .as_ref()
                .is_some_and(|other| other.state_id(other.current) == "CALL_FOR_BACKUP_MOVE")
        {
            return false;
        }
    }
    alive < 5
}

fn rat_attack_performed(c: &mut Combat, e: CId) {
    let flags = c.cr(e).ai.as_ref().unwrap().flags;
    c.cr_mut(e).ai.as_mut().unwrap().flags = rat_set_turns(flags, rat_turns(flags) - 1);
}

fn rat_call_for_backup(c: &mut Combat, e: CId) {
    let can_summon = {
        let ai = c.cr(e).ai.as_ref().unwrap();
        rat_can_summon(c, e, ai)
    };
    if can_summon {
        // r7 act4.py:1512-1519: the backup takes the LAST free encounter slot.
        let (cr, ai) = c.create_monster(MonsterKind::TwoTailedRat, rat_factory_flags(-1));
        let slot = c.encounter_slots.iter().rev().copied()
            .find(|s| !c.enemies.iter().any(|&en| c.cr(en).slot == Some(*s)));
        c.add_enemy_in_slot(cr, ai, slot);
    }

    let rats: ArrayVec<CId, 8> = c
        .enemies
        .iter()
        .copied()
        .filter(|&enemy| c.cr(enemy).monster_id == "TWO_TAILED_RAT")
        .collect();
    let max_count = rats
        .iter()
        .map(|&enemy| c.cr(enemy).ai.as_ref().map_or(0, |a| rat_count(a.flags)) + 1)
        .max()
        .unwrap_or(1);
    for enemy in rats {
        if let Some(ai) = c.cr_mut(enemy).ai.as_mut() {
            ai.flags = rat_set_count(ai.flags, max_count);
        }
    }
}

#[inline]
fn water_gun(flags: u32, asc: i32) -> i32 {
    let stored = (flags & WATER_GUN_MASK) as i32;
    if stored == 0 {
        asc_value(asc, DEADLY_ASC, 23, 20)
    } else {
        stored
    }
}

#[inline]
fn water_set_gun(flags: u32, damage: i32) -> u32 {
    (flags & !WATER_GUN_MASK) | ((damage as u32) & WATER_GUN_MASK)
}

#[inline]
fn water_saved(flags: u32) -> i32 {
    ((flags & WATER_ERUPTION_MASK) >> WATER_ERUPTION_SHIFT) as i32
}

#[inline]
fn water_set_saved(flags: u32, damage: i32) -> u32 {
    (flags & !WATER_ERUPTION_MASK) | (((damage as u32) & 0xffff) << WATER_ERUPTION_SHIFT)
}

/// Initial state chosen by factory arguments; `None` = the table default (or not ours).
pub(crate) fn initial_state(kind: MonsterKind, flags: u32, def: &MonsterDef) -> Option<u8> {
    match kind {
        MonsterKind::CorpseSlug => Some(state_index(
            def,
            match flags % 3 {
                0 => "WHIP_SLAP_MOVE",
                1 => "GLOMP_MOVE",
                _ => "GOOP_MOVE",
            },
        )),
        MonsterKind::Toadpole => Some(state_index(
            def,
            if flags & TOADPOLE_NOT_FRONT == 0 {
                "SPIKEN_MOVE"
            } else {
                "WHIRL_MOVE"
            },
        )),
        MonsterKind::PunchConstruct => Some(state_index(
            def,
            if flags & PUNCH_EVENT_FAST_START != 0 {
                "FAST_PUNCH_MOVE"
            } else if flags & PUNCH_STRONG_START != 0 {
                "STRONG_PUNCH_MOVE"
            } else {
                "READY_MOVE"
            },
        )),
        MonsterKind::TwoTailedRat => Some(state_index(
            def,
            match flags & RAT_START_MASK {
                1 => "SCRATCH_MOVE",
                2 => "DISEASE_BITE_MOVE",
                3 => "SCREECH_MOVE",
                _ => "RAND",
            },
        )),
        MonsterKind::PhantasmalGardener => Some(state_index(
            def,
            match flags & PHANTASMAL_SLOT_MASK {
                0 => "FLAIL_MOVE",
                1 => "BITE_MOVE",
                2 => "LASH_MOVE",
                _ => "ENLARGE_MOVE",
            },
        )),
        _ => None,
    }
}

/// Callable branch weight; `None` = not ours.
pub(crate) fn weight(c: &Combat, e: CId, ai: &MonsterAi, target: u8) -> Option<f64> {
    if ai.kind != MonsterKind::TwoTailedRat {
        return None;
    }
    let can_summon = rat_can_summon(c, e, ai);
    Some(if ai.state_id(target) == "CALL_FOR_BACKUP_MOVE" {
        if can_summon { 0.75 } else { 0.0 }
    } else if can_summon {
        1.0 / 12.0
    } else {
        1.0
    })
}

/// Conditional branch: r7 predicate order, returning the selected target state.
pub(crate) fn condition(c: &Combat, e: CId, ai: &MonsterAi, targets: &'static [u8]) -> Option<u8> {
    match ai.kind {
        // act4.py:462-464: not is_front, True.
        MonsterKind::Toadpole => Some(if ai.flags & TOADPOLE_NOT_FRONT != 0 {
            targets[0]
        } else {
            targets[1]
        }),
        // act4.py:1678-1682: first, second, third, fourth.
        MonsterKind::PhantasmalGardener => {
            Some(targets[(ai.flags & PHANTASMAL_SLOT_MASK) as usize])
        }
        // act4.py:2401-2403: Asleep, True.
        MonsterKind::LagavulinMatriarch => Some(if c.has_power(e, PowerId::Asleep) {
            targets[0]
        } else {
            targets[1]
        }),
        _ => None,
    }
}

pub(crate) fn move_effect(c: &mut Combat, e: CId, kind: MonsterKind, state: &'static str) -> bool {
    let asc = c.run.ascension;
    match (kind, state) {
        (MonsterKind::CorpseSlug, "WHIP_SLAP_MOVE") => c.monster_attack_players(e, 3, 2),
        (MonsterKind::CorpseSlug, "GLOMP_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 9, 8), 1)
        }
        (MonsterKind::CorpseSlug, "GOOP_MOVE") => c.monster_power_to_players(e, PowerId::Frail, 2),

        (MonsterKind::Seapunk, "SEA_KICK_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 13, 11), 1)
        }
        (MonsterKind::Seapunk, "SPINNING_KICK_MOVE") => c.monster_attack_players(e, 2, 4),
        (MonsterKind::Seapunk, "BUBBLE_BURP_MOVE") => {
            c.gain_move_block(e, asc_value(asc, TOUGH_ASC, 8, 7));
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 2, 1),
                Some(e),
            );
        }

        (MonsterKind::SludgeSpinner, "OIL_SPRAY_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 9, 8), 1);
            c.monster_power_to_players(e, PowerId::Weak, 1);
        }
        (MonsterKind::SludgeSpinner, "SLAM_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 12, 11), 1)
        }
        (MonsterKind::SludgeSpinner, "RAGE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 7, 6), 1);
            c.apply_power_to(e, PowerId::Strength, 3, Some(e));
        }

        (MonsterKind::Toadpole, "SPIKE_SPIT_MOVE") => {
            if c.has_power(e, PowerId::Thorns) {
                c.apply_power_to(e, PowerId::Thorns, -2, Some(e));
            }
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 4, 3), 3);
        }
        (MonsterKind::Toadpole, "WHIRL_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 8, 7), 1)
        }
        (MonsterKind::Toadpole, "SPIKEN_MOVE") => {
            c.apply_power_to(e, PowerId::Thorns, 2, Some(e));
        }

        (MonsterKind::CalcifiedCultist, "INCANTATION_MOVE") => {
            c.apply_power_to(e, PowerId::Ritual, 2, Some(e));
        }
        (MonsterKind::CalcifiedCultist, "DARK_STRIKE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 11, 9), 1)
        }
        (MonsterKind::DampCultist, "INCANTATION_MOVE") => {
            c.apply_power_to(
                e,
                PowerId::Ritual,
                asc_value(asc, DEADLY_ASC, 6, 5),
                Some(e),
            );
        }
        (MonsterKind::DampCultist, "DARK_STRIKE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 3, 1), 1)
        }

        (MonsterKind::FossilStalker, "TACKLE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 11, 9), 1);
            c.monster_power_to_players(e, PowerId::Frail, 1);
        }
        (MonsterKind::FossilStalker, "LATCH_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 14, 12), 1)
        }
        (MonsterKind::FossilStalker, "LASH_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 4, 3), 2)
        }

        (MonsterKind::SneakyGremlin, "SPAWNED_MOVE") => {}
        (MonsterKind::SneakyGremlin, "TACKLE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 10, 9), 1)
        }
        (MonsterKind::FatGremlin, "SPAWNED_MOVE") => {}
        (MonsterKind::FatGremlin, "FLEE_MOVE") => {
            // C# FatGremlin.FleeMove: escape, without changing any player's gold.
            c.escape_monster(e);
        }

        (MonsterKind::GremlinMerc, "GIMME_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, TOUGH_ASC, 8, 7), 2);
            c.gremlin_merc_steal(e);
        }
        (MonsterKind::GremlinMerc, "DOUBLE_SMASH_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, TOUGH_ASC, 7, 6), 2);
            c.gremlin_merc_steal(e);
            c.monster_power_to_players(e, PowerId::Weak, 2);
        }
        (MonsterKind::GremlinMerc, "HEHE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, TOUGH_ASC, 9, 8), 1);
            c.gremlin_merc_steal(e);
            c.apply_power_to(e, PowerId::Strength, 2, Some(e));
        }

        (MonsterKind::HauntedShip, "SWIPE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 14, 13), 1)
        }
        (MonsterKind::HauntedShip, "STOMP_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 5, 4), 3)
        }
        (MonsterKind::HauntedShip, "HAUNT_MOVE") => {
            c.monster_power_to_players(e, PowerId::Weak, 3);
            discard_status(c, CardId::Dazed, 5);
        }

        (MonsterKind::GasBomb, "EXPLODE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 9, 8), 1);
            c.kill_creature(e);
        }
        (MonsterKind::LivingFog, "ADVANCED_GAS_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 9, 8), 1);
            c.monster_power_to_players(e, PowerId::Smoggy, 1);
        }
        (MonsterKind::LivingFog, "BLOAT_MOVE") => {
            // r7 initializes bloat_amount=1 and never mutates it (MAX_BLOAT_AMOUNT is unused).
            add(c, MonsterKind::GasBomb, 0);
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 6, 5), 1);
        }
        (MonsterKind::LivingFog, "SUPER_GAS_BLAST_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 9, 8), 1)
        }

        (MonsterKind::PunchConstruct, "READY_MOVE") => c.gain_move_block(e, 10),
        (MonsterKind::PunchConstruct, "STRONG_PUNCH_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 16, 14), 1)
        }
        (MonsterKind::PunchConstruct, "FAST_PUNCH_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 6, 5), 2);
            c.monster_power_to_players(e, PowerId::Frail, 1);
        }

        (MonsterKind::SewerClam, "PRESSURIZE_MOVE") => {
            c.apply_power_to(e, PowerId::Strength, 4, Some(e));
        }
        (MonsterKind::SewerClam, "JET_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 11, 10), 1)
        }

        (MonsterKind::TwoTailedRat, "SCRATCH_MOVE") => {
            rat_attack_performed(c, e);
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 9, 8), 1);
        }
        (MonsterKind::TwoTailedRat, "DISEASE_BITE_MOVE") => {
            rat_attack_performed(c, e);
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 7, 6), 1);
        }
        (MonsterKind::TwoTailedRat, "SCREECH_MOVE") => {
            rat_attack_performed(c, e);
            c.monster_power_to_players(e, PowerId::Frail, 1);
        }
        (MonsterKind::TwoTailedRat, "CALL_FOR_BACKUP_MOVE") => rat_call_for_backup(c, e),

        (MonsterKind::PhantasmalGardener, "BITE_MOVE") => c.monster_attack_players(e, 5, 1),
        (MonsterKind::PhantasmalGardener, "LASH_MOVE") => c.monster_attack_players(e, 7, 1),
        (MonsterKind::PhantasmalGardener, "FLAIL_MOVE") => c.monster_attack_players(e, 1, 3),
        (MonsterKind::PhantasmalGardener, "ENLARGE_MOVE") => {
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 3, 2),
                Some(e),
            );
        }

        (MonsterKind::SkulkingColony, "INERTIA_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 11, 9), 1);
            if !c.is_over {
                c.apply_power_to(
                    e,
                    PowerId::Strength,
                    asc_value(asc, DEADLY_ASC, 4, 2),
                    Some(e),
                );
            }
        }
        (MonsterKind::SkulkingColony, "ZOOM_MOVE" | "ZOOM_MOVE_2") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 16, 14), 1)
        }
        (MonsterKind::SkulkingColony, "PIERCING_STABS_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 8, 7), 2)
        }

        (MonsterKind::TerrorEel, "CRASH_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 18, 16), 1)
        }
        (MonsterKind::TerrorEel, "THRASH_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 4, 3), 3);
            c.apply_power_to(e, PowerId::Vigor, 6, Some(e));
        }
        (MonsterKind::TerrorEel, "STUN_MOVE") => {}
        (MonsterKind::TerrorEel, "TERROR_MOVE") => c.monster_power_to_players(e, PowerId::Vulnerable, 99),

        (MonsterKind::WaterfallGiant, "PRESSURIZE_MOVE") => {
            c.apply_power_to(
                e,
                PowerId::SteamEruption,
                asc_value(asc, DEADLY_ASC, 20, 15),
                Some(e),
            );
        }
        (MonsterKind::WaterfallGiant, "STOMP_MOVE") => {
            c.monster_stomp_players(e, asc_value(asc, DEADLY_ASC, 16, 15));
        }
        (MonsterKind::WaterfallGiant, "RAM_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 11, 10), 1);
            c.apply_power_to(e, PowerId::SteamEruption, 3, Some(e));
        }
        (MonsterKind::WaterfallGiant, "SIPHON_MOVE") => {
            // WaterfallGiant.SiphonMove: SiphonHeal x CombatState.Players.Count.
            let players = c.players.len() as i32;
            c.heal(e, asc_value(asc, TOUGH_ASC, 15, 10) * players);
            c.apply_power_to(e, PowerId::SteamEruption, 3, Some(e));
        }
        (MonsterKind::WaterfallGiant, "PRESSURE_GUN_MOVE") => {
            let flags = c.cr(e).ai.as_ref().unwrap().flags;
            let damage = c.waterfall_pressure_gun_damage(e);
            c.monster_attack_players(e, damage, 1);
            c.cr_mut(e).ai.as_mut().unwrap().flags = water_set_gun(flags, damage + 5);
            c.apply_power_to(e, PowerId::SteamEruption, 3, Some(e));
        }
        (MonsterKind::WaterfallGiant, "PRESSURE_UP_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 14, 13), 1);
            c.apply_power_to(e, PowerId::SteamEruption, 3, Some(e));
        }
        (MonsterKind::WaterfallGiant, "ABOUT_TO_BLOW_MOVE") => {
            let amount = c.power_amount(e, PowerId::SteamEruption);
            let flags = c.cr(e).ai.as_ref().unwrap().flags;
            c.cr_mut(e).ai.as_mut().unwrap().flags = water_set_saved(flags, amount);
            // r7 directly pops the power dict entry here (no on_removed callback).
            if let Some(p) = c.find_power(e, PowerId::SteamEruption)
                && let Some(pos) = c.cr(e).powers.iter().position(|&q| q == p)
            {
                c.cr_mut(e).powers.remove(pos);
            }
        }
        (MonsterKind::WaterfallGiant, "EXPLODE_MOVE") => {
            let damage = c.waterfall_eruption_damage(e);
            c.monster_attack_players(e, damage, 1);
            c.kill_creature(e);
        }

        (MonsterKind::SoulFysh, "BECKON_MOVE") => {
            // r7 act4.py:2190-2194. Random draw-pile insertion fail-closes in the current engine.
            for p in c.living_players() {
                c.create_card_in_pile(p, CardId::Beckon, false, PileKind::Draw, PilePos::Random);
                c.create_card_in_pile(p, CardId::Beckon, false, PileKind::Discard, PilePos::Bottom);
            }
        }
        (MonsterKind::SoulFysh, "DE_GAS_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 18, 16), 1)
        }
        (MonsterKind::SoulFysh, "GAZE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 8, 7), 1);
            if !c.is_over {
                discard_status(c, CardId::Beckon, 1);
            }
        }
        (MonsterKind::SoulFysh, "FADE_MOVE") => {
            c.apply_power_to(e, PowerId::Intangible, 2, Some(e));
        }
        (MonsterKind::SoulFysh, "SCREAM_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 15, 13), 1);
            c.monster_power_to_players(e, PowerId::Vulnerable, 3);
        }

        (MonsterKind::LagavulinMatriarch, "SLEEP_MOVE") => {}
        (MonsterKind::LagavulinMatriarch, "SLASH_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 21, 19), 1)
        }
        (MonsterKind::LagavulinMatriarch, "DISEMBOWEL_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 10, 9), 2)
        }
        (MonsterKind::LagavulinMatriarch, "SLASH2_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 14, 12), 1);
            c.gain_move_block(e, asc_value(asc, TOUGH_ASC, 14, 12));
        }
        (MonsterKind::LagavulinMatriarch, "SOUL_SIPHON_MOVE") => {
            c.monster_power_to_players(e, PowerId::Strength, -2);
            c.monster_power_to_players(e, PowerId::Dexterity, -2);
            c.apply_power_to(e, PowerId::Strength, 2, Some(e));
        }
        _ => return false,
    }
    true
}

pub(crate) fn setup_encounter(c: &mut Combat, encounter_id: crate::identity::EncounterId, rng: &mut Rng) -> bool {
    use crate::identity::EncounterId as E;
    match encounter_id {
        // Weak.
        E::CorpseSlugsWeak => {
            let starter = rng.next_int(0, 2) as u32;
            add(c, MonsterKind::CorpseSlug, starter);
            add(c, MonsterKind::CorpseSlug, (starter + 1) % 3);
        }
        E::SeapunkWeak => {
            add(c, MonsterKind::Seapunk, 0);
        }
        E::SludgeSpinnerWeak => {
            add(c, MonsterKind::SludgeSpinner, 0);
        }
        E::ToadpolesWeak => {
            add(c, MonsterKind::Toadpole, 0);
            add(c, MonsterKind::Toadpole, TOADPOLE_NOT_FRONT);
        }

        // Normal.
        E::CorpseSlugsNormal => {
            let starter = rng.next_int(0, 2) as u32;
            for offset in 0..3 {
                add(c, MonsterKind::CorpseSlug, (starter + offset) % 3);
            }
        }
        E::CultistsNormal => {
            add(c, MonsterKind::CalcifiedCultist, 0);
            add(c, MonsterKind::DampCultist, 0);
        }
        E::FossilStalkerNormal => {
            add(c, MonsterKind::FossilStalker, 0);
        }
        E::GremlinMercNormal => {
            add(c, MonsterKind::GremlinMerc, 0);
        }
        E::HauntedShipNormal => {
            add(c, MonsterKind::HauntedShip, 0);
        }
        E::LivingFogNormal => {
            // r7 encounters/act4.py:106-109
            c.configure_encounter_slots(&["bomb1", "bomb2", "bomb3", "bomb4", "bomb5", "livingFog"]);
            let (cr, ai) = c.create_monster(MonsterKind::LivingFog, 0);
            c.add_enemy_in_slot(cr, ai, Some("livingFog"));
        }
        E::PunchConstructNormal => {
            add_punch_construct(c, false, 0);
        }
        E::PunchOffEventEncounter => {
            // C# PunchOffEventEncounter.GenerateMonsters: [2,10) HP reductions,
            // first starts FAST, second READY. Reductions affect current HP only.
            let first = rng.next_int_exclusive(2, 10) as i32;
            let second = rng.next_int_exclusive(2, 10) as i32;
            add(c, MonsterKind::PunchConstruct,
                punch_construct_flags(false, first) | PUNCH_EVENT_FAST_START);
            add_punch_construct(c, false, second);
        }
        E::SeapunkNormal => {
            add(c, MonsterKind::CalcifiedCultist, 0);
            add(c, MonsterKind::Seapunk, 0);
        }
        E::SewerClamNormal => {
            add(c, MonsterKind::SewerClam, 0);
        }
        // Kept for direct callers in r7 even though not present in ALL_ACT4_ENCOUNTERS.
        E::ToadpolesNormal => {
            add(c, MonsterKind::CalcifiedCultist, 0);
            add(c, MonsterKind::Toadpole, TOADPOLE_NOT_FRONT);
        }
        E::TwoTailedRatsNormal => {
            // r7 encounters/act4.py:137-146
            c.configure_encounter_slots(&["first", "second", "third", "fourth", "fifth"]);
            let starter = rng.next_int(0, 2) as i32;
            for (offset, slot) in ["third", "fourth", "fifth"].into_iter().enumerate() {
                let (cr, ai) =
                    c.create_monster(MonsterKind::TwoTailedRat, rat_factory_flags((starter + offset as i32) % 3));
                c.add_enemy_in_slot(cr, ai, Some(slot));
            }
        }

        // Elites.
        E::PhantasmalGardenersElite => {
            for slot in 0..4 {
                add(c, MonsterKind::PhantasmalGardener, slot);
            }
        }
        E::SkulkingColonyElite => {
            add(c, MonsterKind::SkulkingColony, 0);
        }
        E::TerrorEelElite => {
            add(c, MonsterKind::TerrorEel, 0);
        }

        // Bosses.
        E::WaterfallGiantBoss => {
            let initial_gun = asc_value(c.run.ascension, DEADLY_ASC, 23, 20) as u32;
            add(c, MonsterKind::WaterfallGiant, initial_gun);
        }
        E::SoulFyshBoss => {
            add(c, MonsterKind::SoulFysh, 0);
        }
        E::LagavulinMatriarchBoss => {
            add(c, MonsterKind::LagavulinMatriarch, 0);
        }
        _ => return false,
    }
    true
}
