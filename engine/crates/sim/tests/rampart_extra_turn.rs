use sts2_sim::{combat::Combat, creature::Side, rng::Rng, run::{RunManager, RunOptions}, tables::PowerId};

#[test]
fn rampart_skips_extra_turns_for_every_seat_but_blocks_on_normal_turns() {
    for seat in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("TURRET_OPERATOR_WEAK", &mut Rng::new(1));
        c.start_combat();
        let shield = c.enemies[0];
        let turret = c.enemies[1];
        assert_eq!(c.cr(turret).monster_id, "TURRET_OPERATOR");
        let p = c.find_power(shield, PowerId::Rampart).unwrap();
        c.powers[p as usize].amount = 7;
        c.cr_mut(turret).block = 0;
        c.extra_turn_players = 1 << seat;
        c.fire_after_side_turn_start(Side::Player);
        assert_eq!(c.cr(turret).block, 0);
        c.extra_turn_players = 0;
        c.fire_after_side_turn_start(Side::Player);
        assert_eq!(c.cr(turret).block, 7);
        assert_eq!(c.fault, None);
    }
}
