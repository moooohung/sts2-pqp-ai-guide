use sts2_sim::{combat::Combat, rng::Rng, run::{RunManager, RunOptions}, tables::PowerId};

#[test]
fn temporary_focus_before_applied_precedes_wrapper_for_every_owner_and_stacks_once() {
    for id in [PowerId::Hotfix, PowerId::FocusedStrike, PowerId::Synchronize] {
        for owner in 0..4 {
            let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            run.deck.clear();
            let mut c = Combat::new(run, 1);
            for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
            c.start_combat();
            c.apply_power_to(owner, id, 2, Some(owner));
            let focus = c.find_power(owner, PowerId::Focus).unwrap();
            let wrapper = c.find_power(owner, id).unwrap();
            let order = c.cr(owner).power_list();
            assert!(order.iter().position(|&p| p == focus) < order.iter().position(|&p| p == wrapper));
            assert_eq!(c.powers[focus as usize].amount, 2);
            c.apply_power_to(owner, id, 3, Some(owner));
            assert_eq!(c.powers[focus as usize].amount, 5);
            assert_eq!(c.powers[wrapper as usize].amount, 5);
            assert_eq!(c.fault, None);
        }
    }
}
