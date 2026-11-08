//! C# Creature.StunInternal and MonsterModel.SetMoveImmediate replacement guards.
use sts2_sim::combat::{Combat, Props};
use sts2_sim::monster::{STUNNED, StunMove};
use sts2_sim::rng::Rng;
use sts2_sim::run::{RunManager, RunOptions};
use sts2_sim::tables::{CardId, PowerId};

fn encounter(name: &str) -> Combat {
    let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.setup_encounter(name, &mut Rng::new(7));
    c
}

#[test]
fn whistle_cannot_erase_an_unperformed_beetle_wake_up() {
    let mut c = encounter("SLUMBERING_BEETLE_NORMAL");
    let e = *c.enemies.iter().find(|&&e| c.cr(e).monster_id == "SLUMBERING_BEETLE").unwrap();
    // Wake the beetle without killing it or bypassing its damage hooks.
    while c.has_power(e, PowerId::Slumber) {
        c.deal_damage(Some(0), e, 1, Props::UNPOWERED);
    }
    assert!(c.has_power(e, PowerId::Plating));
    let ai = c.cr(e).ai.as_ref().unwrap();
    assert_eq!(ai.current, STUNNED);
    assert_eq!(ai.stun_move, StunMove::BeetleWakeUp);
    let follow = ai.stun_follow_up;
    let whistle = c.new_card(0, CardId::Whistle, false).unwrap();
    c.play_card_effect(whistle, Some(e));
    let ai = c.cr(e).ai.as_ref().unwrap();
    assert_eq!(ai.stun_move, StunMove::BeetleWakeUp);
    assert_eq!(ai.stun_follow_up, follow);
    c.end_player_turn();
    assert!(!c.has_power(e, PowerId::Plating));
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn plow_cannot_replace_whistles_pending_stun_follow_up() {
    let mut c = encounter("CEREMONIAL_BEAST_BOSS");
    let e = c.enemies[0];
    let whistle = c.new_card(0, CardId::Whistle, false).unwrap();
    c.play_card_effect(whistle, Some(e));
    let ai = c.cr(e).ai.as_ref().unwrap();
    assert_eq!(ai.current, STUNNED);
    let follow = ai.stun_follow_up;
    c.apply_power_to(e, PowerId::Plow, c.cr(e).hp, Some(e));
    c.deal_damage(Some(0), e, 1, Props::UNPOWERED);
    let ai = c.cr(e).ai.as_ref().unwrap();
    assert_eq!(ai.stun_follow_up, follow);
    assert_eq!(ai.stun_move, StunMove::Nothing);
    assert!(!c.has_power(e, PowerId::Plow));
    assert!(c.fault.is_none(), "{:?}", c.fault);
}
