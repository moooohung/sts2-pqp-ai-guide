use sts2_sim::{combat::Combat, rng::Rng, run::{RunManager, RunOptions}, tables::PowerId};

#[test]
fn anticipate_applies_dexterity_before_attachment_and_stacks_once_for_every_owner() {
    for owner in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();

        c.apply_power_to(owner, PowerId::Anticipate, 3, Some(owner));
        let dexterity = c.find_power(owner, PowerId::Dexterity).unwrap();
        let wrapper = c.find_power(owner, PowerId::Anticipate).unwrap();
        let order = c.cr(owner).power_list();
        assert!(order.iter().position(|&p| p == dexterity) < order.iter().position(|&p| p == wrapper));
        assert_eq!(c.powers[dexterity as usize].amount, 3);
        assert_eq!(c.powers[wrapper as usize].amount, 3);

        c.apply_power_to(owner, PowerId::Anticipate, 4, Some(owner));
        assert_eq!(c.powers[dexterity as usize].amount, 7);
        assert_eq!(c.powers[wrapper as usize].amount, 7);
        assert_eq!(c.fault, None);
    }
}
