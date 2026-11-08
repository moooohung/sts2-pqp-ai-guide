use sts2_sim::{combat::Combat, rng::Rng, run::{RunManager, RunOptions}, tables::PowerId};

#[test]
fn setup_strike_applies_strength_before_attachment_and_stacks_once_for_every_owner() {
    for owner in 0..4 {
        for initial in [0, 1] {
            let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            run.deck.clear();
            let mut c = Combat::new(run, 1);
            for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
            c.start_combat();
            if initial > 0 { c.apply_power_to(owner, PowerId::Strength, initial, Some(owner)); }
            c.apply_power_to(owner, PowerId::SetupStrike, 3, Some(owner));
            let strength = c.find_power(owner, PowerId::Strength).unwrap();
            let wrapper = c.find_power(owner, PowerId::SetupStrike).unwrap();
            let order = c.cr(owner).power_list();
            assert!(order.iter().position(|&p| p == strength) < order.iter().position(|&p| p == wrapper));
            assert_eq!(c.powers[strength as usize].amount, initial + 3);
            c.apply_power_to(owner, PowerId::SetupStrike, 4, Some(owner));
            assert_eq!(c.powers[strength as usize].amount, initial + 7);
            assert_eq!(c.powers[wrapper as usize].amount, 7);
            assert_eq!(c.fault, None);
        }
    }
}
