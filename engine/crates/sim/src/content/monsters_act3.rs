//! r7 `monsters/act3.py`, the Act 3 encounters, and Queen's shared Torch Head Amalgam.

use super::{DEADLY_ASC, TOUGH_ASC, asc_value, state_index};
use crate::card::{CId, KRef};
use crate::combat::{Combat, PileKind, PilePos};
use crate::hooks::Ev;
use crate::monster::MonsterAi;
use crate::rng::Rng;
use crate::tables::*;
use arrayvec::ArrayVec;

const SCROLL_START_MASK: u32 = 0x3;

const AXEBOT_BOOT_FIRST: u32 = 1;
const AXEBOT_STOCK_EXPLICIT: u32 = 1 << 1;
const AXEBOT_STOCK_SHIFT: u32 = 8;
const AXEBOT_STOCK_MASK: u32 = 0xff << AXEBOT_STOCK_SHIFT;

const AEON_WITHER_MASK: u32 = 0xff;
const AEON_STRENGTH_SHIFT: u32 = 8;
const AEON_STRENGTH_MASK: u32 = 0xff << AEON_STRENGTH_SHIFT;

const FABRICATOR_LAST_MASK: u32 = 0x7;
const FROG_BEETLE_CHARGED: u32 = 1;

const TEST_RESPAWNS_MASK: u32 = 0x3;
const TEST_CLAW_SHIFT: u32 = 2;
const TEST_CLAW_MASK: u32 = 0xff << TEST_CLAW_SHIFT;


/// `CardPileCmd.AddToCombatAndPreview<id>(targets, pile, count)` for every living player.
fn add_status(c: &mut Combat, id: CardId, count: i32, pile: PileKind, pos: PilePos) {
    for p in c.living_players() {
        for _ in 0..count {
            c.create_card_in_pile(p, id, false, pile, pos);
        }
    }
}

fn raw_pop_power(c: &mut Combat, owner: CId, id: PowerId) {
    let pos = {
        let cr = c.cr(owner);
        cr.powers
            .iter()
            .position(|&p| c.powers[p as usize].id == id)
    };
    if let Some(pos) = pos {
        c.cr_mut(owner).powers.remove(pos);
    }
}

#[inline]
fn ai_flags(c: &Combat, e: CId) -> u32 {
    c.cr(e).ai.as_ref().expect("monster without AI").flags
}

#[inline]
fn set_ai_flags(c: &mut Combat, e: CId, flags: u32) {
    c.cr_mut(e).ai.as_mut().expect("monster without AI").flags = flags;
}

#[inline]
fn axebot_stock(flags: u32) -> i32 {
    if flags & AXEBOT_STOCK_EXPLICIT == 0 {
        2
    } else {
        (((flags & AXEBOT_STOCK_MASK) >> AXEBOT_STOCK_SHIFT) as u8 as i8) as i32
    }
}

#[allow(dead_code)]
pub(crate) fn axebot_flags(start_with_boot_up: bool, stock_amount: Option<i32>) -> u32 {
    let mut flags = u32::from(start_with_boot_up) * AXEBOT_BOOT_FIRST;
    if let Some(stock) = stock_amount {
        flags |= AXEBOT_STOCK_EXPLICIT;
        flags |= ((stock as i8 as u8) as u32) << AXEBOT_STOCK_SHIFT;
    }
    flags
}

pub(crate) fn spawn_stock_axebot(c: &mut Combat, stock: i32) -> CId {
    // powers/remaining_c.py:1057-1069 -> create_axebot(stock_amount=...).
    add(c, MonsterKind::Axebot, axebot_flags(false, Some(stock)))
}

pub(crate) fn spawn_doormaker(c: &mut Combat) -> CId {
    // core/combat.py:3904-3918. Reuse a living Doormaker; otherwise create/add one.
    if let Some(existing) = c
        .enemies
        .iter()
        .copied()
        .find(|&e| c.cr(e).monster_id == "DOORMAKER" && c.cr(e).is_alive())
    {
        return existing;
    }
    add(c, MonsterKind::Doormaker, 0)
}

#[inline]
fn aeon_wither(flags: u32) -> i32 {
    (flags & AEON_WITHER_MASK) as i32
}

#[inline]
fn aeon_strength(flags: u32) -> i32 {
    ((flags & AEON_STRENGTH_MASK) >> AEON_STRENGTH_SHIFT) as i32
}

#[inline]
fn aeon_set(flags: u32, wither: i32, strength: i32) -> u32 {
    (flags & !(AEON_WITHER_MASK | AEON_STRENGTH_MASK))
        | ((wither as u32) & AEON_WITHER_MASK)
        | (((strength as u32) & 0xff) << AEON_STRENGTH_SHIFT)
}

#[inline]
fn test_respawns(flags: u32) -> i32 {
    (flags & TEST_RESPAWNS_MASK) as i32
}

#[inline]
fn test_claw_extra(flags: u32) -> i32 {
    ((flags & TEST_CLAW_MASK) >> TEST_CLAW_SHIFT) as i32
}

#[inline]
fn test_set_respawns(flags: u32, value: i32) -> u32 {
    (flags & !TEST_RESPAWNS_MASK) | ((value as u32) & TEST_RESPAWNS_MASK)
}

#[inline]
fn test_set_claw_extra(flags: u32, value: i32) -> u32 {
    (flags & !TEST_CLAW_MASK) | (((value as u32) & 0xff) << TEST_CLAW_SHIFT)
}

