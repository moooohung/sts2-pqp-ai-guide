use sts2_sim::{
    combat::Combat, rng::Rng, run::{RunManager, RunOptions},
    tables::{CardId, PowerId},
};

#[test]
fn capacity_monologue_remembers_every_nested_havoc_until_its_play_finishes() {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.players[0].hand.clear();
    c.players[0].draw.clear();
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    let enemy = c.enemies[0];
    c.creatures[enemy as usize].hp = 1000;
    c.creatures[enemy as usize].max_hp = 1000;
    let monologue = c.apply_power_to(0, PowerId::Monologue, 1, Some(0)).unwrap();
    for _ in 0..2 {
        let k = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        c.players[0].discard.push(k);
    }
    for _ in 0..3 {
        let k = c.new_card(0, CardId::Havoc, false).unwrap();
        c.players[0].draw.push(k);
    }
    let headbutt = c.new_card(0, CardId::Headbutt, false).unwrap();
    c.players[0].draw.push(headbutt);
    let havoc = c.new_card(0, CardId::Havoc, false).unwrap();
    c.players[0].hand.push(havoc);
    c.auto_play_card(havoc, None, false);
    assert_eq!(c.fault, None);
    assert_eq!(c.hist.play_start_cards.len(), 5);
    assert_eq!(c.power_amount(0, PowerId::Strength), 0);
    assert!(c.pending_choice_for_player(0).is_some());
    assert!(c.resolve_pending_choice_for_player(0, Some(0)));
    assert_eq!(c.fault, None);
    assert_eq!(c.power_amount(0, PowerId::Strength), 5);
    assert_eq!(c.powers[monologue as usize].amount, 5);
    c.end_player_turn();
    assert_eq!(c.fault, None);
    assert_eq!(c.power_amount(0, PowerId::Strength), 0);
}
