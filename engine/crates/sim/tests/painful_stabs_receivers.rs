use sts2_sim::{
    combat::{AttackCtx, Combat, Props}, rng::Rng,
    run::{RunManager, RunOptions}, tables::{CardId, PowerId},
};

#[test]
fn painful_stabs_counts_each_receivers_unblocked_hits_not_the_command_target() {
    // PainfulStabsPower.cs:34-63 flattens AttackCommand.Results, groups in
    // first-receiver order (including blocked results), then counts positive hits.
    for seats in 2..=4 {
        let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        let mut c = Combat::new(run, 1);
        for _ in 1..seats { c.add_player(1000, 1000, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        let attacker = c.enemies[0];
        c.apply_power_to(attacker, PowerId::PainfulStabs, 2, Some(attacker));
        for owner in 0..seats {
            c.players[owner].discard.clear();
            c.creatures[owner as usize].block = (owner as i32) * 5;
        }
        c.attack_stack.push(AttackCtx {
            attacker: Some(attacker), target: None, props: Props::MOVE,
            base_damage: None, results: Vec::new(),
        });
        // Two AoE hits in deliberately reverse seat order. No single target
        // describes this command; each receiver independently absorbs block.
        for _ in 0..2 {
            for owner in (0..seats).rev() {
                let damage = c.calculate_damage(5, Some(attacker), owner, Props::MOVE);
                c.apply_damage(owner, damage, Props::MOVE, Some(attacker));
            }
        }
        // Complete the explicit scope through the public pending-command flush.
        c.pending_auto_attack = true;
        c.flush_pending_attack();
        assert_eq!(c.fault, None);
        for owner in 0..seats {
            let expected = 2usize.saturating_sub(owner as usize) * 2;
            assert_eq!(c.players[owner].discard.len(), expected);
            assert!(c.players[owner].discard.iter().all(|&k| c.cards[k as usize].id == CardId::Wound));
        }
        // A separate command starts with an empty result list; old hits cannot
        // create another set of Wounds.
        for owner in 0..seats { c.creatures[owner as usize].block = 100; }
        let before: Vec<_> = (0..seats).map(|p| c.players[p].discard.len()).collect();
        c.monster_attack_players(attacker, 5, 2);
        c.flush_pending_attack();
        assert_eq!(before, (0..seats).map(|p| c.players[p].discard.len()).collect::<Vec<_>>());
        assert_eq!(c.fault, None);
    }
}
