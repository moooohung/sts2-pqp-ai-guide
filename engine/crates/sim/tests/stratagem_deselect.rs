use sts2_sim::{combat::Combat, rng::Rng, run::{RunManager, RunOptions}, tables::{CardId, PowerId}};

#[test]
fn stratagem_deselect_fails_closed_before_losing_hashset_order() {
    for owner in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        for _ in 0..4 {
            let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.players[owner].discard.push(k);
        }
        c.apply_power_to(owner, PowerId::Stratagem, 2, Some(owner));
        c.shuffle_if_needed(owner);
        assert!(c.resolve_pending_choice(Some(0)));
        assert!(c.resolve_pending_choice(Some(1)));
        let selected = c.choice_selected.clone();
        assert!(!c.resolve_pending_choice(Some(0)));
        assert_eq!(c.choice_selected, selected);
        assert_eq!(c.fault, Some("Stratagem deselection requires HashSet slot order"));
    }
}