#[inline]
fn source_attack_spec(kind: MonsterKind, state: &str, asc: i32) -> Option<(i32, i32)> {
    let deadly = |high, base| asc_value(asc, DEADLY_ASC, high, base);
    Some(match (kind, state) {
        (MonsterKind::Aeonglass, "EBB_MOVE") => (deadly(26, 22), 1),
        (MonsterKind::Aeonglass, "EYE_LASERS_MOVE") => (deadly(12, 11), 2),
        (MonsterKind::DevotedSculptor, "SAVAGE_MOVE") => (deadly(15, 12), 1),
        (MonsterKind::ScrollOfBiting, "CHOMP") => (deadly(16, 14), 1),
        (MonsterKind::ScrollOfBiting, "CHEW") => (deadly(6, 5), 2),
        (MonsterKind::TurretOperator, "UNLOAD_MOVE" | "UNLOAD_MOVE_2") => (deadly(4, 3), 5),
        (MonsterKind::Axebot, "ONE_TWO_MOVE") => (deadly(11, 10), 2),
        (MonsterKind::Axebot, "HAMMER_UPPERCUT_MOVE") => (deadly(18, 14), 1),
        (MonsterKind::Zapbot, "ZAP") => (deadly(15, 14), 1),
        (MonsterKind::Stabbot, "STAB_MOVE") => (deadly(12, 11), 1),
        (MonsterKind::Fabricator, "FABRICATING_STRIKE_MOVE") => (deadly(21, 18), 1),
        (MonsterKind::Fabricator, "DISINTEGRATE_MOVE") => (deadly(13, 11), 1),
        (MonsterKind::FrogKnight, "TONGUE_LASH") => (deadly(14, 13), 1),
        (MonsterKind::FrogKnight, "STRIKE_DOWN_EVIL") => (deadly(23, 21), 1),
        (MonsterKind::FrogKnight, "BEETLE_CHARGE") => (deadly(40, 35), 1),
        (MonsterKind::GlobeHead, "SHOCKING_SLAP") => (deadly(14, 13), 1),
        (MonsterKind::GlobeHead, "THUNDER_STRIKE") => (deadly(7, 6), 3),
        (MonsterKind::GlobeHead, "GALVANIC_BURST") => (deadly(17, 16), 1),
        (MonsterKind::OwlMagistrate, "MAGISTRATE_SCRUTINY") => (deadly(17, 16), 1),
        (MonsterKind::OwlMagistrate, "PECK_ASSAULT") => (4, 6),
        (MonsterKind::OwlMagistrate, "VERDICT") => (deadly(36, 33), 1),
        (MonsterKind::SlimedBerserker, "FURIOUS_PUMMELING_MOVE") => (deadly(5, 4), 4),
        (MonsterKind::SlimedBerserker, "SMOTHER_MOVE") => (deadly(33, 30), 1),
        (MonsterKind::TheLost, "EYE_LASERS") => (deadly(5, 4), 2),
        (MonsterKind::TheForgotten, "DREAD") => (deadly(15, 13), 1),
        (MonsterKind::FlailKnight, "FLAIL_MOVE") => (deadly(10, 9), 2),
        (MonsterKind::FlailKnight, "RAM_MOVE") => (deadly(17, 15), 1),
        (MonsterKind::MysteriousKnight, "FLAIL_MOVE") => (9, 2),
        (MonsterKind::MysteriousKnight, "RAM_MOVE") => (15, 1),
        (MonsterKind::LivingShield, "SHIELD_SLAM_MOVE") => (6, 1),
        (MonsterKind::LivingShield, "SMASH_MOVE") => (deadly(18, 16), 1),
        (MonsterKind::MagiKnight, "POWER_SHIELD_MOVE") => (deadly(7, 6), 1),
        (MonsterKind::MagiKnight, "RAM_MOVE") => (deadly(11, 10), 1),
        (MonsterKind::MagiKnight, "MAGIC_BOMB") => (deadly(40, 35), 1),
        (MonsterKind::SpectralKnight, "SOUL_SLASH") => (deadly(17, 15), 1),
        (MonsterKind::SpectralKnight, "SOUL_FLAME") => (deadly(4, 3), 3),
        (MonsterKind::MechaKnight, "CHARGE_MOVE") => (deadly(30, 25), 1),
        (MonsterKind::MechaKnight, "FLAMETHROWER_MOVE") => (deadly(12, 8), 1),
        (MonsterKind::MechaKnight, "HEAVY_CLEAVE_MOVE") => (deadly(40, 35), 1),
        (MonsterKind::SoulNexus, "SOUL_BURN_MOVE") => (deadly(31, 29), 1),
        (MonsterKind::SoulNexus, "MAELSTROM_MOVE") => (deadly(7, 6), 4),
        (MonsterKind::SoulNexus, "DRAIN_LIFE_MOVE") => (deadly(19, 18), 1),
        (MonsterKind::Door, "DRAMATIC_OPEN_MOVE") => (deadly(28, 25), 1),
        (MonsterKind::Door, "DOOR_SLAM_MOVE") => (15, 2),
        (MonsterKind::Door, "ENFORCE_MOVE") => (20, 1),
        (MonsterKind::Doormaker, "BEAM_MOVE") => (deadly(34, 31), 1),
        (MonsterKind::Doormaker, "GET_BACK_IN_MOVE") => (deadly(45, 40), 1),
        (MonsterKind::RoyalGuard, "STRIKE") => (15, 1),
        (MonsterKind::Queen, "OFF_WITH_YOUR_HEAD_MOVE") => (deadly(4, 3), 5),
        (MonsterKind::Queen, "EXECUTION_MOVE") => (deadly(18, 15), 1),
        (MonsterKind::TestSubject, "BITE_MOVE") => (deadly(22, 20), 1),
        (MonsterKind::TestSubject, "SKULL_BASH_MOVE") => (deadly(16, 14), 1),
        (MonsterKind::TestSubject, "MULTI_CLAW_MOVE") => (deadly(11, 10), 3),
        (MonsterKind::TestSubject, "PHASE3_LACERATE_MOVE") => (deadly(11, 10), 3),
        (MonsterKind::TestSubject, "BIG_POUNCE") => (45, 1),
        (MonsterKind::TorchHeadAmalgam, "STRONG_TACKLE_MOVE") => (deadly(32, 26), 1),
        (MonsterKind::TorchHeadAmalgam, "TACKLE_2_MOVE") => (deadly(22, 18), 1),
        (MonsterKind::TorchHeadAmalgam, "BEAM_MOVE") => (8, 3),
        (MonsterKind::TorchHeadAmalgam, "TACKLE_3_MOVE" | "TACKLE_4_MOVE") => (deadly(16, 14), 1),
        _ => return None,
    })
}

#[inline]
fn attack(c: &mut Combat, e: CId, kind: MonsterKind, state: &str) {
    let (damage, hits) =
        source_attack_spec(kind, state, c.run.ascension).expect("missing Act 3 attack spec");
    c.monster_attack_players(e, damage, hits);
}

