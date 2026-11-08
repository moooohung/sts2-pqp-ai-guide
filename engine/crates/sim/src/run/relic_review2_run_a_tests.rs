use crate::character::CardPool;
use super::*;

fn manager() -> RunManager {
    let mut mgr = RunManager::new(4100000, RunOptions::default());
    mgr.unported = None; mgr
}

#[test]
fn book_of_five_rings_counts_actual_deck_entries_and_shop_purchase_heals_fifth() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.clear();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::BookOfFiveRings));
    mgr.run_ctx_mut().hp = 30; mgr.run_ctx_mut().max_hp = 100; mgr.run_ctx_mut().gold = 1000;
    for count in 1..=4 {
        mgr.add_card_to_deck(CardId::DefendIronclad).unwrap();
        assert_eq!(mgr.run_ctx().relics[0].counter, count);
        assert_eq!(mgr.run_ctx().hp, 30);
    }
    mgr.enter_shop(MapPointType::Shop).unwrap();
    let rng = format!("{:?}", mgr.run_ctx().rngs);
    let action = mgr.available_actions().into_iter()
        .find(|a| a.kind == RunActionKind::BuyCard && a.index == Some(0)).unwrap();
    mgr.take_action(action).unwrap();
    assert_eq!(mgr.run_ctx().relics[0].counter, 5);
    assert_eq!(mgr.run_ctx().hp, 50);
    assert_eq!(format!("{:?}", mgr.run_ctx().rngs), rng);
    mgr.run_ctx_mut().hp = 95;
    for _ in 0..5 { mgr.add_card_to_deck(CardId::DefendIronclad).unwrap(); }
    assert_eq!(mgr.run_ctx().relics[0].counter, 10);
    assert_eq!(mgr.run_ctx().hp, 100);
}

#[test]
fn book_of_five_rings_ignores_foreign_disabled_and_dead_owners() {
    for (owner, enabled, hp) in [(1, true, 30), (0, false, 30), (0, true, 0)] {
        let mut mgr = manager(); mgr.run_ctx_mut().relics.clear();
        let mut relic = Relic::new(RelicId::BookOfFiveRings);
        relic.owner = owner; relic.enabled = enabled; relic.counter = 4;
        mgr.run_ctx_mut().relics.push(relic);
        mgr.run_ctx_mut().hp = hp; mgr.run_ctx_mut().max_hp = 100;
        mgr.add_card_to_deck(CardId::DefendIronclad).unwrap();
        assert_eq!(mgr.run_ctx().hp, hp);
        assert_eq!(mgr.run_ctx().relics[0].counter, 4);
    }
}

#[test]
fn sz02_travel_and_extra_elite_rewards_require_local_owner() {
    for owner in 0..4 {
        let mut mgr = manager();
        mgr.run_ctx_mut().relics.clear();
        for id in [RelicId::WingedBoots, RelicId::WhiteStar] {
            let mut r = Relic::new(id); r.owner = owner; mgr.run_ctx_mut().relics.push(r);
        }
        assert_eq!(mgr.winged_boots_free_travel(), owner == 0);
        mgr.generate_combat_rewards(RoomType::Monster);
        assert!(mgr.unported.is_none(), "{:?}", mgr.unported);
        let cards = |mgr: &RunManager| mgr.reward.items.iter().filter(|i| matches!(i.kind, RewardItemKind::Card { .. })).count();
        assert_eq!(cards(&mgr), 1);
        mgr.generate_combat_rewards(RoomType::Elite);
        assert!(mgr.unported.is_none(), "{:?}", mgr.unported);
        assert_eq!(cards(&mgr), if owner == 0 { 2 } else { 1 });
    }
}

#[test]
fn dingy_rug_real_rewards_use_owner_colorless_union_before_selection() {
    let mut found_colorless = false;
    for owner in 0..4 {
        for seed in 1..=40 {
            let mut mgr = RunManager::new(seed, RunOptions::default());
            mgr.unported = None;
            mgr.run_ctx_mut().relics.clear();
            let mut baseline = RunManager::new(seed, RunOptions::default());
            baseline.unported = None;
            baseline.run_ctx_mut().relics.clear();
            let mut relic = Relic::new(RelicId::DingyRug);
            relic.owner = owner;
            mgr.run_ctx_mut().relics.push(relic);
            baseline.generate_combat_rewards(RoomType::Monster);
            mgr.generate_combat_rewards(RoomType::Monster);
            assert!(mgr.unported.is_none(), "{:?}", mgr.unported);
            if owner != 0 {
                assert_eq!(format!("{:?}", mgr.reward), format!("{:?}", baseline.reward));
            }
            let allowed = super::super::card_pools::cards(mgr.character.into(), mgr.is_multiplayer()).unwrap();
            let colorless = super::super::card_pools::cards(CardPool::Colorless, mgr.is_multiplayer()).unwrap();
            for item in &mgr.reward.items {
                if let RewardItemKind::Card { cards, .. } = &item.kind {
                    for card in cards {
                        assert!(allowed.contains(&card.id) || (owner == 0 && colorless.contains(&card.id)));
                        found_colorless |= owner == 0 && colorless.contains(&card.id);
                    }
                }
            }
        }
    }
    assert!(found_colorless);
}

