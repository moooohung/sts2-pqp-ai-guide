use sts2_sim::{combat::Combat, rng::Rng, run::{RunManager, RunOptions}, tables::{CardId, PowerId}};

#[test]
fn misery_clones_temporary_loss_without_double_subtraction_for_every_seat() {
    for owner in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
        c.start_combat();
        for e in c.enemies.clone() { c.cr_mut(e).hp = 200; c.cr_mut(e).max_hp = 200; }
        let source = c.enemies[0]; let fresh = c.enemies[1]; let stacked = c.enemies[2];
        c.apply_power_to(source, PowerId::Strength, -2, Some(owner));
        let p = c.apply_power_to(source, PowerId::DarkShackles, 6, Some(owner)).unwrap();
        c.apply_power_to(stacked, PowerId::DarkShackles, 2, Some(owner));
        let k = c.new_card(owner, CardId::Misery, false).unwrap();
        c.players[owner].hand.push(k);
        assert!(c.play_card(owner, 0, Some(0)));
        assert_eq!(c.power_amount(fresh, PowerId::Strength), -8);
        assert_eq!(c.power_amount(stacked, PowerId::Strength), -10);
        let copied = c.find_power(fresh, PowerId::DarkShackles).unwrap();
        assert_eq!(c.power_amount(fresh, PowerId::DarkShackles), c.powers[p as usize].amount);
        assert_eq!(c.powers[copied as usize].applier, Some(owner));
        assert_eq!(c.fault, None);
    }
}

#[test]
fn misery_copies_slow_count_only_into_new_instances_for_every_owner() {
    for owner in 0..4 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
        c.start_combat();
        assert_eq!(c.enemies.len(), 3);
        let source = c.enemies[0];
        let fresh = c.enemies[1];
        let stacked = c.enemies[2];
        let p = c.apply_power_to(source, PowerId::Slow, 1, Some(owner)).unwrap();
        c.powers[p as usize].vars[0] = 4;
        let q = c.apply_power_to(stacked, PowerId::Slow, 2, Some(owner)).unwrap();
        c.powers[q as usize].vars[0] = 9;
        let card = c.new_card(owner, CardId::Misery, false).unwrap();
        c.players[owner].hand.push(card);
        assert!(c.play_card(owner, 0, Some(0)));
        assert_eq!(c.fault, None);
        let cloned = c.find_power(fresh, PowerId::Slow).unwrap();
        assert_eq!(c.powers[cloned as usize].vars[0], 5); // 4 cloned, then AfterCardPlayed
        assert_eq!(c.powers[q as usize].vars[0], 10); // existing 9, then AfterCardPlayed
        assert_eq!(c.power_amount(stacked, PowerId::Slow), 3);
    }
}
