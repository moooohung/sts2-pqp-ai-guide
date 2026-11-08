//! C# Coordinate.OnPlay requires a non-null target; direct/autoplay effects must not guess.
use sts2_sim::combat::Combat;
use sts2_sim::rng::Rng;
use sts2_sim::run::{RunManager, RunOptions};
use sts2_sim::tables::{CardId, PowerId};

#[test]
fn coordinate_without_target_fails_closed_without_buffing_its_owner() {
    let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.add_player(80, 80, 3, 0, &[]);
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(7));
    let k = c.new_card(0, CardId::CoordinateCard, false).unwrap();
    c.play_card_effect(k, None);
    assert_eq!(c.fault, Some("Coordinate requires an ally target"));
    for who in 0..2 {
        assert_eq!(c.power_amount(who, PowerId::Coordinate), 0);
        assert_eq!(c.power_amount(who, PowerId::Strength), 0);
    }
}
