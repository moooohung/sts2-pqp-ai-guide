use sts2_sim::{
    combat::Combat, rng::Rng, run::{RunManager, RunOptions}, tables::CardId,
};

#[test]
fn capacity_headbutt_can_select_the_last_card_in_a_large_discard() {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.players[0].hand.clear();
    c.players[0].draw.clear();
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    let enemy = c.enemies[0];
    c.creatures[enemy as usize].hp = 1000;
    c.creatures[enemy as usize].max_hp = 1000;
    for _ in 0..129 {
        let k = c.new_card(0, CardId::DefendIronclad, false).unwrap();
        c.players[0].discard.push(k);
    }
    let selected = *c.players[0].discard.last().unwrap();
    let headbutt = c.new_card(0, CardId::Headbutt, false).unwrap();
    c.players[0].hand.push(headbutt);
    c.auto_play_card(headbutt, Some(enemy), false);
    assert_eq!(c.fault, None);
    assert_eq!(c.choice_options.len(), 129);
    assert!(c.resolve_pending_choice_for_player(0, Some(128)));
    assert_eq!(c.fault, None);
    assert_eq!(c.players[0].draw.last(), Some(&selected));
    assert!(!c.players[0].discard.contains(&selected));
    assert!(c.players[0].discard.contains(&headbutt));
}
