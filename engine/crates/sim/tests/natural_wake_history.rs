//! C# natural wake precedes player-turn RollMove; Whistle must resume the awake move.
use sts2_sim::combat::Combat;
use sts2_sim::creature::Side;
use sts2_sim::monster::STUNNED;
use sts2_sim::rng::Rng;
use sts2_sim::run::{RunManager, RunOptions};
use sts2_sim::tables::{CardId, PowerId};

#[test]
fn natural_wake_replaces_premature_sleep_roll_for_beetle_and_lagavulin() {
    for (enc, monster, sleep_power, awake) in [
        ("SLUMBERING_BEETLE_NORMAL", "SLUMBERING_BEETLE", PowerId::Slumber, "ROLL_OUT_MOVE"),
        ("LAGAVULIN_MATRIARCH_BOSS", "LAGAVULIN_MATRIARCH", PowerId::Asleep, "SLASH_MOVE"),
    ] {
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        c.setup_encounter(enc, &mut Rng::new(7));
        let e = *c.enemies.iter().find(|&&e| c.cr(e).monster_id == monster).unwrap();
        let p = c.find_power(e, sleep_power).unwrap();
        c.powers[p as usize].amount = 1;
        let ai = c.cr_mut(e).ai.as_mut().unwrap();
        let mv = ai.current;
        let old_history = ai.log.clone();
        ai.mark_performing(mv);
        ai.on_move_performed(mv);
        // Rust finish_enemy_turn ordering: roll, then enemy side-end hooks.
        c.roll_enemy_move(e);
        c.fire_after_turn_end(Side::Enemy);
        let ai = c.cr(e).ai.as_ref().unwrap();
        assert_eq!(ai.state_id(ai.current), awake);
        assert_eq!(ai.log.len(), old_history.len() + 1);
        assert_eq!(&ai.log[..old_history.len()], old_history.as_slice());
        assert_eq!(ai.log.last(), Some(&ai.current));
        assert_ne!(ai.used & (1 << ai.current), 0);
        assert!(!c.has_power(e, sleep_power));
        let whistle = c.new_card(0, CardId::Whistle, false).unwrap();
        c.play_card_effect(whistle, Some(e));
        let ai = c.cr_mut(e).ai.as_mut().unwrap();
        assert_eq!(ai.current, STUNNED);
        assert_eq!(ai.state_id(ai.stun_follow_up), awake);
        ai.mark_performing(STUNNED);
        ai.on_move_performed(STUNNED);
        c.roll_enemy_move(e);
        let ai = c.cr(e).ai.as_ref().unwrap();
        assert_eq!(ai.state_id(ai.current), awake);
        assert!(!ai.log.contains(&STUNNED));
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn flutter_uses_next_state_and_illusion_uses_last_logged_state_not_injected_stun() {
    let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.setup_encounter("THIEVING_HOPPER_WEAK", &mut Rng::new(7));
    let e = c.enemies[0];
    let ai = c.cr(e).ai.as_ref().unwrap();
    let initial = *ai.log.last().unwrap();
    let next = match ai.state(initial).kind {
        sts2_sim::tables::StateKind::Move { follow_up: Some(next), .. } => next,
        _ => panic!("hopper initial move must have a follow-up"),
    };
    c.apply_power_to(e, PowerId::Flutter, 1, Some(e));
    c.deal_damage(Some(0), e, 2, sts2_sim::combat::Props::MOVE);
    let ai = c.cr(e).ai.as_ref().unwrap();
    assert_eq!(ai.current, STUNNED);
    assert_eq!(ai.stun_follow_up, next);
    let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(7));
    let e = c.enemies[0];
    c.apply_power_to(e, PowerId::Illusion, 1, Some(e));
    let canonical = c.cr(e).ai.as_ref().unwrap().current;
    c.stun_enemy(e);
    let ai = c.cr_mut(e).ai.as_mut().unwrap();
    ai.mark_performing(STUNNED);
    ai.on_move_performed(STUNNED);
    c.kill_creature(e);
    let ai = c.cr(e).ai.as_ref().unwrap();
    assert_eq!(ai.current, sts2_sim::monster::ILLUSION_REVIVE);
    assert_eq!(ai.revive_follow_up, canonical);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn natural_wake_after_performed_sleep_stun_logs_sleep_once_before_awake_roll() {
    for (enc, monster, power, sleep, awake) in [
        ("SLUMBERING_BEETLE_NORMAL", "SLUMBERING_BEETLE", PowerId::Slumber, "SNORE_MOVE", "ROLL_OUT_MOVE"),
        ("LAGAVULIN_MATRIARCH_BOSS", "LAGAVULIN_MATRIARCH", PowerId::Asleep, "SLEEP_MOVE", "SLASH_MOVE"),
    ] {
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        c.setup_encounter(enc, &mut Rng::new(7));
        let e = *c.enemies.iter().find(|&&e| c.cr(e).monster_id == monster).unwrap();
        let p = c.find_power(e, power).unwrap();
        c.powers[p as usize].amount = 1;
        let before = c.cr(e).ai.as_ref().unwrap().log.clone();
        assert!(c.stun_enemy(e));
        let ai = c.cr_mut(e).ai.as_mut().unwrap();
        ai.mark_performing(STUNNED);
        ai.on_move_performed(STUNNED);
        c.roll_enemy_move(e); // Rust rolls before natural wake.
        c.fire_after_turn_end(Side::Enemy);
        let ai = c.cr_mut(e).ai.as_mut().unwrap();
        assert_eq!(ai.state_id(ai.current), sleep);
        assert_eq!(ai.log.len(), before.len() + 1);
        assert_eq!(&ai.log[..before.len()], before.as_slice());
        assert_eq!(ai.log.last(), Some(&ai.current));
        let slept = ai.current;
        ai.mark_performing(slept);
        ai.on_move_performed(slept);
        c.roll_enemy_move(e);
        let ai = c.cr(e).ai.as_ref().unwrap();
        assert_eq!(ai.state_id(ai.current), awake);
        assert_eq!(ai.log.len(), before.len() + 2);
        assert_eq!(ai.log.last(), Some(&ai.current));
        assert!(!ai.log.contains(&STUNNED));
        assert!(!c.has_power(e, power));
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn natural_wake_preserves_an_unperformed_first_move_stun() {
    for (enc, monster, power) in [
        ("SLUMBERING_BEETLE_NORMAL", "SLUMBERING_BEETLE", PowerId::Slumber),
        ("LAGAVULIN_MATRIARCH_BOSS", "LAGAVULIN_MATRIARCH", PowerId::Asleep),
    ] {
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        c.setup_encounter(enc, &mut Rng::new(7));
        let e = *c.enemies.iter().find(|&&e| c.cr(e).monster_id == monster).unwrap();
        let p = c.find_power(e, power).unwrap();
        c.powers[p as usize].amount = 1;
        assert!(!c.cr(e).ai.as_ref().unwrap().performed_first);
        let history = c.cr(e).ai.as_ref().unwrap().log.clone();
        assert!(c.stun_enemy(e));
        let follow_up = c.cr(e).ai.as_ref().unwrap().stun_follow_up;
        assert!(!c.cr(e).ai.as_ref().unwrap().performed_first);
        c.fire_after_turn_end(Side::Enemy);
        let ai = c.cr(e).ai.as_ref().unwrap();
        assert_eq!(ai.current, STUNNED);
        assert_eq!(ai.stun_follow_up, follow_up);
        assert_eq!(ai.log, history);
        c.roll_enemy_move(e);
        assert_eq!(c.cr(e).ai.as_ref().unwrap().current, STUNNED);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
