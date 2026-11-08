//! MerchantPotionEntry.OnTryPurchase and MerchantEntry.OnTryPurchaseWrapper
//! in the pinned original DLL: failed procurement has no purchase effects.
use super::*;

fn merchant(slots: i32) -> RunManager {
    let mut manager=RunManager::new(20261008000033,RunOptions::default());
    manager.phase=RunPhase::Shop;
    manager.run_ctx_mut().gold=196;
    manager.run_ctx_mut().max_potion_slots=slots;
    manager.run_ctx_mut().potions.clear();
    manager.shop=shop::ShopState::default();
    manager.shop.potions.push(shop::ShopPotionEntry {
        potion_id:PotionId::Powerpotion,rarity:PotionRarity::Common,price:51,
    });
    manager
}

#[test]
fn every_full_belt_and_forbidden_procurement_preserves_purchase_state() {
    for slots in 0..=5 {
        for sozu in [false,true] {
            for courier in [false,true] {
                let mut manager=merchant(slots);
                for _ in 0..slots {manager.run_ctx_mut().potions.push(Some(PotionId::Fairyinabottle));}
                if sozu {manager.run_ctx_mut().relics.push(Relic::new(RelicId::Sozu));}
                if courier {manager.run_ctx_mut().relics.push(Relic::new(RelicId::TheCourier));}
                let before=format!("{:?}",manager.run_ctx());
                let stock=format!("{:?}",manager.shop);
                assert!(!manager.available_actions().iter().any(|a|a.kind==RunActionKind::BuyPotion));
                manager.buy_shop_potion(0).unwrap();
                assert_eq!(format!("{:?}",manager.run_ctx()),before,"slots={slots} sozu={sozu} courier={courier}");
                assert_eq!(format!("{:?}",manager.shop),stock);
            }
        }
    }
    // Forbidden procurement must also preserve an empty, otherwise usable slot.
    let mut manager=merchant(2);
    manager.run_ctx_mut().relics.push(Relic::new(RelicId::Sozu));
    let before=format!("{:?}",manager.run_ctx());
    manager.buy_shop_potion(0).unwrap();
    assert_eq!(format!("{:?}",manager.run_ctx()),before);
    assert_eq!(manager.shop.potions[0].price,51);
}

#[test]
fn successful_procurement_charges_once_and_uses_the_first_free_slot() {
    for hole in [false,true] {
        let mut manager=merchant(2);
        manager.run_ctx_mut().potions.push(Some(PotionId::Fairyinabottle));
        if hole {manager.run_ctx_mut().potions.push(None);}
        assert!(manager.available_actions().iter().any(|a|a.kind==RunActionKind::BuyPotion));
        manager.buy_shop_potion(0).unwrap();
        assert_eq!(manager.run_ctx().gold,145);
        assert_eq!(manager.run_ctx().potions.as_slice(),&[Some(PotionId::Fairyinabottle),Some(PotionId::Powerpotion)]);
        assert_eq!(manager.shop.potions[0].price,shop::SOLD_OUT);
    }
}

#[test]
fn merchant_generation_and_courier_refill_use_the_owning_character_pool() {
    // PotionFactory.GetPotionOptions concatenates the owner's epoch with
    // SharedPotionPool; no foreign character potion is eligible.
    for character in crate::character::Character::ALL {
        let allowed=super::super::potion_pools::cards(character).unwrap();
        let mut manager=merchant(2);
        manager.character=character;
        manager.enter_shop(MapPointType::Shop).unwrap();
        assert!(manager.shop.potions.iter().all(|entry|allowed.contains(&entry.potion_id)));
        for _ in 0..64 {
            let mut state=manager.shop.clone();
            shop::refill_potion(&mut state,0,manager.run_ctx_mut()).unwrap();
            assert!(allowed.contains(&state.potions[0].potion_id),"{character}");
        }
    }
}

#[test]
fn phial_holster_original_dll_batch_witnesses_and_draw_count() {
    // Original-DLL sweep: second item is selected from the pool after
    // removing the first, not a second independent one-potion factory.
    for (seed,first,second) in [
        (20261008010001,"BlessingOfTheForge","FyshOil"),
        (20261008000085,"BloodPotion","EnergyPotion"),
    ] {
        // NativeBackend's explicit-reset wrapper normalizes before the Gym adapter.
        let mut manager=RunManager::new(seed % crate::rng::INT_MAX as u64,RunOptions {
            number_of_runs:999_999_999,..RunOptions::default()
        });
        let option=manager.neow.iter().find(|n|n.relic==RelicId::PhialHolster)
            .expect("original Phial Holster offer").option_id;
        manager.resolve_neow_choice(option).unwrap();
        assert_eq!(manager.run_ctx().potions[0],PotionId::from_name(first));
        assert_eq!(manager.run_ctx().potions[1],PotionId::from_name(second));
        assert_eq!(manager.run_ctx().rngs.combat_potion_generation.counter,4);
    }
}

#[test]
fn potion_reward_routing_keeps_physical_ordinal_after_prior_rewards_are_consumed() {
    let mut manager=merchant(2);
    manager.reward=RewardState::default();
    for id in [PotionId::Firepotion,PotionId::Strengthpotion] {
        manager.reward.items.push(RewardItem {
            kind:RewardItemKind::Potion(id),consumed:false,skippable:true,
            skip_completes:false,add_injury_after:false,neows_bones_final:false,
        });
    }
    manager.reward.active=Some(1);
    assert_eq!(manager.observation_potion_reward_ordinal(None),Some(1));
    manager.reward.items[0].consumed=true;
    // Remaining-menu index zero is still original PotionReward ordinal one.
    assert_eq!(manager.observation_potion_reward_ordinal(Some(0)),Some(1));
    assert_eq!(manager.observation_potion_reward_ordinal(Some(1)),None);
}

#[test]
fn claimed_potion_is_not_offered_again_while_other_rewards_remain_open() {
    let mut manager=merchant(3);
    manager.phase=RunPhase::CardReward;
    manager.reward=RewardState::default();
    manager.reward.stay_in_container=true;
    for id in [PotionId::Weakpotion,PotionId::Firepotion] {
        manager.reward.items.push(RewardItem {
            kind:RewardItemKind::Potion(id),consumed:false,skippable:true,
            skip_completes:false,add_injury_after:false,neows_bones_final:false,
        });
    }
    manager.pick_potion(Some(0)).unwrap();
    assert_eq!(manager.run_ctx().potions.as_slice(),&[Some(PotionId::Weakpotion)]);
    let actions=manager.available_actions();
    assert!(!actions.iter().any(|a|a.kind==RunActionKind::PickPotion&&a.potion_id==Some(PotionId::Weakpotion)));
    assert!(actions.iter().any(|a|a.kind==RunActionKind::PickPotion&&a.potion_id==Some(PotionId::Firepotion)));
    assert!(manager.reward.items[0].consumed);
    assert!(!manager.reward.items[1].consumed);
}