impl Combat {
    /// C# TheForgotten.DreadDamage is a live DamageCalc delegate, not the
    /// static table's initial damage. Execution and intent previews share it.
    pub(crate) fn forgotten_dread_damage(&self, enemy: CId) -> i32 {
        let (base, _) = source_attack_spec(MonsterKind::TheForgotten, "DREAD", self.run.ascension).unwrap();
        base + self.power_amount(enemy, PowerId::Dexterity)
    }

    /// C# TestSubject.MultiClawTotalCount; increases after each MultiClawMove.
    pub(crate) fn test_subject_claw_hits(&self, enemy: CId) -> i32 {
        let (_, base) = source_attack_spec(MonsterKind::TestSubject, "MULTI_CLAW_MOVE", self.run.ascension).unwrap();
        base + test_claw_extra(ai_flags(self, enemy))
    }
}

fn fake_upgrade_wither(c: &mut Combat, k: KRef) {
    if c.cards[k as usize].id != CardId::Wither {
        return;
    }
    let card = &mut c.cards[k as usize];
    for (key, value) in &mut card.vars {
        if *key == VarKey::FakeUpgradeLevel {
            *value += 1;
        } else if *key == VarKey::Damage {
            *value += 3;
        }
    }
    card.damage = Some(card.damage.unwrap_or(0) + 3);
}

/// C# Aeonglass IncreasingIntensity: every target player's Withers (`PlayerCombatState.AllCards`).
fn upgrade_all_wither(c: &mut Combat) {
    for p in c.living_players() {
        for k in c.pile_cards(p) {
            fake_upgrade_wither(c, k);
        }
    }
}

/// One Wither into every target player's discard pile; Aeonglass's generation hook gives it the
/// current WitherUpgradeCount fake upgrades.
fn create_wither(c: &mut Combat) {
    for p in c.living_players() {
        c.create_card_in_pile(p, CardId::Wither, false, PileKind::Discard, PilePos::Bottom);
    }
}

/// C# Aeonglass.AfterCardGeneratedForCombat -> MatchWitherToUpgradeCount: every Wither generated
/// in combat (Increasing Intensity, Withering Presence) gets WitherUpgradeCount fake upgrades.
pub(crate) fn aeonglass_after_card_generated(c: &mut Combat, k: KRef) {
    if c.cards[k as usize].id != CardId::Wither {
        return;
    }
    let upgrades: i32 = c
        .enemies
        .iter()
        .filter(|&&e| c.cr(e).monster_id == "AEONGLASS")
        .map(|&e| aeon_wither(ai_flags(c, e)))
        .sum();
    for _ in 0..upgrades {
        fake_upgrade_wither(c, k);
    }
}

fn add(c: &mut Combat, kind: MonsterKind, flags: u32) -> CId {
    let (mut cr, mut ai) = c.create_monster(kind, flags);

    if kind == MonsterKind::Axebot {
        let respawns = 2 - axebot_stock(flags);
        if respawns != 0 {
            let (lo, hi) = cr.hp_range.expect("axebot hp range");
            cr.hp_range = Some((lo + respawns * 10, hi + respawns * 10));
        }
    }

    let e = c.add_enemy(cr, ai);

    if kind == MonsterKind::Axebot && flags & AXEBOT_STOCK_EXPLICIT != 0 {
        let stock = axebot_stock(flags);
        if stock > 0 {
            if let Some(p) = c.find_power(e, PowerId::Stock) {
                c.powers[p as usize].amount = stock;
            }
        } else {
            raw_pop_power(c, e, PowerId::Stock);
        }
    }

    e
}

#[allow(dead_code)]
pub(crate) fn add_mysterious_knight(c: &mut Combat) -> CId {
    add(c, MonsterKind::MysteriousKnight, 0)
}

fn fabricator_spawn(c: &mut Combat, fabricator: CId, defense: bool) {
    let last = ai_flags(c, fabricator) & FABRICATOR_LAST_MASK;
    let pair = if defense {
        [(MonsterKind::Guardbot, 3u32), (MonsterKind::Noisebot, 4u32)]
    } else {
        [(MonsterKind::Zapbot, 1u32), (MonsterKind::Stabbot, 2u32)]
    };
    let mut options: ArrayVec<(MonsterKind, u32), 2> = ArrayVec::new();
    for option in pair {
        if option.1 != last {
            options.push(option);
        }
    }
    let chosen = options[c.run.rngs.monster_ai.choice_index(options.len())];
    let flags = (ai_flags(c, fabricator) & !FABRICATOR_LAST_MASK) | chosen.1;
    set_ai_flags(c, fabricator, flags);
    let bot = add(c, chosen.0, 0);
    c.apply_power_to(bot, PowerId::Minion, 1, Some(fabricator));
}

fn gain_unpowered_block(c: &mut Combat, target: CId, amount: i32) {
    if c.is_over {
        return;
    }
    let before = c.cr(target).block;
    c.gain_recorded_block(target,amount);
    let gained = c.cr(target).block - before;
    if gained > 0 {
        c.fire(
            Hook::AfterBlockGained,
            Ev::BlockGained {
                creature: target,
                amount: gained,
            },
            None,
        );
    }
}

fn revive_door(c: &mut Combat) {
    let Some(door) = c
        .enemies
        .iter()
        .copied()
        .find(|&x| c.cr(x).monster_id == "DOOR" && c.has_power(x, PowerId::DoorRevival))
    else {
        return;
    };
    // core/combat.py:3945-3954. These are private fields on DoorRevivalPower in r7.
    let returns = c.power_private_int(door, PowerId::DoorRevival, 2) + 1;
    c.set_power_private_int(door, PowerId::DoorRevival, 2, returns);
    let mut initial_max = c.power_private_int(door, PowerId::DoorRevival, 1);
    if initial_max <= 0 {
        initial_max = c.cr(door).max_hp;
        c.set_power_private_int(door, PowerId::DoorRevival, 1, initial_max);
    }
    c.set_power_private_int(door, PowerId::DoorRevival, 0, 0);
    let hp_scale = asc_value(c.run.ascension, TOUGH_ASC, 25, 20);
    let str_scale = asc_value(c.run.ascension, DEADLY_ASC, 4, 3);
    {
        let cr = c.cr_mut(door);
        cr.block = 0;
        cr.max_hp = initial_max;
        cr.hp = initial_max;
        cr.escaped = false;
        cr.death_processed = false;
        cr.in_combat = true;
        cr.max_hp = initial_max + hp_scale * returns;
        cr.hp = cr.max_hp;
        let ai = cr.ai.as_mut().unwrap();
        ai.current = state_index(ai.def(), "DRAMATIC_OPEN_MOVE");
    }
    let strength = str_scale * returns;
    if let Some(p) = c.find_power(door, PowerId::Strength) {
        c.powers[p as usize].amount = strength;
    } else {
        c.apply_power_to(door, PowerId::Strength, strength, Some(door));
    }
}

