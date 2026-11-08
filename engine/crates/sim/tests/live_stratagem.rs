use sts2_sim::{combat::{Combat, ChoiceScheduling}, rng::Rng, run::{RunManager, RunOptions}, tables::{CardId, PowerId}};

#[test]
fn live_shuffle_waits_for_each_seat_without_drawing_ahead() {
    for focus in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        run.choice_scheduling = ChoiceScheduling::Live;
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        for p in 0..4 {
            for _ in 0..6 {
                let k = c.new_card(p, CardId::StrikeIronclad, false).unwrap();
                if p == focus { c.players[p].discard.push(k); }
                else { c.players[p].draw.push(k); }
            }
        }
        c.apply_power_to(focus, PowerId::Stratagem, 1, Some(focus));
        c.end_player_turn();
        eprintln!("Stratagem seat={focus} fault={:?}", c.fault);
        assert_eq!(c.fault, None);
        assert_eq!(c.pending_choice.as_ref().unwrap().player, focus);
        assert!(c.players[focus].hand.is_empty());
        for p in 0..4 {
            if p != focus { assert_eq!(c.players[p].hand.len(), 5); }
        }
        assert!(c.resolve_pending_choice(Some(0)));
        if c.pending_choice.is_some() { assert!(c.resolve_pending_choice(None)); }
        assert_eq!(c.players[focus].hand.len(), 6);
        assert_eq!(c.fault, None);
    }
}
