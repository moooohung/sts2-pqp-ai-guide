use super::*;

#[test]
fn d_g_deck_creation_modifiers_ignore_teammate_inventories() {
    for (id, card_id) in [(RelicId::FrozenEgg, CardId::Inflame),
        (RelicId::FresnelLens, CardId::DefendIronclad)] {
        for owner in 0..4 {
            let mut mgr = RunManager::new(4100000, RunOptions::default());
            mgr.unported = None;
            let mut relic = Relic::new(id); relic.owner = owner;
            mgr.run_ctx_mut().relics.push(relic);
            let gold = mgr.run_ctx().gold;
            mgr.add_card_to_deck(card_id).unwrap();
            let card = mgr.run_ctx().deck.last().unwrap();
            assert_eq!(card.upgraded, owner == 0 && id == RelicId::FrozenEgg);
            assert_eq!(card.enchantments.contains(&(crate::identity::EnchantmentId::Nimble, 2)), owner == 0 && id == RelicId::FresnelLens);
            assert_eq!(mgr.run_ctx().gold, gold);
        }
    }
}

#[test]
fn teammates_unported_reward_modifiers_do_not_refuse_local_reward_generation() {
    for id in [RelicId::Glitter, RelicId::DingyRug] {
        let mut baseline = RunManager::new(4100000, RunOptions::default());
        baseline.unported = None;
        baseline.generate_combat_rewards(RoomType::Monster);
        let expected = format!("{:?}", baseline.reward);
        for owner in 1..4 {
            let mut mgr = RunManager::new(4100000, RunOptions::default());
            mgr.unported = None;
            let mut relic = Relic::new(id); relic.owner = owner;
            mgr.run_ctx_mut().relics.push(relic);
            mgr.generate_combat_rewards(RoomType::Monster);
            assert_eq!(mgr.unported, None);
            assert_eq!(format!("{:?}", mgr.reward), expected);
            assert_eq!(mgr.run_ctx().gold, baseline.run_ctx().gold);
        }
    }
}
