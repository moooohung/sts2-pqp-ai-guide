//! Real merchant/rest/Neow dispatch: teammate relics cannot mutate seat zero.
use super::*;

#[test]
fn teammate_membership_and_parasol_leave_local_shop_and_purchase_unchanged() {
    for owner in 1..4 {
        for id in [RelicId::MembershipCard, RelicId::LordsParasol] {
            let mut baseline = RunManager::new(4100000, RunOptions::default());
            let mut mgr = RunManager::new(4100000, RunOptions::default());
            for m in [&mut baseline, &mut mgr] { m.run_ctx_mut().gold = 9999; }
            let mut r = Relic::new(id); r.owner = owner;
            mgr.run_ctx_mut().relics.push(r);
            baseline.enter_shop(MapPointType::Shop).unwrap();
            mgr.enter_shop(MapPointType::Shop).unwrap();
            assert_eq!(format!("{:?}", mgr.shop), format!("{:?}", baseline.shop));
            let mut buy = RunAction::new(RunActionKind::BuyCard); buy.index = Some(0);
            baseline.take_action(buy).unwrap();
            mgr.take_action(buy).unwrap();
            mgr.run_ctx_mut().relics.retain(|r| r.owner == 0);
            assert_eq!(format!("{:?}", mgr.run_ctx()), format!("{:?}", baseline.run_ctx()));
            assert_eq!(format!("{:?}", mgr.shop), format!("{:?}", baseline.shop));
        }
    }
}

#[test]
fn teammate_meat_cleaver_cannot_offer_cook_or_mutate_local_deck_and_hp() {
    for owner in 1..4 {
        let mut mgr = RunManager::new(4100000, RunOptions::default());
        mgr.unported = None;
        let mut r = Relic::new(RelicId::MeatCleaver); r.owner = owner;
        mgr.run_ctx_mut().relics.push(r);
        mgr.enter_rest_site().unwrap();
        assert!(!mgr.available_actions().iter().any(|a| a.option_id == Some("COOK")));
        let before = format!("{:?}", (mgr.run_ctx(), &mgr.rest, &mgr.run_choice, mgr.phase));
        let mut cook = RunAction::new(RunActionKind::RestOption); cook.option_id = Some("COOK");
        assert!(mgr.take_action(cook).is_err());
        assert_eq!(before, format!("{:?}", (mgr.run_ctx(), &mgr.rest, &mgr.run_choice, mgr.phase)));
    }
}

#[test]
fn large_capsule_neow_dispatch_adds_owners_character_basics() {
    for (character, strike, defend) in [
        (crate::character::Character::Ironclad, CardId::StrikeIronclad, CardId::DefendIronclad),
        (crate::character::Character::Silent, CardId::StrikeSilent, CardId::DefendSilent),
        (crate::character::Character::Defect, CardId::StrikeDefect, CardId::DefendDefect),
        (crate::character::Character::Regent, CardId::StrikeRegent, CardId::DefendRegent),
        (crate::character::Character::Necrobinder, CardId::StrikeNecrobinder, CardId::DefendNecrobinder),
    ] {
        let mut mgr = RunManager::new(4100115, RunOptions::default());
        mgr.character = character;
        mgr.neow[0].relic = RelicId::LargeCapsule;
        for bag in mgr.relic_bags.player[0].iter_mut().chain(mgr.relic_bags.shared.iter_mut()) {
            *bag = vec![RelicId::Strawberry];
        }
        let before = mgr.run_ctx().deck.len();
        let mut choice = RunAction::new(RunActionKind::EventChoice);
        choice.option_id = Some(mgr.neow[0].option_id.legacy_id());
        mgr.take_action(choice).unwrap();
        assert_eq!(mgr.run_ctx().deck.len(), before + 2);
        assert_eq!(mgr.run_ctx().deck[before].id, strike);
        assert_eq!(mgr.run_ctx().deck[before + 1].id, defend);
        assert!(mgr.run_ctx().deck[before..].iter().all(|card| card.owner == 0));
    }
}
