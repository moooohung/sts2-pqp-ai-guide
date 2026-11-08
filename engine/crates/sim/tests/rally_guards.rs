//! C# Rally.OnPlay / CreatureCmd.GainBlock guards.
use sts2_sim::combat::Combat;
use sts2_sim::rng::Rng;
use sts2_sim::run::{RunManager, RunOptions};
use sts2_sim::tables::{CardId, PowerId};

#[test]
fn rally_skips_dead_seats_and_never_grants_block_after_combat_end() {
    for ended in [false, true] {
        let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        c.add_player(80, 80, 3, 0, &[]);
        c.add_player(80, 80, 3, 0, &[]);
        c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(7));
        c.apply_power_to(1, PowerId::BeaconOfHope, 1, Some(1));
        c.creatures[1].hp = 0;
        let k = c.new_card(0, CardId::Rally, false).unwrap();
        c.is_over = ended;
        c.play_card_effect(k, None);
        let expected = if ended { 0 } else { 12 };
        assert_eq!([c.cr(0).block, c.cr(1).block, c.cr(2).block], [expected, 0, expected]);
        assert_eq!(c.hist.block_this_turn.len(), if ended { 0 } else { 2 });
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
