//! r7 `monsters/act1_weak.py` + `monsters/act1.py` and `encounters/act1.py` (Overgrowth).

use super::{DEADLY_ASC, TOUGH_ASC, asc_value, state_index};
use crate::card::CId;
use crate::combat::{Combat, PileKind, PilePos};
use crate::monster::MonsterAi;
use crate::rng::Rng;
use crate::tables::*;

// Factory arguments encoded in `MonsterAi::flags`.
const NIBBIT_ALONE: u32 = 1;
const NIBBIT_FRONT: u32 = 2;
const INKLET_MIDDLE: u32 = 1;
const KIN_FOLLOWER_DANCE: u32 = 1;


/// `CardPileCmd.AddToCombatAndPreview<id>(targets, Discard, count)` for every living player.
fn discard_status(c: &mut Combat, id: CardId, count: i32) {
    for p in c.living_players() {
        for _ in 0..count {
            c.create_card_in_pile(p, id, false, PileKind::Discard, PilePos::Bottom);
        }
    }
}

#[inline]
fn add(c: &mut Combat, kind: MonsterKind, flags: u32) -> CId {
    let (cr, ai) = c.create_monster(kind, flags);
    c.add_enemy(cr, ai)
}

/// Initial state chosen by factory arguments; `None` = the table default (or not ours).
pub(crate) fn initial_state(kind: MonsterKind, flags: u32, def: &MonsterDef) -> Option<u8> {
    match kind {
        MonsterKind::Nibbit => Some(state_index(
            def,
            if flags & NIBBIT_ALONE != 0 {
                "BUTT_MOVE"
            } else if flags & NIBBIT_FRONT != 0 {
                "SLICE_MOVE"
            } else {
                "HISS_MOVE"
            },
        )),
        MonsterKind::Inklet => Some(state_index(
            def,
            if flags & INKLET_MIDDLE != 0 {
                "WHIRLWIND_MOVE"
            } else {
                "JAB_MOVE"
            },
        )),
        MonsterKind::KinFollower => Some(state_index(
            def,
            if flags & KIN_FOLLOWER_DANCE != 0 {
                "POWER_DANCE_MOVE"
            } else {
                "QUICK_SLASH_MOVE"
            },
        )),
        _ => None,
    }
}

pub(crate) fn weight(_c: &Combat, _e: CId, _ai: &MonsterAi, _target: u8) -> Option<f64> {
    None
}

pub(crate) fn condition(
    _c: &Combat,
    _e: CId,
    ai: &MonsterAi,
    targets: &'static [u8],
) -> Option<u8> {
    match ai.kind {
        // r7 act1_weak.py:344-347: is_alone, is_front, True.
        MonsterKind::Nibbit => Some(if ai.flags & NIBBIT_ALONE != 0 {
            targets[0]
        } else if ai.flags & NIBBIT_FRONT != 0 {
            targets[1]
        } else {
            targets[2]
        }),
        _ => None,
    }
}

