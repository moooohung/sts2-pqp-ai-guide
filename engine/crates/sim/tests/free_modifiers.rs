use sts2_sim::{
    combat::{Combat, Props}, rng::Rng, run::{RunManager, RunOptions},
    tables::{CardId, PowerId},
};

fn combat() -> Combat {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.players[0].hand.clear();
    c.players[0].draw.clear();
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    let e = c.enemies[0];
    c.cr_mut(e).hp = 1000;
    c.cr_mut(e).max_hp = 1000;
    c
}

#[test]
fn gigantification_triples_complete_multihit_attack_then_consumes_one_stack() {
    let mut c = combat();
    let e = c.enemies[0];
    let k = c.new_card(0, CardId::TwinStrike, false).unwrap();
    c.players[0].hand.push(k);
    c.players[0].energy = 3;
    c.apply_power_to(0, PowerId::Gigantification, 2, Some(0));
    assert!(c.play_card(0, 0, Some(0)));
    assert_eq!(c.cr(e).hp, 1000 - 2 * 5 * 3);
    assert_eq!(c.power_amount(0, PowerId::Gigantification), 1);
    assert_eq!(c.fault, None);
}

#[test]
fn gigantification_binds_card_owner_not_dealer_and_ignores_unpowered_damage() {
    let mut c = combat();
    let e = c.enemies[0];
    let k = c.new_card(0, CardId::StrikeNecrobinder, false).unwrap();
    c.active_card = Some(k);
    c.apply_power_to(0, PowerId::Gigantification, 1, Some(0));
    assert_eq!(c.modify_damage(7, Some(0), e, Props::MOVE, Some(k)), 21);
    assert_eq!(c.modify_damage(7, Some(0), e, Props::UNPOWERED, Some(k)), 7);
    assert_eq!(c.power_amount(0, PowerId::Gigantification), 1);
    assert_eq!(c.fault, None);
}

#[test]
fn ambergris_solo_grants_same_round_extra_turn_and_decrements_once() {
    let mut c = combat();
    c.players[0].energy = 3;
    let before = c.round;
    c.apply_power_to(0, PowerId::Ambergris, 2, Some(0));
    c.end_player_turn();
    assert_eq!(c.round, before);
    assert_eq!(c.power_amount(0, PowerId::Ambergris), 1);
    assert_eq!(c.fault, None);
}

#[test]
fn ambergris_multiplayer_restarts_only_its_recipient() {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.add_player(50, 50, 3, 0, &[]);
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    c.start_combat();
    let round = c.round;
    let turns = [c.players[0].turn_number, c.players[1].turn_number];
    c.apply_power_to(1, PowerId::Ambergris, 1, Some(1));
    c.end_player_turn();
    assert_eq!(c.round, round);
    assert_eq!(c.players[0].turn_number, turns[0]);
    assert_eq!(c.players[1].turn_number, turns[1] + 1);
    assert_eq!(c.fault, None);
}