pub(crate) fn escape_creature(c: &mut Combat, e: CId) {
    if !c.cr(e).is_alive() {
        return;
    }
    let before=crate::between::PublicState::of(c.cr(e));
    // C# CombatState.CreatureEscaped, distinct from removal after death.
    c.escaped_enemies.push(e);
    {
        let cr = c.cr_mut(e);
        // C# CreatureCmd.Escape: RemoveAllPowersInternalExcept, without removal hooks.
        cr.powers.clear();
        cr.extra.clear();
        cr.escaped = true;
        cr.block = 0;
        cr.in_combat = false;
    }
    if let Some(pos) = c.enemies.iter().position(|&x| x == e) {
        c.enemies.remove(pos);
    }
    c.sort_enemies_by_slot();
    c.record_creature_lifecycle(e,"escape",before);
    c.check_combat_end();
}

pub(crate) fn initial_state(kind: MonsterKind, flags: u32, def: &MonsterDef) -> Option<u8> {
    match kind {
        MonsterKind::ScrollOfBiting => Some(state_index(
            def,
            match flags & SCROLL_START_MASK {
                0 => "CHOMP",
                1 => "CHEW",
                _ => "MORE_TEETH",
            },
        )),
        MonsterKind::Axebot => Some(state_index(
            def,
            if flags & (AXEBOT_BOOT_FIRST | AXEBOT_STOCK_EXPLICIT) != 0 {
                "BOOT_UP_MOVE"
            } else {
                "HAMMER_UPPERCUT_MOVE"
            },
        )),
        MonsterKind::MysteriousKnight => Some(state_index(def, "RAM_MOVE")),
        _ => None,
    }
}

pub(crate) fn weight(_c: &Combat, _e: CId, ai: &MonsterAi, target: u8) -> Option<f64> {
    if ai.kind != MonsterKind::MysteriousKnight {
        return None;
    }
    Some(match ai.state_id(target) {
        "FLAIL_MOVE" | "RAM_MOVE" => 2.0,
        "WAR_CHANT" => 1.0,
        _ => 0.0,
    })
}