#[test]
fn foreign_discard_potion_refuses_in_every_phase_before_local_inventory_changes() {
    for phase in [RunPhase::Event, RunPhase::MapChoice, RunPhase::Combat,
        RunPhase::CardReward, RunPhase::Treasure, RunPhase::Shop,
        RunPhase::RestSite, RunPhase::RunOver] {
        for seat in 1..4 {
            let mut mgr = manager();
            mgr.run_ctx_mut().potions = vec![Some(PotionId::Firepotion), None, None];
            if phase == RunPhase::Combat {
                let mut c = Combat::new(mgr.run_ctx().clone(), 1);
                for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
                mgr.combat = Some(c);
            }
            mgr.phase = phase;
            let before = format!("{:?}", mgr.run_ctx());
            let mut action = RunAction::new(RunActionKind::DiscardPotion);
            action.player_id = Some(PLAYER_ID + seat);
            action.slot_index = Some(0); action.potion_id = Some(PotionId::Firepotion);
            assert_eq!(mgr.take_action(action), Err("owner pickup needs MP run layer"));
            assert_eq!(format!("{:?}", mgr.run_ctx()), before, "{phase:?} seat {seat}");
        }
    }
}

#[test]
fn glitter_real_rewards_are_glam_enchanted_collectable_and_repeatable() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::Glitter));
    for _ in 0..2 {
        mgr.generate_combat_rewards(RoomType::Monster);
        assert!(mgr.unported.is_none(), "{:?}", mgr.unported);
        let index = mgr.reward.items.iter().position(|i| matches!(i.kind, RewardItemKind::Card { .. })).unwrap();
        if let RewardItemKind::Card { cards, .. } = &mgr.reward.items[index].kind {
            assert_eq!(cards.len(), 3);
            assert!(cards.iter().all(|c| c.enchantments.contains(&(crate::identity::EnchantmentId::Glam, 1))));
        }
        mgr.reward.active = Some(index); mgr.phase = RunPhase::CardReward;
        let action = mgr.available_actions().into_iter().find(|a| a.kind == RunActionKind::PickCard && a.index == Some(0)).unwrap();
        mgr.take_action(action).unwrap();
        assert!(mgr.run_ctx().deck.last().unwrap().enchantments.contains(&(crate::identity::EnchantmentId::Glam, 1)));
        assert!(mgr.run_ctx().relics.iter().any(|r| r.id == RelicId::Glitter && r.enabled));
    }
}

#[test]
fn driftwood_action_and_execution_both_require_local_owner() {
    for owner in 0..4 {
        let mut mgr = manager();
        let mut r = Relic::new(RelicId::Driftwood); r.owner = owner;
        mgr.run_ctx_mut().relics.push(r);
        mgr.generate_combat_rewards(RoomType::Monster);
        let index = mgr.reward.items.iter().position(|i| matches!(i.kind, RewardItemKind::Card { .. })).unwrap();
        mgr.reward.active = Some(index); mgr.phase = RunPhase::CardReward;
        assert_eq!(mgr.available_actions().iter().any(|a| a.kind == RunActionKind::RerollCardReward), owner == 0);
        let before = format!("{:?}", mgr.run_ctx());
        let reward = format!("{:?}", mgr.reward);
        let result = mgr.take_action(RunAction::new(RunActionKind::RerollCardReward));
        if owner == 0 { assert!(result.is_ok()); }
        else {
            assert_eq!(result, Err("reroll requires Driftwood"));
            assert_eq!(format!("{:?}", mgr.run_ctx()), before);
            assert_eq!(format!("{:?}", mgr.reward), reward);
        }
    }
}

#[test]
fn combat_reward_gold_uses_shared_ectoplasm_owner_aware_gain_path() {
    for room in [RoomType::Monster, RoomType::Elite, RoomType::Boss] {
        let mut baseline = manager(); baseline.run_ctx_mut().gold = 100;
        baseline.generate_combat_rewards(room);
        assert!(baseline.run_ctx().gold > 100);
        for owner in 0..4 {
            let mut mgr = manager(); mgr.run_ctx_mut().gold = 100;
            let mut r = Relic::new(RelicId::Ectoplasm); r.owner = owner;
            mgr.run_ctx_mut().relics.push(r);
            mgr.generate_combat_rewards(room);
            assert_eq!(mgr.run_ctx().gold, if owner == 0 { 100 } else { baseline.run_ctx().gold });
            assert_eq!(mgr.run_ctx().rngs.rewards.counter, baseline.run_ctx().rngs.rewards.counter);
        }
    }
}
