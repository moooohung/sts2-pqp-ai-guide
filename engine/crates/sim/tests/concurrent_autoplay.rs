use sts2_sim::{
    combat::{Combat, ChoiceScheduling}, creature::Side, rng::Rng,
    run::{RunManager, RunOptions}, tables::{CardId, PowerId},
};

fn combat(count: usize) -> Combat {
    let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    run.choice_scheduling = ChoiceScheduling::Live;
    let mut c = Combat::new(run, 1);
    c.players[0].hand.clear();
    c.players[0].draw.clear();
    for _ in 1..count { c.add_player(1000, 1000, 3, 0, &[]); }
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    let enemy = c.enemies[0];
    c.creatures[enemy as usize].hp = 1000;
    c.creatures[enemy as usize].max_hp = 1000;
    c
}

#[test]
fn lethal_later_listener_still_joins_the_earlier_stampede_and_its_shuffle_draws() {
    for scheduling in [ChoiceScheduling::Live, ChoiceScheduling::Sequential] {
      for extra in [None, Some(CardId::HowlFromBeyond), Some(CardId::IAmInvincible)] {
        let mut c = combat(2);
        c.combat_started = true;
        c.run.choice_scheduling = scheduling;
        let enemy = c.enemies[0];
        c.creatures[enemy as usize].hp = 12;
        for _ in 0..2 {
            let k = c.new_card(0, CardId::Headbutt, false).unwrap();
            c.players[0].hand.push(k);
            let k = c.new_card(0, CardId::DefendIronclad, false).unwrap();
            c.players[0].discard.push(k);
        }
        c.apply_power_to(0, PowerId::Stampede, 2, Some(0));
        let howl = c.new_card(1, CardId::HowlFromBeyond, false).unwrap();
        c.players[1].exhaust.push(howl);
        if let Some(id) = extra {
            let k = c.new_card(0, id, false).unwrap();
            if id == CardId::HowlFromBeyond { c.players[0].exhaust.push(k); }
            else { c.players[0].draw.push(k); }
        }
        let mut rng = c.run.rngs.shuffle.clone();
        rng.choice_index(2);
        c.end_player_turn();
        assert_eq!(c.fault, None);
        assert!(!c.is_over);
        assert!(c.pending_choice_for_player(0).is_some());
        assert_eq!(c.run.rngs.shuffle.counter, rng.counter);
        if scheduling == ChoiceScheduling::Live {
            assert_eq!(c.cr(enemy).hp, 0);
            rng.choice_index(1); // in-flight Stampede samples even though AutoPlay refuses
            assert!(c.resolve_pending_choice_for_player(0, Some(0)));
        } else {
            assert_eq!(c.cr(enemy).hp, 3);
            rng.choice_index(1);
            assert!(c.resolve_pending_choice_for_player(0, Some(0)));
            // The second Headbutt kills before its selection; CardSelect refuses
            // a new selection once ending, unlike the already-open first one.
        }
        assert_eq!(c.fault, None);
        assert_eq!(c.run.rngs.shuffle.counter, rng.counter,
            "{scheduling:?}: enemy hp {}, over {}, choice {:?}", c.cr(enemy).hp, c.is_over, c.pending_choice);
        assert!(c.is_over);
      }
    }
}

#[test]
fn live_stampede_assigns_all_seats_before_resuming_either_stack() {
    for seats in 2..=4 {
        for reverse in [false, true] {
            let mut c = combat(seats);
            for owner in 0..2 {
                for _ in 0..2 {
                    let k = c.new_card(owner, CardId::Headbutt, false).unwrap();
                    c.players[owner].hand.push(k);
                    let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
                    c.players[owner].discard.push(k);
                }
                c.apply_power_to(owner, PowerId::Stampede, 2, Some(owner));
            }
            let bomb = c.apply_power_to(0, PowerId::TheBomb, 3, Some(0)).unwrap();
            c.end_player_turn();
            assert_eq!(c.fault, None);
            assert_eq!(c.hist.play_start_cards.len(), 2);
            assert_eq!(c.cards[c.hist.play_start_cards[0] as usize].owner, 0);
            assert_eq!(c.cards[c.hist.play_start_cards[1] as usize].owner, 1);
            assert!(c.pending_choice_for_player(0).is_some());
            assert!(c.pending_choice_for_player(1).is_some());
            assert_eq!(c.powers[bomb as usize].amount, 3);
            let order = if reverse { [1, 0] } else { [0, 1] };
            for owner in order {
                assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
                assert_eq!(c.fault, None);
                assert!(c.pending_choice_for_player(owner).is_some());
                assert_eq!(c.powers[bomb as usize].amount, 3);
                if owner == order[1] { assert_eq!(c.hist.play_start_cards.len(), 4); }
                assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
                assert_eq!(c.fault, None);
            }
            assert!(c.pending_choice.is_none());
            assert_eq!(c.powers[bomb as usize].amount, 2);
            assert!(c.players.0.iter().all(|p| p.play.is_empty()));
        }
    }
}

#[test]
fn live_later_seat_card_listeners_execute_while_stampede_is_parked() {
    for seats in 2..=4 {
        for id in [CardId::HowlFromBeyond, CardId::IAmInvincible] {
            let mut c = combat(seats);
            let headbutt = c.new_card(0, CardId::Headbutt, false).unwrap();
            c.players[0].hand.push(headbutt);
            for _ in 0..2 {
                let k = c.new_card(0, CardId::DefendIronclad, false).unwrap();
                c.players[0].discard.push(k);
            }
            c.apply_power_to(0, PowerId::Stampede, 2, Some(0));
            let owner = (seats - 1) as u8;
            let k = c.new_card(owner, id, false).unwrap();
            if id == CardId::HowlFromBeyond { c.players[owner].exhaust.push(k); }
            else { c.players[owner].draw.push(k); }
            c.fire_before_turn_end(Side::Player);
            assert_eq!(c.fault, None);
            assert_eq!(c.hist.play_start_cards.as_slice(), &[headbutt, k]);
            assert!(c.pending_choice_for_player(0).is_some());
            assert!(c.resolve_pending_choice_for_player(0, Some(0)));
            assert_eq!(c.fault, None);
        }
    }
}

#[test]
fn live_auto_pre_play_keeps_nested_play_stacks_separate_for_each_seat() {
    for seats in 2..=4 {
        let mut c = combat(seats);
        for owner in 0..seats as u8 {
            for _ in 0..5 {
                let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
                c.players[owner].draw.push(k);
            }
            let headbutt = c.new_card(owner, CardId::Headbutt, false).unwrap();
            c.players[owner].draw.push(headbutt);
            for _ in 0..2 {
                let k = c.new_card(owner, CardId::Bash, false).unwrap();
                c.players[owner].discard.push(k);
            }
            c.apply_power_to(owner, PowerId::Mayhem, 1, Some(owner));
        }
        c.start_player_turn();
        assert_eq!(c.fault, None);
        for owner in 0..seats as u8 {
            assert!(c.pending_choice_for_player(owner).is_some());
            assert_eq!(c.players[owner].play.len(), 1);
        }
        for owner in (0..seats as u8).rev() {
            assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
            assert_eq!(c.fault, None);
            assert!(c.players[owner].play.is_empty());
            assert!(c.pending_choice_for_player(owner).is_none());
        }
        assert!(c.pending_choice.is_none());
        assert_eq!(c.hist.play_start_cards.len(), seats);
    }
}
