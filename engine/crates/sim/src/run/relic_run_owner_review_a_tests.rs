//! Real RunAction/room dispatch regressions for the a8abeda review.
use super::*;

fn manager() -> RunManager {
    let mut mgr = RunManager::new(4100000, RunOptions::default());
    mgr.unported = None;
    mgr
}

fn enter(mgr: &mut RunManager, kind: MapPointType) {
    mgr.phase = RunPhase::MapChoice;
    mgr.visited_points.clear();
    let map = mgr.acts[0].map.as_mut().unwrap();
    map.points[map.start as usize].point_type = kind;
    let coord = map.points[map.start as usize].coord;
    let mut action = RunAction::new(RunActionKind::Move);
    action.coord = Some(coord);
    mgr.take_action(action).unwrap();
}

#[test]
fn teammate_purchases_and_potion_actions_refuse_before_any_local_inventory_mutation() {
    for seat in 1..4 {
        for kind in [RunActionKind::BuyCard, RunActionKind::BuyPotion,
            RunActionKind::BuyRelic, RunActionKind::RemoveCard,
            RunActionKind::UsePotion, RunActionKind::DiscardPotion] {
            let mut mgr = manager();
            mgr.run_ctx_mut().gold = 9999;
            enter(&mut mgr, MapPointType::Shop);
            let before = format!("{:?}", mgr.run_ctx());
            let stock = format!("{:?}", mgr.shop);
            let mut action = RunAction::new(kind);
            action.player_id = Some(PLAYER_ID + seat);
            action.index = Some(0); action.slot_index = Some(0);
            assert_eq!(mgr.take_action(action), Err("owner pickup needs MP run layer"));
            assert_eq!(format!("{:?}", mgr.run_ctx()), before);
            assert_eq!(format!("{:?}", mgr.shop), stock);
        }
    }
}

#[test]
fn teammates_a_g_merchant_card_hooks_do_not_upgrade_or_enchant_local_stock() {
    for id in [RelicId::FrozenEgg, RelicId::FresnelLens] {
        let mut baseline = manager();
        enter(&mut baseline, MapPointType::Shop);
        let expected = format!("{:?}", baseline.shop.cards);
        for owner in 1..4 {
            let mut mgr = manager();
            let mut relic = Relic::new(id); relic.owner = owner;
            mgr.run_ctx_mut().relics.push(relic);
            let gold = mgr.run_ctx().gold;
            let deck = format!("{:?}", mgr.run_ctx().deck);
            enter(&mut mgr, MapPointType::Shop);
            assert_eq!(format!("{:?}", mgr.shop.cards), expected, "{id:?} owner {owner}");
            assert_eq!(mgr.run_ctx().gold, gold);
            assert_eq!(format!("{:?}", mgr.run_ctx().deck), deck);
        }
    }
}

#[test]
fn teammates_rest_hooks_leave_local_hp_flags_options_and_rewards_unchanged() {
    for owner in 1..4 {
        let mut mgr = manager();
        mgr.run_ctx_mut().hp = 10;
        for id in [RelicId::EternalFeather, RelicId::FakeVenerableTeaSet,
            RelicId::Girya, RelicId::DreamCatcher] {
            let mut relic = Relic::new(id); relic.owner = owner;
            mgr.run_ctx_mut().relics.push(relic);
        }
        let gold = mgr.run_ctx().gold;
        let deck = format!("{:?}", mgr.run_ctx().deck);
        enter(&mut mgr, MapPointType::RestSite);
        assert_eq!(mgr.run_ctx().hp, 10);
        assert!(!mgr.run_ctx().relics.iter().find(|r| r.id == RelicId::FakeVenerableTeaSet).unwrap().flag1);
        assert!(!mgr.available_actions().iter().any(|a| a.option_id == Some("LIFT")));
        let mut action = RunAction::new(RunActionKind::RestOption);
        action.option_id = Some("HEAL");
        mgr.take_action(action).unwrap();
        assert!(mgr.reward.items.is_empty()); // teammate DreamCatcher offers nothing.
        assert_eq!(mgr.run_ctx().gold, gold);
        assert_eq!(format!("{:?}", mgr.run_ctx().deck), deck);
    }
}

#[test]
fn black_star_adds_only_local_owners_extra_elite_relic_reward() {
    for room in [RoomType::Monster, RoomType::Elite, RoomType::Boss] {
        let mut baseline = manager();
        baseline.generate_combat_rewards(room);
        let base = baseline.reward.items.iter().filter(|i| matches!(i.kind, RewardItemKind::Relic(_))).count();
        for owner in 0..4 {
            let mut mgr = manager();
            let mut relic = Relic::new(RelicId::BlackStar); relic.owner = owner;
            mgr.run_ctx_mut().relics.push(relic);
            let gold = baseline.run_ctx().gold; let deck = format!("{:?}", mgr.run_ctx().deck);
            mgr.generate_combat_rewards(room);
            let count = mgr.reward.items.iter().filter(|i| matches!(i.kind, RewardItemKind::Relic(_))).count();
            assert_eq!(count, base + usize::from(owner == 0 && room == RoomType::Elite));
            assert_eq!(mgr.run_ctx().gold, gold);
            assert_eq!(format!("{:?}", mgr.run_ctx().deck), deck);
        }
    }
}
