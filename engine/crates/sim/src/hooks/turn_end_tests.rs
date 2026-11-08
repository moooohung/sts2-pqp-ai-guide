use super::*;
use crate::{
    choice::{ChoiceSrc, PendingChoice, Resolver},
    combat::TurnStage,
    run::{RunManager, RunOptions},
};

fn combat() -> Combat {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    Combat::new(run, 1)
}

#[test]
fn sequential_before_side_end_tiers_wait_without_advancing_later_listeners() {
    for tier in 1..=3 {
        let mut c = combat();
        // Legacy recordings intentionally retain synchronous selector scheduling.
        c.run.choice_scheduling = crate::combat::ChoiceScheduling::Sequential;
        let card = c.new_card(0, CardId::Headbutt, false).unwrap();
        let bomb = c.apply_power_to(0, PowerId::TheBomb, 3, Some(0)).unwrap();
        c.powers[bomb as usize].amount = 3;
        c.pending_choice = Some(PendingChoice {
            player: 0,
            resolver: Resolver { src: ChoiceSrc::Card(CardId::Headbutt), k: card,
                target: None, step: 0, arg: 0 },
            source: PileKind::Discard, allow_skip: false, min: 1, max: 1, single: true,
        });
        c.pending_turn_end_hooks = Some(TurnEndWalk {
            auto_post_owner: 0, tier, loaded: true,
            listeners: vec![TurnStartListener::Power(0, bomb)], next: 0,
            stampede: None, detached: false, resume_stage: TurnStage::PlayerTurnEnd,
        });
        let round = c.round;
        c.resume_turn_end_hooks();
        assert_eq!(c.fault, None);
        assert_eq!(c.powers[bomb as usize].amount, 3);
        assert!(c.pending_choice.is_some());
        assert_eq!(c.round, round);
        assert_eq!(c.side, Side::Player);
    }
}

#[test]
fn stampede_finishes_remaining_shuffle_samples_after_over_or_ending() {
    for over in [false, true] {
        let mut c = combat();
        c.players[0].hand.clear();
        c.players[0].draw.clear();
        let card = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
        c.players[0].hand.push(card);
        let p = c.apply_power_to(0, PowerId::Stampede, 3, Some(0)).unwrap();
        let mut expected = c.run.rngs.shuffle.clone();
        for _ in 1..3 { expected.choice_index(1); }
        // The first autoplay has completed. Its callback still has two iterations,
        // even though CardCmd.AutoPlay now refuses every subsequent attempt.
        c.is_over = over;
        c.is_ending = !over;
        c.pending_turn_end_hooks = Some(TurnEndWalk {
            auto_post_owner: 0, tier: 0, loaded: true, listeners: Vec::new(), next: 0,
            stampede: Some((p, 0, 1)), detached: false, resume_stage: TurnStage::PlayerTurnEnd,
        });
        let targets_before = c.run.rngs.combat_targets.counter;
        c.resume_turn_end_hooks();
        assert_eq!(c.run.rngs.shuffle.counter, expected.counter);
        assert_eq!(c.run.rngs.combat_targets.counter, targets_before);
        assert_eq!(c.players[0].hand.as_slice(), &[card]);
        assert!(c.players[0].play.is_empty());
        assert!(c.pending_play.is_empty());
        assert_eq!(c.fault, None);
    }
}

#[test]
fn an_ending_combat_does_not_start_a_new_turn_end_listener() {
    let mut c = combat();
    let bomb = c.apply_power_to(0, PowerId::TheBomb, 3, Some(0)).unwrap();
    c.powers[bomb as usize].amount = 3;
    c.pending_turn_end_hooks = Some(TurnEndWalk {
        auto_post_owner: 0, tier: 3, loaded: true,
        listeners: vec![TurnStartListener::Power(0, bomb)], next: 0,
        stampede: None, detached: false, resume_stage: TurnStage::PlayerTurnEnd,
    });
    c.is_ending = true;
    c.resume_turn_end_hooks();
    assert_eq!(c.powers[bomb as usize].amount, 3);
    assert_eq!(c.fault, None);
}

#[test]
fn live_before_side_end_assigns_later_tiers_before_joining_a_paused_callback() {
    let mut c = combat();
    c.run.choice_scheduling = crate::combat::ChoiceScheduling::Live;
    let k = c.new_card(0, CardId::Headbutt, false).unwrap();
    c.apply_power_to(0, PowerId::Plating, 7, Some(0));
    let bomb = c.apply_power_to(0, PowerId::TheBomb, 3, Some(0)).unwrap();
    c.pending_choice = Some(PendingChoice {
        player: 0, resolver: Resolver { src: ChoiceSrc::Card(CardId::Headbutt),
            k, target: None, step: 0, arg: 0 },
        source: PileKind::Discard, allow_skip: false, min: 1, max: 1, single: true,
    });
    // Checkpoint immediately after a VeryEarly callback paused. Hook.cs:955-976
    // assigns Early and normal callbacks before awaiting that first task.
    c.pending_turn_end_hooks = Some(TurnEndWalk {
        auto_post_owner: 0, tier: 1, loaded: true, listeners: Vec::new(), next: 0,
        stampede: None, detached: false, resume_stage: TurnStage::PlayerTurnEnd,
    });
    c.resume_turn_end_hooks();
    assert_eq!(c.fault, None);
    assert_eq!(c.cr(0).block, 7);
    assert_eq!(c.powers[bomb as usize].amount, 2);
    assert!(c.pending_choice_for_player(0).is_some());
    assert!(c.turn_end_join.is_some());
    assert_eq!(c.side, Side::Player);
}
