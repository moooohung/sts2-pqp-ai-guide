use sts2_sim::{
    combat::Combat, creature::Side, rng::Rng, run::{RunManager, RunOptions},
    tables::{CardId, PowerId},
};

fn combat() -> Combat {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.players[0].hand.clear();
    c.players[0].draw.clear();
    for _ in 1..4 { c.add_player(1000, 1000, 3, 0, &[]); }
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    let enemy = c.enemies[0];
    c.creatures[enemy as usize].hp = 1000;
    c.creatures[enemy as usize].max_hp = 1000;
    c
}

#[test]
fn invincible_runs_in_auto_post_play_only_when_it_tops_its_owners_draw() {
    for owner in 0..4 {
        let mut c = combat();
        let top = c.new_card(owner, CardId::IAmInvincible, false).unwrap();
        let separator = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        let buried = c.new_card(owner, CardId::IAmInvincible, false).unwrap();
        c.players[owner].draw.extend([top, separator, buried]);
        let bomb = c.apply_power_to(owner, PowerId::TheBomb, 3, Some(owner)).unwrap();
        c.powers[bomb as usize].amount = 3;
        c.fire_before_turn_end(Side::Player);
        assert_eq!(c.cr(owner).block, 10);
        assert!(c.players[owner].discard.contains(&top));
        assert_eq!(c.players[owner].draw.as_slice(), &[separator, buried]);
        assert_eq!(c.hist.play_start_cards.as_slice(), &[top]);
        assert_eq!(c.powers[bomb as usize].amount, 2);
        assert_eq!(c.fault, None);
    }
}

fn pausing_stampede(c: &mut Combat) {
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
    }
    c.apply_power_to(0, PowerId::Stampede, 2, Some(0));
}

#[test]
fn sequential_later_seat_invincible_waits_for_the_current_selector() {
    let mut c = combat();
    pausing_stampede(&mut c);
    let card = c.new_card(1, CardId::IAmInvincible, false).unwrap();
    c.players[1].draw.push(card);
    let mut expected_shuffle = c.run.rngs.shuffle.clone();
    expected_shuffle.choice_index(2);
    c.end_player_turn();
    assert_eq!(c.fault, None);
    assert_eq!(c.pending_choice.as_ref().unwrap().player, 0);
    assert_eq!(c.players[1].draw.as_slice(), &[card]);
    assert_eq!(c.cr(1).block, 0);
    assert_eq!(c.run.rngs.shuffle.counter, expected_shuffle.counter);
}

#[test]
fn howl_runs_from_exhaust_in_auto_post_play_not_before_hand_draw() {
    for owner in 0..4 {
        let mut c = combat();
        let howl = c.new_card(owner, CardId::HowlFromBeyond, false).unwrap();
        let other = c.new_card(owner, CardId::HowlFromBeyond, false).unwrap();
        c.players[owner].exhaust.push(howl);
        c.players[owner].hand.push(other);
        let enemy = c.enemies[0];
        c.fire_card_before_hand_draw(owner);
        assert_eq!(c.cr(enemy).hp, 1000);
        assert!(c.hist.play_start_cards.is_empty());
        c.fire_before_turn_end(Side::Player);
        assert_eq!(c.cr(enemy).hp, 982);
        assert_eq!(c.hist.play_start_cards.as_slice(), &[howl]);
        assert!(c.players[owner].hand.contains(&other));
        assert!(c.players[owner].play.is_empty());
        c.fire_card_before_hand_draw(owner);
        assert_eq!(c.cr(enemy).hp, 982);
        assert_eq!(c.fault, None);
    }
}

#[test]
fn sequential_later_seat_howl_waits_for_the_current_selector() {
    let mut c = combat();
    pausing_stampede(&mut c);
    let howl = c.new_card(1, CardId::HowlFromBeyond, false).unwrap();
    c.players[1].exhaust.push(howl);
    let mut expected_shuffle = c.run.rngs.shuffle.clone();
    expected_shuffle.choice_index(2);
    c.end_player_turn();
    assert_eq!(c.fault, None);
    assert_eq!(c.pending_choice.as_ref().unwrap().player, 0);
    assert_eq!(c.players[1].exhaust.as_slice(), &[howl]);
    assert_eq!(c.run.rngs.shuffle.counter, expected_shuffle.counter);
}
