//! Owner-pickup validation must run before the public run-action dispatch mutates state.
use super::*;

fn snapshot(mgr: &RunManager) -> String {
    format!("{:?}", (mgr.run_ctx(), &mgr.relic_bags, &mgr.shop, &mgr.rest,
        mgr.phase, mgr.unported, &mgr.reward, &mgr.run_choice))
}

#[test]
fn acquiring_teammate_without_inventory_sentinel_cannot_pick_up_leafy_poultice() {
    for owner in 1..4 {
        let mut mgr = RunManager::new(4100000, RunOptions::default());
        mgr.neow[0].relic = RelicId::LeafyPoultice;
        let before = snapshot(&mgr);
        let mut action = RunAction::new(RunActionKind::EventChoice);
        action.option_id = Some(mgr.neow[0].option_id.legacy_id());
        action.player_id = Some(owner + 1);
        assert_eq!(mgr.take_action(action), Err("owner pickup needs MP run layer"));
        assert_eq!(mgr.unported, Some("owner pickup needs MP run layer"));
        mgr.unported = Some("EVENT:Neow"); // the fault marker is the only permitted mutation
        assert_eq!(snapshot(&mgr), before);
    }
}

#[test]
fn teammate_maw_bank_normal_shop_entry_never_pays_seat_zero() {
    for owner in 1..4 {
        let mut baseline = RunManager::new(4100000, RunOptions::default());
        let mut mgr = RunManager::new(4100000, RunOptions::default());
        let mut r = Relic::new(RelicId::MawBank); r.owner = owner;
        mgr.run_ctx_mut().relics.push(r);
        baseline.enter_shop(MapPointType::Shop).unwrap();
        mgr.enter_shop(MapPointType::Shop).unwrap();
        mgr.run_ctx_mut().relics.retain(|r| r.owner == 0);
        assert_eq!(format!("{:?}", mgr.run_ctx()), format!("{:?}", baseline.run_ctx()));
        assert_eq!(format!("{:?}", mgr.shop), format!("{:?}", baseline.shop));
    }
}

#[test]
fn teammate_shop_pickup_refuses_before_gold_bags_stock_and_purchase_hooks() {
    for owner in 1..4 {
        for id in [RelicId::Kifuda, RelicId::LeesWaffle] {
            let mut mgr = RunManager::new(4100000, RunOptions::default());
            mgr.unported = None;
            mgr.run_ctx_mut().gold = 9999;
            mgr.enter_shop(MapPointType::Shop).unwrap();
            mgr.shop.relics[0].relic_id = id;
            let mut r = Relic::new(id); r.owner = owner;
            mgr.run_ctx_mut().relics.push(r);
            mgr.run_ctx_mut().relics.push(Relic::new(RelicId::MawBank));
            mgr.run_ctx_mut().relics.push(Relic::new(RelicId::TheCourier));
            let before = snapshot(&mgr);
            let mut action = RunAction::new(RunActionKind::BuyRelic);
            action.index = Some(0);
            action.player_id = Some(i32::from(owner) + 1);
            assert_eq!(mgr.take_action(action), Err("owner pickup needs MP run layer"));
            mgr.unported = None;
            assert_eq!(snapshot(&mgr), before, "{id:?}, owner {owner}");
        }
    }
}

#[test]
fn teammate_neow_pickup_refuses_before_marker_bags_inventory_and_rewards() {
    for owner in 1..4 {
        for id in [RelicId::LeadPaperweight, RelicId::LostCoffer, RelicId::Kaleidoscope,
            RelicId::LeafyPoultice, RelicId::Astrolabe, RelicId::BiiigHug, RelicId::CursedPearl] {
            let mut mgr = RunManager::new(4100000, RunOptions::default());
            mgr.neow[0].relic = id;
            let mut r = Relic::new(id); r.owner = owner;
            mgr.run_ctx_mut().relics.push(r);
            let before = snapshot(&mgr);
            let mut action = RunAction::new(RunActionKind::EventChoice);
            action.option_id = Some(mgr.neow[0].option_id.legacy_id());
            action.player_id = Some(i32::from(owner) + 1);
            assert_eq!(mgr.take_action(action), Err("owner pickup needs MP run layer"));
            mgr.unported = Some("EVENT:Neow");
            assert_eq!(snapshot(&mgr), before, "{id:?}, owner {owner}");
        }
    }
}

#[test]
fn teammate_tent_does_not_enable_a_second_rest_mutation_but_local_tent_does() {
    for owner in 0..4 {
        let mut mgr = RunManager::new(4100000, RunOptions::default());
        mgr.unported = None;
        let mut tent = Relic::new(RelicId::MiniatureTent); tent.owner = owner;
        mgr.run_ctx_mut().relics.push(tent);
        mgr.run_ctx_mut().hp = 20;
        mgr.enter_rest_site().unwrap();
        let heal = mgr.available_actions().into_iter().find(|a|
            a.kind == RunActionKind::RestOption && a.option_id == Some("HEAL")).unwrap();
        mgr.take_action(heal).unwrap();
        if owner == 0 {
            assert_eq!(mgr.phase, RunPhase::RestSite);
            assert!(mgr.available_actions().iter().any(|a|
                a.kind == RunActionKind::RestOption && a.option_id == Some("SMITH")));
        } else {
            assert_eq!(mgr.phase, RunPhase::MapChoice);
            let before = snapshot(&mgr);
            let mut smith = RunAction::new(RunActionKind::RestOption);
            smith.option_id = Some("SMITH");
            assert!(mgr.take_action(smith).is_err());
            assert_eq!(snapshot(&mgr), before, "owner {owner}");
        }
    }
}