pub(crate) fn condition(c: &Combat, e: CId, ai: &MonsterAi, targets: &'static [u8]) -> Option<u8> {
    match ai.kind {
        MonsterKind::Fabricator => {
            let side = c.cr(e).side;
            let alive_same_side = c
                .alive_enemies()
                .iter()
                .filter(|&&x| c.cr(x).side == side)
                .count();
            Some(if alive_same_side < 4 {
                targets[0]
            } else {
                targets[1]
            })
        }
        MonsterKind::FrogKnight => Some(
            if ai.flags & FROG_BEETLE_CHARGED == 0 && c.cr(e).hp < c.cr(e).max_hp / 2 {
                targets[0]
            } else {
                targets[1]
            },
        ),
        MonsterKind::LivingShield => Some(if c.alive_enemies().iter().any(|&x| x != e) {
            targets[0]
        } else {
            targets[1]
        }),
        MonsterKind::Queen => Some(
            if c.alive_enemies()
                .iter()
                .any(|&x| c.cr(x).monster_id == "TORCH_HEAD_AMALGAM")
            {
                targets[0]
            } else {
                targets[1]
            },
        ),
        MonsterKind::TestSubject => Some(if test_respawns(ai.flags) < 2 {
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
        (MonsterKind::Aeonglass, "EBB_MOVE") => {
            attack(c, e, kind, state);
            c.gain_move_block(e, 33);
        }
        (MonsterKind::Aeonglass, "EYE_LASERS_MOVE") => attack(c, e, kind, state),
        (MonsterKind::Aeonglass, "INCREASING_INTENSITY_MOVE") => {
            upgrade_all_wither(c);
            let flags = ai_flags(c, e);
            // IncreasingIntensityMove: WitherUpgradeCount++ before the new Withers are added.
            let wither_upgrades = aeon_wither(flags) + 1;
            let extra_strength = aeon_strength(flags);
            set_ai_flags(c, e, aeon_set(flags, wither_upgrades, extra_strength));
            for _ in 0..asc_value(asc, DEADLY_ASC, 2, 1) {
                create_wither(c);
            }
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 4, 3) + extra_strength,
                Some(e),
            );
            set_ai_flags(c, e, aeon_set(ai_flags(c, e), wither_upgrades, extra_strength + 1));
        }

        (MonsterKind::DevotedSculptor, "FORBIDDEN_INCANTATION_MOVE") => {
            c.apply_power_to(e, PowerId::Ritual, 9, Some(e));
        }
        (MonsterKind::DevotedSculptor, "SAVAGE_MOVE") => attack(c, e, kind, state),

        (MonsterKind::ScrollOfBiting, "CHOMP" | "CHEW") => attack(c, e, kind, state),
        (MonsterKind::ScrollOfBiting, "MORE_TEETH") => {
            c.apply_power_to(e, PowerId::Strength, 2, Some(e));
        }

        (MonsterKind::TurretOperator, "UNLOAD_MOVE" | "UNLOAD_MOVE_2") => attack(c, e, kind, state),
        (MonsterKind::TurretOperator, "RELOAD_MOVE") => {
            c.apply_power_to(e, PowerId::Strength, 1, Some(e));
        }

        (MonsterKind::Axebot, "BOOT_UP_MOVE") => {
            let respawn_count = 2 - axebot_stock(ai_flags(c, e));
            c.gain_move_block(e, asc_value(asc, DEADLY_ASC, 15, 10));
            let strength = asc_value(asc, DEADLY_ASC, 4, 3) * respawn_count;
            if strength != 0 {
                c.apply_power_to(e, PowerId::Strength, strength, Some(e));
            }
        }
        (MonsterKind::Axebot, "ONE_TWO_MOVE") => attack(c, e, kind, state),
        (MonsterKind::Axebot, "HAMMER_UPPERCUT_MOVE") => {
            attack(c, e, kind, state);
            c.monster_power_to_players(e, PowerId::Weak, 2);
            c.monster_power_to_players(e, PowerId::Frail, 2);
        }

        (MonsterKind::Zapbot, "ZAP") => attack(c, e, kind, state),
        (MonsterKind::Stabbot, "STAB_MOVE") => {
            attack(c, e, kind, state);
            c.monster_power_to_players(e, PowerId::Frail, 1);
        }
        (MonsterKind::Guardbot, "GUARD_MOVE") => {
            let targets: ArrayVec<CId, 8> = c
                .alive_enemies()
                .into_iter()
                .filter(|&x| c.cr(x).monster_id == "FABRICATOR")
                .collect();
            for target in targets {
                gain_unpowered_block(c, target, 15);
            }
        }
        (MonsterKind::Noisebot, "NOISE_MOVE") => {
            add_status(c, CardId::Dazed, 1, PileKind::Discard, PilePos::Bottom);
            add_status(c, CardId::Dazed, 1, PileKind::Draw, PilePos::Random);
        }
        (MonsterKind::Fabricator, "FABRICATE_MOVE") => {
            fabricator_spawn(c, e, true);
            fabricator_spawn(c, e, false);
        }
        (MonsterKind::Fabricator, "FABRICATING_STRIKE_MOVE") => {
            attack(c, e, kind, state);
            fabricator_spawn(c, e, false);
        }
        (MonsterKind::Fabricator, "DISINTEGRATE_MOVE") => attack(c, e, kind, state),

        (MonsterKind::FrogKnight, "TONGUE_LASH") => {
            attack(c, e, kind, state);
            c.monster_power_to_players(e, PowerId::Frail, 2);
        }
        (MonsterKind::FrogKnight, "STRIKE_DOWN_EVIL") => attack(c, e, kind, state),
        (MonsterKind::FrogKnight, "FOR_THE_QUEEN") => {
            c.apply_power_to(e, PowerId::Strength, 5, Some(e));
        }
        (MonsterKind::FrogKnight, "BEETLE_CHARGE") => {
            attack(c, e, kind, state);
            set_ai_flags(c, e, ai_flags(c, e) | FROG_BEETLE_CHARGED);
        }

        (MonsterKind::GlobeHead, "SHOCKING_SLAP") => {
            attack(c, e, kind, state);
            c.monster_power_to_players(e, PowerId::Frail, 2);
        }
        (MonsterKind::GlobeHead, "THUNDER_STRIKE") => attack(c, e, kind, state),
        (MonsterKind::GlobeHead, "GALVANIC_BURST") => {
            attack(c, e, kind, state);
            c.apply_power_to(e, PowerId::Strength, 2, Some(e));
        }

        (MonsterKind::OwlMagistrate, "MAGISTRATE_SCRUTINY" | "PECK_ASSAULT") => {
            attack(c, e, kind, state)
        }
        (MonsterKind::OwlMagistrate, "JUDICIAL_FLIGHT") => {
            c.apply_power_to(e, PowerId::Soar, 1, Some(e));
        }
        (MonsterKind::OwlMagistrate, "VERDICT") => {
            attack(c, e, kind, state);
            c.monster_power_to_players(e, PowerId::Vulnerable, 4);
            raw_pop_power(c, e, PowerId::Soar);
        }

        (MonsterKind::SlimedBerserker, "VOMIT_ICHOR_MOVE") => {
            add_status(c, CardId::Slimed, 10, PileKind::Discard, PilePos::Bottom);
        }
        (MonsterKind::SlimedBerserker, "FURIOUS_PUMMELING_MOVE") => attack(c, e, kind, state),
        (MonsterKind::SlimedBerserker, "LEECHING_HUG_MOVE") => {
            c.monster_power_to_players(e, PowerId::Weak, 3);
            c.apply_power_to(e, PowerId::Strength, 3, Some(e));
        }
        (MonsterKind::SlimedBerserker, "SMOTHER_MOVE") => attack(c, e, kind, state),

        (MonsterKind::TheLost, "DEBILITATING_SMOG") => {
            c.monster_power_to_players(e, PowerId::Strength, -2);
            c.apply_power_to(e, PowerId::Strength, 2, Some(e));
        }
        (MonsterKind::TheLost, "EYE_LASERS") => attack(c, e, kind, state),

        (MonsterKind::TheForgotten, "MIASMA") => {
            c.monster_power_to_players(e, PowerId::Dexterity, -2);
            c.gain_move_block(e, 8);
            c.apply_power_to(e, PowerId::Dexterity, 2, Some(e));
        }
        (MonsterKind::TheForgotten, "DREAD") => {
            c.monster_attack_players(e, c.forgotten_dread_damage(e), 1);
        }

        (MonsterKind::FlailKnight, "WAR_CHANT") | (MonsterKind::MysteriousKnight, "WAR_CHANT") => {
            c.apply_power_to(e, PowerId::Strength, 3, Some(e));
        }
        (MonsterKind::FlailKnight, "FLAIL_MOVE" | "RAM_MOVE") => attack(c, e, kind, state),
        (MonsterKind::MysteriousKnight, "FLAIL_MOVE" | "RAM_MOVE") => attack(c, e, kind, state),

        (MonsterKind::LivingShield, "SHIELD_SLAM_MOVE") => attack(c, e, kind, state),
        (MonsterKind::LivingShield, "SMASH_MOVE") => {
            attack(c, e, kind, state);
            c.apply_power_to(e, PowerId::Strength, 3, Some(e));
        }

        (MonsterKind::MagiKnight, "POWER_SHIELD_MOVE") => {
            attack(c, e, kind, state);
            c.gain_move_block(e, asc_value(asc, TOUGH_ASC, 9, 5));
        }
        (MonsterKind::MagiKnight, "DAMPEN_MOVE") => {
            // MagiKnight.cs DampenMove: every target gets this caster on its Dampen (applied
            // first when absent).
            for p in 0..c.players.len() as CId {
                if c.find_power(p, PowerId::Dampen).is_none() {
                    c.apply_power_to(p, PowerId::Dampen, 1, Some(e));
                }
                if c.find_power(p, PowerId::Dampen).is_some() {
                    c.dampen_add_caster(p, e);
                }
            }
        }
        (MonsterKind::MagiKnight, "RAM_MOVE" | "MAGIC_BOMB") => attack(c, e, kind, state),
        (MonsterKind::MagiKnight, "PREP_MOVE") => {
            c.gain_move_block(e, asc_value(asc, TOUGH_ASC, 9, 5))
        }

        (MonsterKind::SpectralKnight, "HEX") => c.monster_power_to_players(e, PowerId::Hex, 2),
        (MonsterKind::SpectralKnight, "SOUL_SLASH" | "SOUL_FLAME") => attack(c, e, kind, state),

        (MonsterKind::MechaKnight, "CHARGE_MOVE") => attack(c, e, kind, state),
        (MonsterKind::MechaKnight, "FLAMETHROWER_MOVE") => {
            attack(c, e, kind, state);
            if !c.is_over {
                add_status(c, CardId::Burn, 4, PileKind::Hand, PilePos::Bottom);
            }
        }
        (MonsterKind::MechaKnight, "WINDUP_MOVE") => {
            c.gain_move_block(e, 15);
            c.apply_power_to(e, PowerId::Strength, 5, Some(e));
        }
        (MonsterKind::MechaKnight, "HEAVY_CLEAVE_MOVE") => attack(c, e, kind, state),

        (MonsterKind::Osty, "NOTHING_MOVE") => {}

        (MonsterKind::SoulNexus, "SOUL_BURN_MOVE" | "MAELSTROM_MOVE") => attack(c, e, kind, state),
        (MonsterKind::SoulNexus, "DRAIN_LIFE_MOVE") => {
            attack(c, e, kind, state);
            c.monster_power_to_players(e, PowerId::Vulnerable, 2);
            c.monster_power_to_players(e, PowerId::Weak, 2);
        }

        (MonsterKind::Door, "DRAMATIC_OPEN_MOVE" | "DOOR_SLAM_MOVE") => attack(c, e, kind, state),
        (MonsterKind::Door, "ENFORCE_MOVE") => {
            attack(c, e, kind, state);
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 4, 3),
                Some(e),
            );
        }
        (MonsterKind::Door, "DEAD_MOVE") => {}

        (MonsterKind::Doormaker, "WHAT_IS_IT_MOVE") => {}
        (MonsterKind::Doormaker, "BEAM_MOVE") => attack(c, e, kind, state),
        (MonsterKind::Doormaker, "GET_BACK_IN_MOVE") => {
            attack(c, e, kind, state);
            if !c.is_over && c.cr(e).is_alive() {
                c.apply_power_to(e, PowerId::Strength, 5, Some(e));
                revive_door(c);
                escape_creature(c, e);
            }
        }

        (MonsterKind::RoyalGuard, "STRIKE") => attack(c, e, kind, state),

        (MonsterKind::Queen, "PUPPET_STRINGS_MOVE") => {
            c.monster_power_to_players(e, PowerId::ChainsOfBinding, 3)
        }
        (MonsterKind::Queen, "YOU_ARE_MINE_MOVE") => {
            c.monster_power_to_players(e, PowerId::Frail, 99);
            c.monster_power_to_players(e, PowerId::Weak, 99);
            c.monster_power_to_players(e, PowerId::Vulnerable, 99);
        }
        (MonsterKind::Queen, "BURN_BRIGHT_FOR_ME_MOVE") => {
            let side = c.cr(e).side;
            let allies: ArrayVec<CId, 8> = c
                .alive_enemies()
                .into_iter()
                .filter(|&x| x != e && c.cr(x).side == side)
                .collect();
            for ally in allies {
                c.apply_power_to(ally, PowerId::Strength, 1, Some(e));
            }
            c.gain_move_block(e, 20);
        }
        (MonsterKind::Queen, "OFF_WITH_YOUR_HEAD_MOVE" | "EXECUTION_MOVE") => {
            attack(c, e, kind, state)
        }
        (MonsterKind::Queen, "ENRAGE_MOVE") => {
            c.apply_power_to(e, PowerId::Strength, 2, Some(e));
        }

        (MonsterKind::TestSubject, "RESPAWN_MOVE") => {
            let flags = ai_flags(c, e);
            let respawns = test_respawns(flags) + 1;
            set_ai_flags(c, e, test_set_respawns(flags, respawns));
            // powers/remaining_a.py AdaptablePower.do_revive.
            c.set_power_private_int(e, PowerId::Adaptable, 0, 0);
            let hp = if respawns == 1 {
                asc_value(asc, TOUGH_ASC, 212, 200)
            } else {
                asc_value(asc, TOUGH_ASC, 313, 300)
            };
            // TestSubject.Revive: ScaleHpForMultiplayer(base respawn HP).
            let hp = c.scale_monster_hp(hp);
            {
                let cr = c.cr_mut(e);
                cr.max_hp = hp;
                cr.hp = hp;
                cr.escaped = false;
                cr.death_processed = false;
                cr.in_combat = true;
            }
            if respawns == 1 {
                c.apply_power_to(e, PowerId::PainfulStabs, 1, Some(e));
            } else {
                c.apply_power_to(e, PowerId::Nemesis, 1, Some(e));
                raw_pop_power(c, e, PowerId::Adaptable);
                raw_pop_power(c, e, PowerId::PainfulStabs);
            }
        }
        (MonsterKind::TestSubject, "BITE_MOVE") => attack(c, e, kind, state),
        (MonsterKind::TestSubject, "SKULL_BASH_MOVE") => {
            attack(c, e, kind, state);
            c.monster_power_to_players(e, PowerId::Vulnerable, 1);
        }
        (MonsterKind::TestSubject, "MULTI_CLAW_MOVE") => {
            let flags = ai_flags(c, e);
            let (damage, _) = source_attack_spec(kind, state, asc).unwrap();
            c.monster_attack_players(e, damage, c.test_subject_claw_hits(e));
            set_ai_flags(c, e, test_set_claw_extra(flags, test_claw_extra(flags) + 1));
        }
        (MonsterKind::TestSubject, "PHASE3_LACERATE_MOVE" | "BIG_POUNCE") => {
            attack(c, e, kind, state)
        }
        (MonsterKind::TestSubject, "BURNING_GROWL_MOVE") => {
            add_status(
                c,
                CardId::Burn,
                asc_value(asc, DEADLY_ASC, 5, 3),
                PileKind::Discard,
                PilePos::Bottom,
            );
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 3, 2),
                Some(e),
            );
        }

        (
            MonsterKind::TorchHeadAmalgam,
            "STRONG_TACKLE_MOVE" | "TACKLE_2_MOVE" | "BEAM_MOVE" | "TACKLE_3_MOVE"
            | "TACKLE_4_MOVE",
        ) => attack(c, e, kind, state),

        _ => return false,
    }
    true
}

