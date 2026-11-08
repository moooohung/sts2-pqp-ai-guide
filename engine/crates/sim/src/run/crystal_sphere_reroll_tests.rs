use super::*;

#[test]
fn sphere_rewards_share_one_retained_rng_across_independent_one_shot_rerolls() {
    let mut mgr = RunManager::new(73003, RunOptions::default());
    mgr.unported = None;
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::Driftwood));
    let rewards = mgr.run_ctx().rngs.rewards.counter;
    let mut rng = Rng::new(123);
    let first = mgr.make_crystal_sphere_card_reward(&mut rng, CardRarity::Common).unwrap();
    let second = mgr.make_crystal_sphere_card_reward(&mut rng, CardRarity::Rare).unwrap();
    let counter = rng.counter;
    mgr.reward.items = vec![first, second];
    mgr.reward.sphere_rng = Some(rng);
    for (i, rarity) in [(0, CardRarity::Common), (1, CardRarity::Rare)] {
        mgr.reward.active = Some(i);
        mgr.reroll_card_reward().unwrap();
        match &mgr.reward.items[i].kind {
            RewardItemKind::Card { cards, progress, .. } => {
                assert_eq!(cards.len(), 3);
                assert!(cards.iter().all(|c| c.rarity == rarity));
                assert_eq!(progress.rerolls_used, 1);
            }
            _ => panic!("expected retained card reward"),
        }
        assert!(mgr.reroll_card_reward().is_err());
    }
    assert_eq!(mgr.reward.sphere_rng.as_ref().unwrap().counter, counter + 12);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, rewards);
    assert!(mgr.unported.is_none());
}