pub(crate) fn move_effect(c: &mut Combat, e: CId, kind: MonsterKind, state: &'static str) -> bool {
    let asc = c.run.ascension;
    match (kind, state) {
        // act1_weak.py
        (MonsterKind::ShrinkerBeetle, "SHRINKER_MOVE") => c.monster_power_to_players(e, PowerId::Shrink, -1),
        (MonsterKind::ShrinkerBeetle, "CHOMP_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 8, 7), 1)
        }
        (MonsterKind::ShrinkerBeetle, "STOMP_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 14, 13), 1)
        }

        (MonsterKind::FuzzyWurmCrawler, "FIRST_ACID_GOOP" | "ACID_GOOP") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 6, 4), 1)
        }
        (MonsterKind::FuzzyWurmCrawler, "INHALE") => {
            c.apply_power_to(e, PowerId::Strength, 7, Some(e));
        }

        (MonsterKind::Nibbit, "BUTT_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 13, 12), 1)
        }
        (MonsterKind::Nibbit, "SLICE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 7, 6), 1);
            c.gain_move_block(e, asc_value(asc, TOUGH_ASC, 6, 5));
        }
        (MonsterKind::Nibbit, "HISS_MOVE") => {
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 3, 2),
                Some(e),
            );
        }

        (MonsterKind::LeafSlimeS, "TACKLE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 4, 3), 1)
        }
        (MonsterKind::LeafSlimeS, "GOOP_MOVE") => discard_status(c, CardId::Slimed, 1),
        (MonsterKind::TwigSlimeS, "TACKLE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 5, 4), 1)
        }
        (MonsterKind::LeafSlimeM, "STICKY_SHOT") => discard_status(c, CardId::Slimed, 2),
        (MonsterKind::LeafSlimeM, "CLUMP_SHOT") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 9, 8), 1)
        }
        (MonsterKind::TwigSlimeM, "STICKY_SHOT_MOVE") => discard_status(c, CardId::Slimed, 1),
        (MonsterKind::TwigSlimeM, "POKEY_POUNCE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 12, 11), 1)
        }

        // act1.py
        (MonsterKind::CubexConstruct, "CHARGE_UP_MOVE") => {
            c.apply_power_to(e, PowerId::Strength, 2, Some(e));
        }
        (MonsterKind::CubexConstruct, "REPEATER_BLAST_MOVE" | "REPEATER_BLAST_MOVE_2") => {
            c.monster_attack_players_then_strength(e, asc_value(asc, DEADLY_ASC, 8, 7), 1, 2);
        }
        (MonsterKind::CubexConstruct, "EXPEL_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 6, 5), 2)
        }

        (MonsterKind::Flyconid, "VULNERABLE_SPORES_MOVE") => {
            c.monster_power_to_players(e, PowerId::Vulnerable, 2)
        }
        (MonsterKind::Flyconid, "FRAIL_SPORES_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 9, 8), 1);
            c.monster_power_to_players(e, PowerId::Frail, 2);
        }
        (MonsterKind::Flyconid, "SMASH_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 12, 11), 1)
        }

        (MonsterKind::EyeWithTeeth, "DISTRACT_MOVE") => discard_status(c, CardId::Dazed, 3),
        (MonsterKind::Fogmog, "ILLUSION_MOVE") => {
            // r7 act1.py:352-354. `add_enemy` marks this fail-closed while the engine lacks mid-combat summon support.
            add(c, MonsterKind::EyeWithTeeth, 0);
        }
        (MonsterKind::Fogmog, "SWIPE_MOVE" | "SWIPE_RANDOM_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 9, 8), 1);
            c.apply_power_to(e, PowerId::Strength, 1, Some(e));
        }
        (MonsterKind::Fogmog, "HEADBUTT_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 16, 14), 1)
        }

        (MonsterKind::Inklet, "JAB_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 4, 3), 1)
        }
        (MonsterKind::Inklet, "WHIRLWIND_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 3, 2), 3)
        }
        (MonsterKind::Inklet, "PIERCING_GAZE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 11, 10), 1)
        }

        (MonsterKind::Mawler, "RIP_AND_TEAR_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 16, 14), 1)
        }
        (MonsterKind::Mawler, "ROAR_MOVE") => c.monster_power_to_players(e, PowerId::Vulnerable, 3),
        (MonsterKind::Mawler, "CLAW_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 5, 4), 2)
        }

        (MonsterKind::VineShambler, "GRASPING_VINES_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 9, 8), 1);
            c.monster_power_to_players(e, PowerId::Tangled, 1);
        }
        (MonsterKind::VineShambler, "SWIPE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 7, 6), 2)
        }
        (MonsterKind::VineShambler, "CHOMP_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 18, 16), 1)
        }

        (MonsterKind::SlitheringStrangler, "CONSTRICT") => {
            c.monster_power_to_players(e, PowerId::Constrict, 3)
        }
        (MonsterKind::SlitheringStrangler, "THWACK") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 8, 7), 1);
            c.gain_move_block(e, 5);
        }
        (MonsterKind::SlitheringStrangler, "LASH") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 13, 12), 1)
        }

        (MonsterKind::SnappingJaxfruit, "ENERGY_ORB_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 4, 3), 1);
            c.apply_power_to(e, PowerId::Strength, 2, Some(e));
        }

        (MonsterKind::AssassinRubyRaider, "KILLSHOT_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 11, 10), 1)
        }
        (MonsterKind::AxeRubyRaider, "SWING_1" | "SWING_2") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 6, 5), 1);
            c.gain_move_block(e, asc_value(asc, DEADLY_ASC, 6, 5));
        }
        (MonsterKind::AxeRubyRaider, "BIG_SWING") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 13, 12), 1)
        }
        (MonsterKind::BruteRubyRaider, "BEAT_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 8, 7), 1)
        }
        (MonsterKind::BruteRubyRaider, "ROAR_MOVE") => {
            c.apply_power_to(e, PowerId::Strength, 3, Some(e));
        }
        (MonsterKind::CrossbowRubyRaider, "RELOAD_MOVE") => c.gain_move_block(e, 3),
        (MonsterKind::CrossbowRubyRaider, "FIRE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 16, 14), 1)
        }
        (MonsterKind::TrackerRubyRaider, "TRACK_MOVE") => c.monster_power_to_players(e, PowerId::Frail, 2),
        (MonsterKind::TrackerRubyRaider, "HOUNDS_MOVE") => {
            c.monster_attack_players(e, 1, asc_value(asc, DEADLY_ASC, 9, 8))
        }

        (MonsterKind::BygoneEffigy, "SLEEP_MOVE" | "SLEEP_MOVE_2") => {}
        (MonsterKind::BygoneEffigy, "WAKE_MOVE") => {
            c.apply_power_to(e, PowerId::Strength, 10, Some(e));
        }
        (MonsterKind::BygoneEffigy, "SLASHES_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 15, 13), 1)
        }
        (MonsterKind::Byrdonis, "SWOOP_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 19, 17), 1)
        }
        (MonsterKind::Byrdonis, "PECK_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 4, 3), 3)
        }
        (MonsterKind::PhrogParasite, "INFECT_MOVE") => discard_status(c, CardId::Infection, 3),
        (MonsterKind::PhrogParasite, "LASH_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 5, 4), 4)
        }
        (MonsterKind::Parafright, "SLAM_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 17, 16), 1)
        }

        (MonsterKind::Vantom, "INK_BLOT_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 8, 7), 1)
        }
        (MonsterKind::Vantom, "INKY_LANCE_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 7, 6), 2)
        }
        (MonsterKind::Vantom, "DISMEMBER_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 30, 26), 1);
            if !c.is_over {
                discard_status(c, CardId::Wound, 3);
            }
        }
        (MonsterKind::Vantom, "PREPARE_MOVE") => {
            c.apply_power_to(e, PowerId::Strength, 2, Some(e));
        }

        (MonsterKind::CeremonialBeast, "STAMP_MOVE") => {
            c.apply_power_to(
                e,
                PowerId::Plow,
                asc_value(asc, DEADLY_ASC, 160, 150),
                Some(e),
            );
        }
        (MonsterKind::CeremonialBeast, "PLOW_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 20, 18), 1);
            if !c.is_over {
                c.apply_power_to(e, PowerId::Strength, 2, Some(e));
            }
        }
        (MonsterKind::CeremonialBeast, "STUN_MOVE") => {}
        (MonsterKind::CeremonialBeast, "BEAST_CRY_MOVE") => c.monster_power_to_players(e, PowerId::Ringing, 1),
        (MonsterKind::CeremonialBeast, "STOMP_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 17, 15), 1)
        }
        (MonsterKind::CeremonialBeast, "CRUSH_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 19, 17), 1);
            if !c.is_over {
                c.apply_power_to(
                    e,
                    PowerId::Strength,
                    asc_value(asc, DEADLY_ASC, 4, 3),
                    Some(e),
                );
            }
        }

        (MonsterKind::KinPriest, "ORB_OF_FRAILTY_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 9, 8), 1);
            if !c.is_over {
                c.monster_power_to_players(e, PowerId::Frail, 1);
            }
        }
        (MonsterKind::KinPriest, "ORB_OF_WEAKNESS_MOVE") => {
            c.monster_attack_players(e, asc_value(asc, DEADLY_ASC, 9, 8), 1);
            if !c.is_over {
                c.monster_power_to_players(e, PowerId::Weak, 1);
            }
        }
        (MonsterKind::KinPriest, "BEAM_MOVE") => c.monster_attack_players(e, 3, 3),
        (MonsterKind::KinPriest, "RITUAL_MOVE") => {
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 3, 2),
                Some(e),
            );
        }
        (MonsterKind::KinFollower, "QUICK_SLASH_MOVE") => c.monster_attack_players(e, 5, 1),
        (MonsterKind::KinFollower, "BOOMERANG_MOVE") => c.monster_attack_players(e, 2, 2),
        (MonsterKind::KinFollower, "POWER_DANCE_MOVE") => {
            c.apply_power_to(
                e,
                PowerId::Strength,
                asc_value(asc, DEADLY_ASC, 3, 2),
                Some(e),
            );
        }
        _ => return false,
    }
    true
}

