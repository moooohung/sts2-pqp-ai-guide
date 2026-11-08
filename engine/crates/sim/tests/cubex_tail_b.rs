use sts2_sim::{combat::Combat, relic::Relic, rng::Rng, run::{RunManager, RunOptions},
    tables::{CardId, PowerId, RelicId}};

#[test]
fn cubex_strength_tail_awaits_all_received_callbacks_for_every_seat() {
    for owner in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.ascension = 0; // CubexConstruct.cs BlastDamage: 7 at A0, 8 at A9+.
        let mut c = Combat::new(run, 1);
        c.run.relics.clear();
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("CUBEX_CONSTRUCT_NORMAL", &mut Rng::new(1));
        let enemy = c.enemies[0];
        let mv = {
            let ai = c.cr(enemy).ai.as_ref().unwrap();
            sts2_sim::content::state_index(ai.def(), "REPEATER_BLAST_MOVE")
        };
        let mut puzzle = Relic::new(RelicId::CentennialPuzzle); puzzle.owner = owner;
        c.run.relics.push(puzzle);
        for _ in 0..5 {
            let card = c.new_card(owner, CardId::Wound, false).unwrap();
            c.players[owner].discard.push(card);
        }
        c.apply_power_to(owner, PowerId::Stratagem, 1, Some(owner));
        let hp: Vec<_> = (0..4).map(|seat| c.cr(seat).hp).collect();
        c.monster_move_effect(enemy, mv);
        assert!(c.pending_choice.is_some());
        assert_eq!(c.power_amount(enemy, PowerId::Strength), 0, "move tail awaits attack");
        for seat in 0..4 { assert_eq!(c.cr(seat).hp, hp[seat as usize] - 7); }
        assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
        assert_eq!(c.power_amount(enemy, PowerId::Strength), 2);
        for seat in 0..4 { assert_eq!(c.cr(seat).hp, hp[seat as usize] - 7); }
        assert_eq!(c.fault, None);
    }
}