pub(crate) fn setup_encounter(c: &mut Combat, encounter_id: crate::identity::EncounterId, rng: &mut Rng) -> bool {
    use crate::identity::EncounterId as E;
    match encounter_id {
        E::DevotedSculptorWeak => {
            add(c, MonsterKind::DevotedSculptor, 0);
        }
        E::ScrollsOfBitingWeak => {
            let starter = rng.next_int(0, 2) as u32;
            for offset in 0..3 {
                add(c, MonsterKind::ScrollOfBiting, (starter + offset) % 3);
            }
        }
        E::TurretOperatorWeak => {
            add(c, MonsterKind::LivingShield, 0);
            add(c, MonsterKind::TurretOperator, 0);
        }

        E::AxebotsNormal => {
            add(c, MonsterKind::Axebot, axebot_flags(false, None));
        }
        E::ConstructMenagerieNormal => {
            super::monsters_underdocks::add_punch_construct(c, false, 0);
            for _ in 0..2 {
                let cubex = add(c, MonsterKind::CubexConstruct, 0);
                c.apply_power_to(cubex, PowerId::Artifact, 1, Some(cubex));
            }
        }
        E::FabricatorNormal => {
            c.configure_encounter_slots(&["bot1", "bot2", "fabricator", "bot3", "bot4"]);
            let (cr, ai) = c.create_monster(MonsterKind::Fabricator, 0);
            c.add_enemy_in_slot(cr, ai, Some("fabricator"));
        }
        E::FrogKnightNormal => {
            add(c, MonsterKind::FrogKnight, 0);
        }
        E::GlobeHeadNormal => {
            add(c, MonsterKind::GlobeHead, 0);
        }
        E::OwlMagistrateNormal => {
            add(c, MonsterKind::OwlMagistrate, 0);
        }
        E::ScrollsOfBitingNormal => {
            let starter = rng.next_int(0, 2) as u32;
            for offset in 0..3 {
                add(c, MonsterKind::ScrollOfBiting, (starter + offset) % 3);
            }
            add(c, MonsterKind::ScrollOfBiting, 2);
        }
        E::SlimedBerserkerNormal => {
            add(c, MonsterKind::SlimedBerserker, 0);
        }
        E::TheLostAndForgottenNormal => {
            add(c, MonsterKind::TheLost, 0);
            add(c, MonsterKind::TheForgotten, 0);
        }

        E::KnightsElite => {
            add(c, MonsterKind::FlailKnight, 0);
            add(c, MonsterKind::SpectralKnight, 0);
            add(c, MonsterKind::MagiKnight, 0);
        }
        E::MechaKnightElite => {
            add(c, MonsterKind::MechaKnight, 0);
        }
        E::SoulNexusElite => {
            add(c, MonsterKind::SoulNexus, 0);
        }

        // TheLanternKey's C# CanonicalEncounter is MysteriousKnightEventEncounter.
        // Frozen r7 setup_mysterious_knight creates the A0/A10 MysteriousKnight
        // directly; the generated monster table carries its +6 Strength/+6 Plating,
        // RAM initial move, weighted RAND follow-up, and ascension-scaled HP/damage.
        E::MysteriousKnightEventEncounter => {
            add(c, MonsterKind::MysteriousKnight, 0);
        }

        E::AeonglassBoss => {
            let e = add(c, MonsterKind::Aeonglass, 0);
            // Aeonglass.AfterAddedToRoom: one Withering Presence per opponent (CardsLeft 6).
            for target in c.player_side() {
                if let Some(p) = c.apply_targeted_power(e, PowerId::WitheringPresence, 6, Some(e), target) {
                    c.powers[p as usize].vars[0] = 6;
                }
            }
            c.apply_power_to(e, PowerId::Artifact, 3, Some(e));
        }
        E::DoormakerBoss => {
            add(c, MonsterKind::Door, 0);
        }
        E::QueenBoss => {
            add(c, MonsterKind::TorchHeadAmalgam, 0);
            add(c, MonsterKind::Queen, 0);
        }
        E::TestSubjectBoss => {
            add(c, MonsterKind::TestSubject, 0);
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACT3_TABLE_ATTACKS: &[(MonsterKind, &str)] = &[
        (MonsterKind::Aeonglass, "EBB_MOVE"),
        (MonsterKind::Aeonglass, "EYE_LASERS_MOVE"),
        (MonsterKind::DevotedSculptor, "SAVAGE_MOVE"),
        (MonsterKind::ScrollOfBiting, "CHOMP"),
        (MonsterKind::ScrollOfBiting, "CHEW"),
        (MonsterKind::TurretOperator, "UNLOAD_MOVE"),
        (MonsterKind::TurretOperator, "UNLOAD_MOVE_2"),
        (MonsterKind::Axebot, "ONE_TWO_MOVE"),
        (MonsterKind::Axebot, "HAMMER_UPPERCUT_MOVE"),
        (MonsterKind::Zapbot, "ZAP"),
        (MonsterKind::Stabbot, "STAB_MOVE"),
        (MonsterKind::Fabricator, "FABRICATING_STRIKE_MOVE"),
        (MonsterKind::Fabricator, "DISINTEGRATE_MOVE"),
        (MonsterKind::FrogKnight, "TONGUE_LASH"),
        (MonsterKind::FrogKnight, "STRIKE_DOWN_EVIL"),
        (MonsterKind::FrogKnight, "BEETLE_CHARGE"),
        (MonsterKind::GlobeHead, "SHOCKING_SLAP"),
        (MonsterKind::GlobeHead, "THUNDER_STRIKE"),
        (MonsterKind::GlobeHead, "GALVANIC_BURST"),
        (MonsterKind::OwlMagistrate, "MAGISTRATE_SCRUTINY"),
        (MonsterKind::OwlMagistrate, "PECK_ASSAULT"),
        (MonsterKind::OwlMagistrate, "VERDICT"),
        (MonsterKind::SlimedBerserker, "FURIOUS_PUMMELING_MOVE"),
        (MonsterKind::SlimedBerserker, "SMOTHER_MOVE"),
        (MonsterKind::TheLost, "EYE_LASERS"),
        (MonsterKind::TheForgotten, "DREAD"),
        (MonsterKind::FlailKnight, "FLAIL_MOVE"),
        (MonsterKind::FlailKnight, "RAM_MOVE"),
        (MonsterKind::LivingShield, "SHIELD_SLAM_MOVE"),
        (MonsterKind::LivingShield, "SMASH_MOVE"),
        (MonsterKind::MagiKnight, "POWER_SHIELD_MOVE"),
        (MonsterKind::MagiKnight, "RAM_MOVE"),
        (MonsterKind::MagiKnight, "MAGIC_BOMB"),
        (MonsterKind::SpectralKnight, "SOUL_SLASH"),
        (MonsterKind::SpectralKnight, "SOUL_FLAME"),
        (MonsterKind::MechaKnight, "CHARGE_MOVE"),
        (MonsterKind::MechaKnight, "FLAMETHROWER_MOVE"),
        (MonsterKind::MechaKnight, "HEAVY_CLEAVE_MOVE"),
        (MonsterKind::SoulNexus, "SOUL_BURN_MOVE"),
        (MonsterKind::SoulNexus, "MAELSTROM_MOVE"),
        (MonsterKind::SoulNexus, "DRAIN_LIFE_MOVE"),
        (MonsterKind::Door, "DRAMATIC_OPEN_MOVE"),
        (MonsterKind::Door, "DOOR_SLAM_MOVE"),
        (MonsterKind::Door, "ENFORCE_MOVE"),
        (MonsterKind::Doormaker, "BEAM_MOVE"),
        (MonsterKind::Doormaker, "GET_BACK_IN_MOVE"),
        (MonsterKind::RoyalGuard, "STRIKE"),
        (MonsterKind::Queen, "OFF_WITH_YOUR_HEAD_MOVE"),
        (MonsterKind::Queen, "EXECUTION_MOVE"),
        (MonsterKind::TestSubject, "BITE_MOVE"),
        (MonsterKind::TestSubject, "SKULL_BASH_MOVE"),
        (MonsterKind::TestSubject, "MULTI_CLAW_MOVE"),
        (MonsterKind::TestSubject, "PHASE3_LACERATE_MOVE"),
        (MonsterKind::TestSubject, "BIG_POUNCE"),
    ];

    const SHARED_TORCH_ATTACKS: &[(&str, MonsterKind)] = &[
        ("STRONG_TACKLE_MOVE", MonsterKind::TorchHeadAmalgam),
        ("TACKLE_2_MOVE", MonsterKind::TorchHeadAmalgam),
        ("BEAM_MOVE", MonsterKind::TorchHeadAmalgam),
        ("TACKLE_3_MOVE", MonsterKind::TorchHeadAmalgam),
        ("TACKLE_4_MOVE", MonsterKind::TorchHeadAmalgam),
    ];

    fn table_attack(kind: MonsterKind, state: &str, band: usize) -> Option<(i32, i32)> {
        let def = monster_def(kind, band);
        let state = def.states.iter().find(|s| s.id == state)?;
        let StateKind::Move { intents, .. } = state.kind else {
            return None;
        };
        intents.iter().find_map(|i| match i.kind {
            IntentType::Attack | IntentType::MultiAttack => Some((i.damage, i.hits)),
            _ => None,
        })
    }

    #[test]
    fn act3_attack_intents_match_r7_monster_defs_a0_a10() {
        let mut checks = 0usize;
        for &(kind, state) in ACT3_TABLE_ATTACKS {
            for (band, asc) in [(0usize, 0i32), (1usize, 10i32)] {
                let source = source_attack_spec(kind, state, asc).unwrap();
                let table = table_attack(kind, state, band)
                    .unwrap_or_else(|| panic!("missing table attack {kind:?} {state}"));
                assert_eq!(source, table, "{kind:?} {state} A{asc}");
                checks += 1;
            }
        }
        for &(state, kind) in SHARED_TORCH_ATTACKS {
            for (band, asc) in [(0usize, 0i32), (1usize, 10i32)] {
                assert_eq!(
                    source_attack_spec(kind, state, asc).unwrap(),
                    table_attack(kind, state, band).unwrap()
                );
                checks += 1;
            }
        }
        println!("act3 attack audit: {checks} A0/A10 checks, 0 mismatches");
    }

    #[test]
    fn source_only_mysterious_knight_uses_act3_values() {
        assert_eq!(
            source_attack_spec(MonsterKind::MysteriousKnight, "FLAIL_MOVE", 0),
            Some((9, 2))
        );
        assert_eq!(
            source_attack_spec(MonsterKind::MysteriousKnight, "FLAIL_MOVE", 10),
            Some((9, 2))
        );
        assert_eq!(
            source_attack_spec(MonsterKind::MysteriousKnight, "RAM_MOVE", 0),
            Some((15, 1))
        );
        assert_eq!(
            source_attack_spec(MonsterKind::MysteriousKnight, "RAM_MOVE", 10),
            Some((15, 1))
        );
    }
}
