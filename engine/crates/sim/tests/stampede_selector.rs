use sts2_sim::{
    combat::Combat, creature::Side, rng::Rng,
    run::{RunManager, RunOptions}, tables::{CardId, PowerId},
};

fn combat() -> Combat {
    combat_with_players(4)
}

fn combat_with_players(count: usize) -> Combat {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.players[0].hand.clear();
    c.players[0].draw.clear();
    for _ in 1..count { c.add_player(1000, 1000, 3, 0, &[]); }
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    let e = c.enemies[0];
    c.creatures[e as usize].hp = 1000;
    c.creatures[e as usize].max_hp = 1000;
    c
}

#[test]
fn every_seat_awaits_first_or_last_autoplay_and_later_listener_once() {
    for owner in 0..4 {
        for selector_first in [true, false] {
            let mut c = combat();
            let first_index = c.run.rngs.shuffle.clone().choice_index(2);
            let headbutt = c.new_card(owner, CardId::Headbutt, false).unwrap();
            let strike = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
            let mut hand = [strike; 2];
            hand[if selector_first { first_index } else { 1 - first_index }] = headbutt;
            c.players[owner].hand.extend(hand);
            for _ in 0..2 {
                let q = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
                c.players[owner].discard.push(q);
            }
            c.apply_power_to(owner, PowerId::Stampede, 2, Some(owner));
            let bomb = c.apply_power_to(owner, PowerId::TheBomb, 3, Some(owner)).unwrap();
            c.powers[bomb as usize].amount = 3;
            let round = c.round;
            c.end_player_turn();
            assert_eq!(c.fault, None);
            assert_eq!(c.pending_choice.as_ref().unwrap().player, owner);
            assert_eq!(c.players[owner].hand.len(), usize::from(selector_first));
            assert_eq!(c.powers[bomb as usize].amount, 3);
            assert_eq!(c.side, Side::Player);
            assert_eq!(c.round, round);
            assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
            assert_eq!(c.fault, None);
            assert!(c.pending_choice.is_none());
            assert!(c.pending_play.is_empty());
            assert!(c.players[owner].play.is_empty());
            assert_eq!(c.powers[bomb as usize].amount, 2);
            assert_eq!(c.round, round + 1);
        }
    }
}

#[test]
fn two_stampede_instances_resume_independently_in_the_listener_snapshot() {
    for owner in 0..4 {
        let mut c = combat();
        for _ in 0..2 {
            let k = c.new_card(owner, CardId::Headbutt, false).unwrap();
            c.players[owner].hand.push(k);
            let q = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.players[owner].discard.push(q);
            c.creature_apply_power(owner, PowerId::Stampede, 1, Some(owner), None, false, true);
        }
        let bomb = c.apply_power_to(owner, PowerId::TheBomb, 3, Some(owner)).unwrap();
        c.powers[bomb as usize].amount = 3;
        let round = c.round;
        c.end_player_turn();
        assert_eq!(c.fault, None);
        assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
        assert_eq!(c.powers[bomb as usize].amount, 3);
        assert!(c.pending_choice.is_some());
        assert_eq!(c.round, round);
        assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
        assert!(c.pending_choice.is_none());
        assert!(c.pending_play.is_empty());
        assert_eq!(c.powers[bomb as usize].amount, 2);
        assert_eq!(c.round, round + 1);
        assert_eq!(c.fault, None);
    }
}

#[test]
fn sequential_stampede_waits_before_the_later_seats_shared_rng() {
    let mut c = combat_with_players(2);
    c.run.choice_scheduling = sts2_sim::combat::ChoiceScheduling::Sequential;
    let first_index = c.run.rngs.shuffle.clone().choice_index(2);
    let headbutt = c.new_card(0, CardId::Headbutt, false).unwrap();
    let strike = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
    let mut hand = [strike; 2];
    hand[first_index] = headbutt;
    c.players[0].hand.extend(hand);
    for _ in 0..2 {
        let q = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        c.players[0].discard.push(q);
        let q = c.new_card(1, CardId::Bash, false).unwrap();
        c.players[1].hand.push(q);
    }
    for owner in 0..2 { c.apply_power_to(owner, PowerId::Stampede, 2, Some(owner)); }
    let mut expected_shuffle = c.run.rngs.shuffle.clone();
    expected_shuffle.choice_index(2); // only A1; never serially consume A2 before B1
    let round = c.round;
    let bomb = c.apply_power_to(0, PowerId::TheBomb, 3, Some(0)).unwrap();
    c.powers[bomb as usize].amount = 3;
    c.end_player_turn();
    assert_eq!(c.fault, None);
    assert_eq!(c.pending_choice.as_ref().unwrap().player, 0);
    assert_eq!(c.players[0].hand.len(), 1);
    assert_eq!(c.players[1].hand.len(), 2);
    assert!(c.players[1].play.is_empty());
    assert_eq!(c.run.rngs.shuffle.counter, expected_shuffle.counter);
    assert_eq!(c.powers[bomb as usize].amount, 3);
    assert_eq!(c.side, Side::Player);
    assert_eq!(c.round, round);
    assert!(c.resolve_pending_choice_for_player(0, Some(0)));
    assert_eq!(c.fault, None);
    assert!(c.pending_choice.is_none());
    assert_eq!(c.powers[bomb as usize].amount, 2);
    assert_eq!(c.round, round + 1);
}
