//! Acquisition belongs to the requesting network player, not an inventory sentinel.
use super::*;

#[test]
fn local_neow_pickup_is_not_blocked_by_teammates_copy() {
    for owner in 1..4 {
        let mut baseline = RunManager::new(4100000, RunOptions::default());
        let mut mgr = RunManager::new(4100000, RunOptions::default());
        let mut relic = Relic::new(RelicId::LeafyPoultice); relic.owner = owner;
        mgr.run_ctx_mut().relics.push(relic);
        for m in [&mut baseline, &mut mgr] {
            m.neow[0].relic = RelicId::LeafyPoultice;
            let mut action = RunAction::new(RunActionKind::EventChoice);
            action.player_id = Some(PLAYER_ID);
            action.option_id = Some(m.neow[0].option_id.legacy_id());
            m.take_action(action).unwrap();
        }
        assert!(mgr.run_ctx().relics.iter().any(|r| r.id == RelicId::LeafyPoultice && r.owner == 0));
        mgr.run_ctx_mut().relics.retain(|r| r.owner == 0);
        assert_eq!(format!("{:?}", mgr.run_ctx()), format!("{:?}", baseline.run_ctx()));
        assert_eq!(mgr.phase, baseline.phase);
    }
}

#[test]
fn local_shop_pickup_gets_its_own_copy_and_effect_when_teammate_owns_one() {
    for owner in 1..4 {
        let mut baseline = RunManager::new(4100000, RunOptions::default());
        let mut mgr = RunManager::new(4100000, RunOptions::default());
        for m in [&mut baseline, &mut mgr] {
            m.run_ctx_mut().gold = 9999;
            m.run_ctx_mut().hp = 20;
            m.enter_shop(MapPointType::Shop).unwrap();
            m.shop.relics[0].relic_id = RelicId::LeesWaffle;
        }
        let mut relic = Relic::new(RelicId::LeesWaffle); relic.owner = owner;
        mgr.run_ctx_mut().relics.push(relic);
        for m in [&mut baseline, &mut mgr] {
            let mut action = RunAction::new(RunActionKind::BuyRelic);
            action.player_id = Some(PLAYER_ID); action.index = Some(0);
            m.take_action(action).unwrap();
        }
        assert!(mgr.run_ctx().relics.iter().any(|r| r.id == RelicId::LeesWaffle && r.owner == 0));
        mgr.run_ctx_mut().relics.retain(|r| r.owner == 0);
        assert_eq!(format!("{:?}", mgr.run_ctx()), format!("{:?}", baseline.run_ctx()));
        assert_eq!(format!("{:?}", mgr.shop), format!("{:?}", baseline.shop));
    }
}