pub(crate) fn setup_encounter(c: &mut Combat, encounter_id: crate::identity::EncounterId, rng: &mut Rng) -> bool {
    use crate::identity::EncounterId as E;
    match encounter_id {
        // Weak.
        E::ShrinkerBeetleWeak => {
            add(c, MonsterKind::ShrinkerBeetle, 0);
        }
        E::FuzzyWurmCrawlerWeak => {
            add(c, MonsterKind::FuzzyWurmCrawler, 0);
        }
        E::NibbitsWeak => {
            add(c, MonsterKind::Nibbit, NIBBIT_ALONE);
        }
        E::SlimesWeak => {
            let small = [MonsterKind::LeafSlimeS, MonsterKind::TwigSlimeS];
            let chosen = rng.sample(&small, 2);
            let medium = [MonsterKind::LeafSlimeM, MonsterKind::TwigSlimeM];
            let mid = medium[rng.choice_index(medium.len())];
            add(c, chosen[0], 0);
            add(c, mid, 0);
            add(c, chosen[1], 0);
        }

        // Normal.
        E::CubexConstructNormal => {
            let e = add(c, MonsterKind::CubexConstruct, 0);
            // r7 act1.py:176-184: initial-room GainBlock is gated off; Artifact still applies.
            c.apply_power_to(e, PowerId::Artifact, 1, Some(e));
        }
        E::FlyconidNormal => {
            let medium = [MonsterKind::LeafSlimeM, MonsterKind::TwigSlimeM];
            add(c, medium[rng.choice_index(medium.len())], 0);
            add(c, MonsterKind::Flyconid, 0);
        }
        E::FogmogNormal => {
            // r7 encounters/act1.py:98-101
            c.configure_encounter_slots(&["illusion", "fogmog"]);
            let (cr, ai) = c.create_monster(MonsterKind::Fogmog, 0);
            c.add_enemy_in_slot(cr, ai, Some("fogmog"));
        }
        E::InkletsNormal => {
            add(c, MonsterKind::Inklet, 0);
            add(c, MonsterKind::Inklet, INKLET_MIDDLE);
            add(c, MonsterKind::Inklet, 0);
        }
        E::MawlerNormal => {
            add(c, MonsterKind::Mawler, 0);
        }
        E::NibbitsNormal => {
            add(c, MonsterKind::Nibbit, NIBBIT_FRONT);
            add(c, MonsterKind::Nibbit, 0);
        }
        E::OvergrowthCrawlers => {
            add(c, MonsterKind::ShrinkerBeetle, 0);
            add(c, MonsterKind::FuzzyWurmCrawler, 0);
        }
        E::RubyRaidersNormal => {
            let raiders = [
                MonsterKind::AxeRubyRaider,
                MonsterKind::AssassinRubyRaider,
                MonsterKind::BruteRubyRaider,
                MonsterKind::CrossbowRubyRaider,
                MonsterKind::TrackerRubyRaider,
            ];
            for kind in rng.sample(&raiders, 3) {
                add(c, kind, 0);
            }
        }
        E::SlimesNormal => {
            let (first_small, second_small) = if rng.next_bool() {
                (MonsterKind::LeafSlimeS, MonsterKind::TwigSlimeS)
            } else {
                (MonsterKind::TwigSlimeS, MonsterKind::LeafSlimeS)
            };
            for kind in [
                MonsterKind::TwigSlimeM,
                MonsterKind::LeafSlimeM,
                first_small,
                second_small,
            ] {
                add(c, kind, 0);
            }
        }
        E::SlitheringStranglerNormal => {
            match rng.choice_index(3) {
                0 => {
                    add(c, MonsterKind::SnappingJaxfruit, 0);
                }
                1 => {
                    let medium = [MonsterKind::LeafSlimeM, MonsterKind::TwigSlimeM];
                    add(c, medium[rng.choice_index(2)], 0);
                }
                _ => {
                    let small = [MonsterKind::LeafSlimeS, MonsterKind::TwigSlimeS];
                    add(c, small[rng.choice_index(2)], 0);
                    add(c, small[rng.choice_index(2)], 0);
                }
            }
            add(c, MonsterKind::SlitheringStrangler, 0);
        }
        E::SnappingJaxfruitNormal => {
            add(c, MonsterKind::SnappingJaxfruit, 0);
            add(c, MonsterKind::Flyconid, 0);
        }
        E::VineShamblerNormal => {
            add(c, MonsterKind::VineShambler, 0);
        }

        // Elites.
        E::BygoneEffigyElite => {
            add(c, MonsterKind::BygoneEffigy, 0);
        }
        E::ByrdonisElite => {
            add(c, MonsterKind::Byrdonis, 0);
        }
        E::PhrogParasiteElite => {
            add(c, MonsterKind::PhrogParasite, 0);
        }

        // Bosses.
        E::VantomBoss => {
            add(c, MonsterKind::Vantom, 0);
        }
        E::CeremonialBeastBoss => {
            add(c, MonsterKind::CeremonialBeast, 0);
        }
        E::TheKinBoss => {
            add(c, MonsterKind::KinFollower, KIN_FOLLOWER_DANCE);
            add(c, MonsterKind::KinFollower, 0);
            add(c, MonsterKind::KinPriest, 0);
        }
        _ => return false,
    }
    true
}
