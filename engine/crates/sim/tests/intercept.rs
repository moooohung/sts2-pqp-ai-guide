//! C# Intercept.OnPlay, CoveredPower and InterceptPower combat contracts.
use sts2_sim::card::CId;
use sts2_sim::combat::{Combat, Props};
use sts2_sim::creature::Side;
use sts2_sim::rng::Rng;
use sts2_sim::run::{RunManager, RunOptions};
use sts2_sim::tables::{CardId, PowerId};

fn combat(seats: usize) -> Combat {
    let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    for _ in 1..seats {
        c.add_player(80, 80, 3, 0, &[]);
    }
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(7));
    c
}

#[test]
fn intercept_blocks_for_caster_and_counts_distinct_covered_seats_until_enemy_end() {
    for seats in 2..=4 {
        let mut c = combat(seats);
        let enemy = c.enemies[0];
        let k = c.new_card(0, CardId::InterceptCard, false).unwrap();
        for target in 1..seats as CId {
            for _ in 0..2 {
                c.play_card_effect(k, Some(target));
            }
            assert_eq!(c.cr(target).block, 0);
            assert_eq!(c.calculate_damage(8, Some(enemy), target, Props::MOVE), 0);
            assert_eq!(c.calculate_damage(8, Some(enemy), target, Props::UNPOWERED), 8);
        }
        assert_eq!(c.cr(0).block, 18 * (seats - 1) as i32);
        assert_eq!(c.calculate_damage(8, Some(enemy), 0, Props::MOVE), 8 * seats as i32);
        c.fire_after_turn_end(Side::Player);
        assert!(c.has_power(0, PowerId::Intercept));
        c.fire_after_turn_end(Side::Enemy);
        for target in 0..seats as CId {
            assert!(!c.has_power(target, PowerId::Covered));
            assert!(!c.has_power(target, PowerId::Intercept));
        }
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn covered_instances_preserve_other_appliers_when_one_interceptor_dies() {
    let mut c = combat(3);
    let enemy = c.enemies[0];
    for applier in [0, 1] {
        let k = c.new_card(applier, CardId::InterceptCard, false).unwrap();
        c.play_card_effect(k, Some(2));
    }
    c.kill_creature(0);
    let instances: Vec<_> = c.cr(2).power_list().into_iter()
        .filter(|&p| c.powers[p as usize].id == PowerId::Covered).collect();
    assert_eq!(instances.len(), 1);
    assert_eq!(c.powers[instances[0] as usize].applier, Some(1));
    assert_eq!(c.calculate_damage(8, Some(enemy), 2, Props::MOVE), 0);
    c.kill_creature(1);
    assert!(!c.has_power(2, PowerId::Covered));
    assert_eq!(c.calculate_damage(8, Some(enemy), 2, Props::MOVE), 8);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn intercept_allows_self_but_missing_target_fails_before_granting_block() {
    let mut c = combat(2);
    let enemy = c.enemies[0];
    let k = c.new_card(0, CardId::InterceptCard, true).unwrap();
    c.play_card_effect(k, Some(0));
    assert_eq!(c.cr(0).block, 13);
    assert_eq!(c.calculate_damage(8, Some(enemy), 0, Props::MOVE), 0);
    assert!(c.fault.is_none(), "{:?}", c.fault);
    let mut c = combat(2);
    let k = c.new_card(0, CardId::InterceptCard, false).unwrap();
    c.play_card_effect(k, None);
    assert_eq!(c.fault, Some("Intercept requires an ally target"));
    assert_eq!(c.cr(0).block, 0);
    assert!(!c.has_power(0, PowerId::Intercept));
}
