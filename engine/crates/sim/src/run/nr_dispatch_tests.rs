use super::*;
use super::review_tests::{manager, finish_nr_combat};

#[test]
fn prayer_wheel_local_monster_victory_adds_one_reward_and_nine_rng_draws() {
    for room in [RoomType::Monster, RoomType::Elite] {
        let mut baseline = manager();
        let mut actual = baseline.clone();
        actual.run_ctx_mut().relics.push(Relic::new(RelicId::PrayerWheel));
        finish_nr_combat(&mut baseline, room);
        finish_nr_combat(&mut actual, room);
        let cards = |m: &RunManager| m.reward.items.iter()
            .filter(|r| matches!(r.kind, RewardItemKind::Card { .. })).count();
        let extra = usize::from(room == RoomType::Monster);
        assert_eq!(cards(&actual), cards(&baseline) + extra);
        assert_eq!(actual.run_ctx().rngs.rewards.counter,
            baseline.run_ctx().rngs.rewards.counter + (extra * 9) as u64);
        assert_eq!(actual.run_ctx().gold, baseline.run_ctx().gold);
        for item in &actual.reward.items {
            if let RewardItemKind::Card { cards, .. } = &item.kind {
                assert!(cards.iter().all(|c| c.owner == 0));
            }
        }
        assert!(actual.unported.is_none());
    }
}

#[test]
fn orrery_shop_dispatch_populates_five_rewards_and_returns_to_shop() {
    let mut mgr = manager();
    mgr.phase = RunPhase::Shop;
    mgr.run_ctx_mut().room_type = RoomType::Shop;
    mgr.run_ctx_mut().gold = 1000;
    mgr.shop.relics.push(shop::ShopRelicEntry {
        relic_id: RelicId::Orrery, rarity: RelicRarity::Shop, price: 175,
    });
    let rng = mgr.run_ctx().rngs.rewards.counter;
    let buy = *mgr.available_actions().iter().find(|a| a.kind == RunActionKind::BuyRelic).unwrap();
    mgr.take_action(buy).unwrap();
    assert_eq!(mgr.run_ctx().gold, 825);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, rng + 45);
    assert_eq!(mgr.reward.items.len(), 5);
    assert!(mgr.reward.active.is_none());
    assert!(mgr.reward.items.iter().all(|r| matches!(&r.kind,
        RewardItemKind::Card { cards, .. } if cards.len() == 3 && cards.iter().all(|c| c.owner == 0))));
    assert!(mgr.unported.is_none());
    let skip = *mgr.available_actions().iter().find(|a| a.kind == RunActionKind::SkipRewards).unwrap();
    mgr.take_action(skip).unwrap();
    assert_eq!(mgr.phase(), RunPhase::Shop);
}

#[test]
fn orrery_wing_charm_modifies_each_reward_before_offering() {
    let mut mgr = manager();
    mgr.phase = RunPhase::Shop;
    mgr.run_ctx_mut().room_type = RoomType::Shop;
    mgr.run_ctx_mut().gold = 1000;
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::WingCharm));
    mgr.shop.relics.push(shop::ShopRelicEntry {
        relic_id: RelicId::Orrery, rarity: RelicRarity::Shop, price: 175,
    });
    let buy = *mgr.available_actions().iter().find(|a| a.kind == RunActionKind::BuyRelic).unwrap();
    let counter=mgr.run_ctx().rngs.niche.counter;
    mgr.take_action(buy).unwrap();
    assert_eq!(mgr.unported,None);
    assert_eq!(mgr.reward.items.len(),5);
    assert_eq!(mgr.run_ctx().rngs.niche.counter,counter+5);
    for item in &mgr.reward.items {
        let RewardItemKind::Card{cards,..}=&item.kind else{panic!("Orrery card reward");};
        assert_eq!(cards.iter().filter(|c|c.enchantments.contains(&(crate::identity::EnchantmentId::Swift,1))).count(),1);
    }
    assert_eq!(mgr.phase(),RunPhase::CardReward);
}

#[test]
fn kifuda_shop_pickup_enchants_selected_cards_and_returns_to_shop() {
    let mut mgr = manager();
    mgr.phase = RunPhase::Shop;
    mgr.run_ctx_mut().room_type = RoomType::Shop;
    mgr.run_ctx_mut().gold = 1000;
    mgr.shop.relics.push(shop::ShopRelicEntry {
        relic_id: RelicId::Kifuda, rarity: RelicRarity::Shop, price: 175,
    });
    let buy = *mgr.available_actions().iter().find(|a| a.kind == RunActionKind::BuyRelic).unwrap();
    mgr.take_action(buy).unwrap();
    assert!(mgr.unported.is_none());
    assert_eq!(mgr.phase(), RunPhase::CardReward);
    let choice = mgr.pending_run_choice.as_ref().unwrap();
    assert_eq!(choice.wire_source, Some("relic:KIFUDA"));
    assert_eq!((choice.min_choices, choice.max_choices, choice.allow_skip),
        (0, choice.options.len().min(3), true));
    let selected: Vec<_> = choice.options.iter().take(3).map(|c| c.instance_id).collect();
    for index in 0..selected.len() {
        let action = *mgr.available_actions().iter()
            .find(|a| a.kind == RunActionKind::Choose && a.index == Some(index)).unwrap();
        mgr.take_action(action).unwrap();
    }
    let confirm = *mgr.available_actions().iter().find(|a| a.kind == RunActionKind::ConfirmChoice).unwrap();
    mgr.take_action(confirm).unwrap();
    assert_eq!(mgr.phase(), RunPhase::Shop);
    for instance_id in selected {
        let card = mgr.run_ctx().deck.iter().find(|c| c.instance_id == instance_id).unwrap();
        assert!(card.enchantments.contains(&(crate::identity::EnchantmentId::Adroit, 3)));
    }
}

#[test]
fn preserved_fog_relic_reward_dispatch_removes_three_then_adds_one_folly() {
    let mut mgr = manager();
    mgr.run_ctx_mut().total_floor = 5;
    mgr.phase = RunPhase::CardReward;
    mgr.reward.items.push(RewardItem {
        kind: RewardItemKind::Relic(RelicId::PreservedFog), consumed: false,
        skippable: true, skip_completes: true, add_injury_after: false, neows_bones_final: false,
    });
    let before = mgr.run_ctx().deck.len();
    let rng = mgr.run_ctx().rngs.rewards.counter;
    let niche = mgr.run_ctx().rngs.niche.counter;
    let pick = *mgr.available_actions().iter().find(|a| a.kind == RunActionKind::PickRelicReward).unwrap();
    mgr.take_action(pick).unwrap();
    for i in 0..3 {
        let choice = mgr.available_actions().into_iter().find(|a|
            a.kind == RunActionKind::Choose && a.index == Some(i)).unwrap();
        mgr.take_action(choice).unwrap();
    }
    if mgr.pending_run_choice.is_some() {
        let confirm = *mgr.available_actions().iter().find(|a| a.kind == RunActionKind::ConfirmChoice).unwrap();
        mgr.take_action(confirm).unwrap();
    }
    assert_eq!(mgr.run_ctx().deck.len(), before - 2);
    assert_eq!(mgr.run_ctx().deck.iter().filter(|c| c.id == CardId::Folly).count(), 1);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, rng);
    assert_eq!(mgr.run_ctx().rngs.niche.counter, niche);
    assert!(mgr.unported.is_none(), "{:?}", mgr.unported);
}
