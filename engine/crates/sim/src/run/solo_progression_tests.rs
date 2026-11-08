use super::*;

#[test]
fn parasol_waits_for_mirror_choice_before_purchasing_remaining_relics() {
    let mut run=RunManager::new(73001,RunOptions::default());
    run.unported=None;
    run.phase=RunPhase::Shop;
    run.run_ctx_mut().room_type=RoomType::Shop;
    run.shop.cards.clear();
    run.shop.colorless_cards.clear();
    run.shop.potions.clear();
    run.shop.relics.clear();
    run.shop.relics.extend([
        shop::ShopRelicEntry{relic_id:RelicId::DollysMirror,rarity:RelicRarity::Shop,price:0},
        shop::ShopRelicEntry{relic_id:RelicId::Strawberry,rarity:RelicRarity::Common,price:0}]);
    let hp=run.run_ctx().max_hp;
    run.auto_purchase_shop_inventory().unwrap();
    assert!(run.pending_run_choice.is_some());
    assert_eq!(run.run_ctx().max_hp,hp);
    let action=run.available_actions().into_iter().find(|a|a.kind==RunActionKind::Choose).unwrap();
    run.take_action(action).unwrap();
    run.available_actions();
    assert!(run.unported.is_none());
    assert_eq!(run.run_ctx().max_hp,hp+7);
    assert_eq!(run.run_ctx().relics.iter().filter(|r|r.id==RelicId::DollysMirror).count(),1);
}

#[test]
fn census_win_resumes_combat_already_ended_during_setup() {
    let mut run=RunManager::new_debug_late_boss(73001,RunOptions{ascension:0,..Default::default()},
        crate::run::mp::Character::Ironclad,1).unwrap();
    run.combat.as_mut().unwrap().debug_win_combat().unwrap();
    run.debug_win_combat().unwrap();
    assert_eq!(run.phase(),RunPhase::CardReward);
    assert!(run.unported.is_none());
}

#[test]
fn solo_boss_rewards_progression_reaches_all_acts_and_victory() {
    use crate::run::mp::Character;
    for character in [Character::Ironclad,Character::Silent,Character::Defect,Character::Regent,Character::Necrobinder] {
        let mut run=RunManager::new_for_character(73001,RunOptions{ascension:0,..Default::default()},character);
        run.unported=None;
        for act in 0..3 {
            assert_eq!(run.current_act_index(),act);
            let boss=run.acts[act].map.as_ref().unwrap().boss;
            run.current_point=boss;
            let coord=run.acts[act].map.as_ref().unwrap().points[boss as usize].coord;
            run.run_ctx_mut().coord=Some((coord.col,coord.row));
            let encounter=run.acts[act].rooms.boss;
            rest::after_room_entered(run.run_ctx_mut(),RoomType::Boss,MapPointType::Boss);
            run.enter_encounter(RoomType::Boss,encounter).unwrap();
            run.debug_win_combat().unwrap();
            for _ in 0..20 {
                if run.current_act_index()!=act || run.current_event_id()==Some("THE_ARCHITECT") {break;}
                let actions=run.available_actions();
                let action=actions.iter().find(|a|a.enabled&&a.kind==RunActionKind::SkipRewards)
                    .or_else(||actions.iter().find(|a|a.enabled&&a.kind==RunActionKind::Skip)).unwrap();
                run.take_action(*action).unwrap();
            }
            if act<2 {assert_eq!(run.current_act_index(),act+1);}
        }
        assert_eq!(run.current_event_id(),Some("THE_ARCHITECT"));
        for _ in 0..3 {
            if run.phase()==RunPhase::RunOver {break;}
            let action=run.available_actions().iter().find(|a|a.enabled&&a.kind==RunActionKind::EventChoice).copied().unwrap();
            run.take_action(action).unwrap();
        }
        assert!(run.observation_full_run_victory());
    }
}
