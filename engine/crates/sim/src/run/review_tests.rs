//! Behavior regressions for review_r1_findings.md; expected rules are from C#.
use crate::character::CardPool;
use crate::character::Character;
use crate::identity::EventId;
use crate::identity::EnchantmentId;
use super::*;

pub(super) fn manager() -> RunManager {
    let mut mgr = RunManager::new(4100000, RunOptions::default());
    mgr.unported = None;
    mgr
}

#[test]
fn tanx_whistle_pickup_adds_card_and_claws_keeps_deck_position(){
    for relic in [RelicId::TanxsWhistle,RelicId::Claws,RelicId::TriBoomerang]{
        let mut mgr=manager();
        let mut event=EventState::new(EventId::Tanx,Rng::new(1));
        event.relics.push(relic);event.options.push(EventOption::new("relic_1","Tanx",""));
        mgr.current_event=Some(event);mgr.phase=RunPhase::Event;
        mgr.take_event_choice("relic_1").unwrap();
        assert!(mgr.run_ctx().relics.iter().any(|r|r.id==relic));
        if relic==RelicId::TanxsWhistle{
            assert_eq!(mgr.run_ctx().deck.last().unwrap().id,CardId::Whistle);
        }else if relic==RelicId::Claws{
            let choice=mgr.pending_run_choice.take().unwrap();
            mgr.apply_run_choice(choice,&[0]).unwrap();
            assert_eq!(mgr.run_ctx().deck[0].id,CardId::Maul);
        }else{assert_eq!(mgr.pending_run_choice.as_ref().unwrap().min_choices,3);}
    }
}

#[test]
fn fur_coat_pickup_marks_eight_column_major_rooms_without_shared_rng_draws(){
    let seed=20261113300000;
    let mut mgr=RunManager::new_debug_act(seed,RunOptions::default(),
        super::super::mp::Character::Ironclad,2).unwrap();
    let map=mgr.acts[2].map.as_ref().unwrap();
    let mut expected=Vec::new();
    for col in 0..7{for row in 0..=map.map_length{
        if map.point(Coord::new(col,row)).is_some_and(|p|
            matches!(p.point_type,MapPointType::Monster|MapPointType::Elite)){
            expected.push((col,row));
        }
    }}
    Rng::new(mgr.run_ctx().rngs.seed.wrapping_add(crate::rng::deterministic_hash64("FUR_COAT"))).shuffle(&mut expected);
    expected.truncate(8);
    let before=format!("{:?}",mgr.run_ctx().rngs);
    let mut event=EventState::new(EventId::Nonupeipe,Rng::new(0));
    event.relics.push(RelicId::FurCoat);
    event.options.push(EventOption::new("relic_1","Fur Coat",""));
    mgr.current_event=Some(event);mgr.phase=RunPhase::Event;
    mgr.take_event_choice("relic_1").unwrap();
    assert_eq!(mgr.phase,RunPhase::MapChoice);assert_eq!(mgr.unported,None);
    let relic=mgr.run_ctx().relics.iter().find(|r|r.id==RelicId::FurCoat).unwrap();
    assert_eq!(expected.len(),8);
    assert!(expected.iter().all(|&coord|relic.fur_coat_has_coord(coord)));
    assert_eq!(before,format!("{:?}",mgr.run_ctx().rngs));
}

#[test]
fn solo_signet_ring_and_event_gold_honor_ectoplasm_and_after_gold_gained(){
    for blocked in [false,true]{
        let mut mgr=manager();
        mgr.run_ctx_mut().relics.push(Relic::new(RelicId::DragonFruit));
        if blocked{mgr.run_ctx_mut().relics.push(Relic::new(RelicId::Ectoplasm));}
        let gold=mgr.run_ctx().gold;let max_hp=mgr.run_ctx().max_hp;
        let mut event=EventState::new(EventId::Nonupeipe,Rng::new(0));
        event.relics.push(RelicId::SignetRing);
        event.options.push(EventOption::new("relic_1","Ring",""));
        mgr.current_event=Some(event);mgr.phase=RunPhase::Event;
        mgr.take_event_choice("relic_1").unwrap();
        assert_eq!(mgr.run_ctx().gold,gold+if blocked{0}else{888});
        assert_eq!(mgr.run_ctx().max_hp,max_hp+if blocked{0}else{1});
        mgr.apply_event_effect(EventEffect::GainGold(75)).unwrap();
        assert_eq!(mgr.run_ctx().gold,gold+if blocked{0}else{963});
        assert_eq!(mgr.run_ctx().max_hp,max_hp+if blocked{0}else{2});
    }
}

#[test]
fn solo_relic_obtain_appends_distinct_models_including_fallback_circlets(){
    let mut mgr=manager();
    for _ in 0..6{mgr.obtain_event_relic(RelicId::Circlet).unwrap();}
    for _ in 0..2{mgr.obtain_event_relic(RelicId::OldCoin).unwrap();}
    assert_eq!(mgr.run_ctx().relics.iter().filter(|r|r.id==RelicId::Circlet).count(),6);
    assert_eq!(mgr.run_ctx().relics.iter().filter(|r|r.id==RelicId::OldCoin).count(),2);
}

#[test]
fn solo_duplicate_golden_compass_initializes_only_the_obtained_instance(){
    let mut mgr=manager();
    let index=shop::attach_relic(mgr.run_ctx_mut(),RelicId::GoldenCompass);
    mgr.run_ctx_mut().relics[index].counter2=99;
    let before=format!("{:?}",mgr.run_ctx().relics[index]);
    let mut event=EventState::new(EventId::Tezcatara,Rng::new(0));
    event.relics.push(RelicId::GoldenCompass);
    event.options.push(EventOption::new("comfort_1","Relic",""));
    mgr.current_event=Some(event);mgr.phase=RunPhase::Event;
    mgr.take_event_choice("comfort_1").unwrap();
    assert_eq!(format!("{:?}",mgr.run_ctx().relics[index]),before);
    assert!(mgr.run_ctx().relics.last().unwrap().flag1);
    assert_eq!(mgr.run_ctx().relics.last().unwrap().counter2,0);
}

#[test]
fn enchantment_type_filters_match_csharp_over_all_character_pools() {
    for character in crate::character::Character::ALL {
        for id in super::super::card_pools::cards(character.into(),true).unwrap() {
            let card=Card::from_factory(id,false,1).unwrap();
            if card.is_unplayable() {continue;}
            let normal=matches!(card.card_type,CardType::Attack|CardType::Skill|CardType::Power);
            assert_eq!(RunManager::event_can_enchant(&card,"RoyallyApproved"),
                matches!(card.card_type,CardType::Attack|CardType::Skill));
            for name in ["Sharp","Momentum","Corrupted","Vigorous","Instinct"] {
                assert_eq!(RunManager::event_can_enchant(&card,name),card.card_type==CardType::Attack,"{name} {id:?}");
            }
            assert_eq!(RunManager::event_can_enchant(&card,"Imbued"),card.card_type==CardType::Skill);
            assert_eq!(RunManager::event_can_enchant(&card,"Adroit"),normal);
            assert_eq!(RunManager::event_can_enchant(&card,"Slither"),normal&&!card.x_cost);
            assert_eq!(RunManager::event_can_enchant(&card,"Goopy"),normal&&card.has_tag(CardTag::Defend));
        }
    }
}

#[test]
fn silent_native_start_uses_csharp_character_deck_relic_hp_and_reward_pool() {
    let mut mgr=RunManager::new_for_character(4100000,RunOptions::default(),
        super::super::mp::Character::Silent);
    assert_eq!(mgr.character,Character::Silent);
    assert_eq!((mgr.run_ctx().max_hp,mgr.run_ctx().hp),(70,56));
    assert_eq!(mgr.run_ctx().relics[0].id,RelicId::RingOfTheSnake);
    assert_eq!(mgr.run_ctx().deck.iter().filter(|c|c.id==CardId::StrikeSilent).count(),5);
    assert_eq!(mgr.run_ctx().deck.iter().filter(|c|c.id==CardId::DefendSilent).count(),5);
    assert!(mgr.run_ctx().deck.iter().any(|c|c.id==CardId::Neutralize));
    assert!(mgr.run_ctx().deck.iter().any(|c|c.id==CardId::Survivor));
    mgr.unported=None;
    mgr.enter_encounter(RoomType::Boss, EncounterId::TheKinBoss).unwrap();
    assert_eq!(mgr.combat().unwrap().players[0].character_pool,Some(crate::character::CardPool::Silent));
    assert!(mgr.combat().unwrap().players[0].hand.len()>=7);
}

#[test]
fn defect_native_start_retains_identity_pool_and_cracked_core_orb() {
    let mut mgr=RunManager::new_for_character(4100000,RunOptions {ascension:0,..Default::default()},
        super::super::mp::Character::Defect);
    assert_eq!(mgr.character,Character::Defect);
    assert_eq!((mgr.run_ctx().max_hp,mgr.run_ctx().hp),(75,75));
    assert_eq!(mgr.run_ctx().relics[0].id,RelicId::CrackedCore);
    assert_eq!(mgr.run_ctx().base_orb_slots,3);
    assert_eq!(mgr.run_ctx().deck.iter().map(|c|c.id).collect::<Vec<_>>(),
        vec![CardId::StrikeDefect,CardId::StrikeDefect,CardId::StrikeDefect,CardId::StrikeDefect,
            CardId::DefendDefect,CardId::DefendDefect,CardId::DefendDefect,CardId::DefendDefect,
            CardId::Zap,CardId::Dualcast]);
    assert!(mgr.relic_bags.player[0].iter().flatten().any(|&id|id==RelicId::DataDisk));
    assert!(!mgr.relic_bags.player[0].iter().flatten().any(|&id|id==RelicId::HelicalDart));
    mgr.unported=None;
    mgr.enter_encounter(RoomType::Boss, EncounterId::TheKinBoss).unwrap();
    let c=mgr.combat().unwrap();
    assert_eq!(c.players[0].character,Some(crate::character::Character::Defect));
    assert_eq!(c.players[0].character_pool,Some(crate::character::CardPool::Defect));
    assert_eq!(c.players[0].orbs.capacity,3);
    assert_eq!(c.players[0].orbs.orbs.len(),1);
    assert_eq!(c.players[0].orbs.orbs[0].kind,crate::orbs::OrbKind::Lightning);
}

#[test]
fn starting_decks_and_mirror_copies_have_csharp_acquisition_floors() {
    for character in [super::super::mp::Character::Ironclad,super::super::mp::Character::Silent] {
        let mgr=RunManager::new_for_character(1,RunOptions {ascension:0,..Default::default()},character);
        assert!(mgr.run_ctx().deck.iter().all(|c|c.floor_added_to_deck==Some(1)));
    }
    let mut mgr=manager();
    mgr.run_ctx_mut().total_floor=12;
    mgr.phase=RunPhase::Shop;
    mgr.run_ctx_mut().deck.truncate(1);
    let original=mgr.run_ctx().deck[0].instance_id;
    assert!(mgr.offer_nr_pickup(RelicId::DollysMirror).unwrap());
    assert!(mgr.pending_run_choice.is_none());
    assert_eq!(mgr.phase,RunPhase::Shop);
    assert_eq!(mgr.run_ctx().deck.len(),2);
    assert_eq!(mgr.run_ctx().deck[0].floor_added_to_deck,Some(1));
    assert_eq!(mgr.run_ctx().deck[1].floor_added_to_deck,Some(12));
    assert_ne!(mgr.run_ctx().deck[1].instance_id,original);
}

#[test]
fn necrobinder_native_start_summons_grows_and_revives_the_same_osty() {
    let mut mgr=RunManager::new_for_character(4100000,RunOptions {ascension:0,..Default::default()},
        super::super::mp::Character::Necrobinder);
    assert_eq!(mgr.character,Character::Necrobinder);
    assert_eq!((mgr.run_ctx().hp,mgr.run_ctx().max_hp),(66,66));
    assert_eq!(mgr.run_ctx().relics[0].id,RelicId::BoundPhylactery);
    assert_eq!(mgr.run_ctx().base_orb_slots,0);
    assert_eq!(mgr.run_ctx().deck.iter().map(|c|c.id).collect::<Vec<_>>(),
        vec![CardId::StrikeNecrobinder,CardId::StrikeNecrobinder,CardId::StrikeNecrobinder,CardId::StrikeNecrobinder,
            CardId::DefendNecrobinder,CardId::DefendNecrobinder,CardId::DefendNecrobinder,CardId::DefendNecrobinder,
            CardId::Bodyguard,CardId::Unleash]);
    assert!(mgr.run_ctx().deck.iter().all(|c|c.floor_added_to_deck==Some(1)));
    assert!(mgr.relic_bags.player[0].iter().flatten().any(|&id|id==RelicId::BoneFlute));
    assert!(!mgr.relic_bags.player[0].iter().flatten().any(|&id|id==RelicId::DataDisk));
    mgr.unported=None;
    mgr.enter_encounter(RoomType::Boss, EncounterId::TheKinBoss).unwrap();
    let c=mgr.combat.as_mut().unwrap();
    assert_eq!(c.players[0].character,Some(crate::character::Character::Necrobinder));
    assert_eq!(c.players[0].character_pool,Some(crate::character::CardPool::Necrobinder));
    let pet=c.osty(0).unwrap();
    assert_eq!((c.cr(pet).hp,c.cr(pet).max_hp),(1,1));
    assert_eq!(c.cr(pet).pet_owner,Some(0));
    assert!(c.has_power(pet,crate::tables::PowerId::DieForYou));
    assert_eq!(c.summon_osty(0,7),Some(pet));
    assert_eq!((c.cr(pet).hp,c.cr(pet).max_hp),(8,8));
    c.cr_mut(pet).hp=0;
    assert_eq!(c.summon_osty(0,3),Some(pet));
    assert_eq!((c.cr(pet).hp,c.cr(pet).max_hp),(3,3));
    assert_eq!(c.players[0].pets.iter().filter(|&&p|p==pet).count(),1);
    assert_eq!(c.fault,None);
}

#[test]
fn pickup_singletons_auto_select_but_optional_kifuda_stays_manual() {
    for (relic,enchantment) in [(RelicId::RoyalStamp,crate::identity::EnchantmentId::RoyallyApproved),
        (RelicId::PunchDagger,crate::identity::EnchantmentId::Momentum)] {
        let mut mgr=manager();
        mgr.phase=RunPhase::Shop;
        mgr.run_ctx_mut().deck.truncate(1);
        assert!(mgr.offer_nr_pickup(relic).unwrap());
        assert!(mgr.pending_run_choice.is_none());
        assert_eq!(mgr.phase,RunPhase::Shop);
        assert!(mgr.run_ctx().deck[0].enchantments.iter().any(|(name,_)|*name==enchantment));
    }
    let mut mgr=manager();
    mgr.run_ctx_mut().deck.truncate(1);
    assert!(mgr.offer_nr_pickup(RelicId::Kifuda).unwrap());
    assert!(mgr.pending_run_choice.is_some());
}

#[test]
fn royal_stamp_shuffles_eligible_cards_then_enchants_one_and_returns_to_shop() {
    let mut mgr=manager();
    mgr.phase=RunPhase::Shop;
    let mut expected:Vec<_>=mgr.run_ctx().deck.iter().enumerate()
        .filter_map(|(i,c)|RunManager::event_can_enchant(c,"RoyallyApproved").then_some(i)).collect();
    let mut rng=mgr.run_ctx().rngs.niche.clone();
    rng.shuffle(&mut expected);
    let target=mgr.run_ctx().deck[expected[0]].instance_id;
    assert!(mgr.offer_nr_pickup(RelicId::RoyalStamp).unwrap());
    assert_eq!(mgr.run_ctx().rngs.niche,rng);
    let actions=mgr.available_actions();
    assert!(actions.iter().all(|a|a.kind!=RunActionKind::ConfirmChoice));
    let choices:Vec<_>=actions.iter().filter(|a|a.kind==RunActionKind::Choose).collect();
    assert_eq!(choices.len(),expected.len());
    let action=*choices[0];
    mgr.take_action(action).unwrap();
    assert_eq!(mgr.phase,RunPhase::Shop);
    let card=mgr.run_ctx().deck.iter().find(|c|c.instance_id==target).unwrap();
    assert!(card.enchantments.contains(&(crate::identity::EnchantmentId::RoyallyApproved,1)));
    assert!(card.has_kw(KW_INNATE));
    assert!(card.has_kw(KW_RETAIN));
}

#[test]
fn dollys_mirror_duplicates_exact_mutable_card_without_rng_and_returns_to_shop() {
    let mut mgr=manager();
    mgr.phase=RunPhase::Shop;
    mgr.run_ctx_mut().upgrade_deck_card(0);
    assert!(crate::card::enchant::apply_static_enchantment(
        &mut mgr.run_ctx_mut().deck[0],"Vigorous",8));
    let original=mgr.run_ctx().deck[0].clone();
    let len=mgr.run_ctx().deck.len();
    let rng=mgr.run_ctx().rngs.niche.clone();
    assert!(mgr.offer_nr_pickup(RelicId::DollysMirror).unwrap());
    let choose=mgr.available_actions().into_iter()
        .find(|a|a.kind==RunActionKind::Choose && a.index==Some(0)).unwrap();
    mgr.take_action(choose).unwrap();
    assert_eq!(mgr.phase,RunPhase::Shop);
    assert_eq!(mgr.run_ctx().rngs.niche,rng);
    assert_eq!(mgr.run_ctx().deck.len(),len+1);
    let copy=mgr.run_ctx().deck.last().unwrap();
    assert_ne!(copy.instance_id,original.instance_id);
    assert_eq!((copy.id,copy.upgraded,copy.cost,copy.damage,copy.enchantments.clone()),
        (original.id,original.upgraded,original.cost,original.damage,original.enchantments));
    assert_eq!(mgr.run_ctx().deck[0].instance_id,original.instance_id);
}

#[test]
fn byrdpip_pickup_transforms_all_eggs_with_fresh_base_cards_without_run_rng() {
    let mut mgr=manager();
    mgr.run_ctx_mut().total_floor=12;
    mgr.add_card_to_deck(CardId::ByrdonisEgg).unwrap();
    mgr.add_card_to_deck(CardId::ByrdonisEgg).unwrap();
    let eggs:Vec<_>=mgr.run_ctx().deck.iter().filter(|c|c.id==CardId::ByrdonisEgg)
        .map(|c|c.instance_id).collect();
    let counter=mgr.run_ctx().rngs.niche.clone();
    let next=mgr.run_ctx().next_card_id;
    mgr.apply_byrdpip_pickup().unwrap();
    assert!(mgr.run_ctx().deck.iter().all(|c|!eggs.contains(&c.instance_id)));
    let swoops:Vec<_>=mgr.run_ctx().deck.iter().filter(|c|c.id==CardId::ByrdSwoop).collect();
    assert_eq!(swoops.len(),2);
    assert!(swoops.iter().all(|c|!c.upgraded && c.floor_added_to_deck==Some(12)));
    assert_eq!(mgr.run_ctx().next_card_id,next+2);
    assert_eq!(mgr.run_ctx().rngs.niche,counter);
}

#[test]
fn lantern_key_extra_preserves_ordinary_rewards_rng_and_modifiers() {
    let mut ordinary=manager();
    ordinary.run_ctx_mut().total_floor=10;
    // Allocate the same pre-combat event card on both sides. Generated card
    // instance IDs must not differ merely because the control skipped setup.
    let id=ordinary.run_ctx_mut().new_card_id();
    ordinary.run_ctx_mut().relics.push(Relic::new(RelicId::PrayerWheel));
    ordinary.run_ctx_mut().relics.push(Relic::new(RelicId::AmethystAubergine));
    let mut lantern=ordinary.clone();
    lantern.lantern_key_reward=Some(Card::from_factory(CardId::LanternKey,false,id).unwrap());
    ordinary.generate_combat_rewards(RoomType::Monster);
    lantern.generate_combat_rewards(RoomType::Monster);
    assert_eq!(lantern.unported,None);
    assert_eq!(lantern.run_ctx().gold,ordinary.run_ctx().gold);
    assert_eq!(lantern.run_ctx().rngs.rewards,ordinary.run_ctx().rngs.rewards);
    assert_eq!(lantern.potion_reward_odds,ordinary.potion_reward_odds);
    assert_eq!(lantern.card_rarity_offset,ordinary.card_rarity_offset);
    let extras:Vec<_>=lantern.reward.items.iter()
        .filter(|r|matches!(r.kind,RewardItemKind::SpecialCard{..})).collect();
    assert_eq!(extras.len(),1);
    assert!(!extras[0].skippable);
    let non_extra:Vec<_>=lantern.reward.items.iter()
        .filter(|r|!matches!(r.kind,RewardItemKind::SpecialCard{..}))
        .map(|r|format!("{:?}",r.kind)).collect();
    let expected:Vec<_>=ordinary.reward.items.iter().map(|r|format!("{:?}",r.kind)).collect();
    assert_eq!(non_extra,expected);
    assert!(lantern.lantern_key_reward.is_none());
}

#[test]
fn white_beast_statue_forces_potion_without_odds_roll_before_gold() {
    for room in [RoomType::Monster,RoomType::Elite,RoomType::Boss] {
        let mut mgr=RunManager::new(7,RunOptions{ascension:0,..RunOptions::default()});
        mgr.run_ctx_mut().relics.push(Relic::new(RelicId::WhiteBeastStatue));
        let odds=mgr.potion_reward_odds;
        let mut rng=mgr.run_ctx().rngs.rewards.clone();
        let (lo,hi)=match room {RoomType::Monster=>(10,20),RoomType::Elite=>(35,45),_=>(100,100)};
        let expected_gold=rng.next_int(lo,hi) as i32;
        let gold=mgr.run_ctx().gold;
        mgr.generate_combat_rewards(room);
        assert_eq!(mgr.potion_reward_odds,odds);
        assert_eq!(mgr.run_ctx().gold-gold,expected_gold);
        assert!(mgr.reward.items.iter().any(|item|matches!(item.kind,RewardItemKind::Potion(_))));
    }
}

#[test]
fn round_tea_party_defers_fight_damage_and_relic_until_continue() {
    let mut mgr=manager();
    mgr.run_ctx_mut().hp=40;
    mgr.current_event=super::super::events::initialize(EventId::RoundTeaParty,7,99,24,40,80);
    mgr.phase=RunPhase::Event;
    let relics=mgr.run_ctx().relics.len();
    let initial=&mgr.current_event.as_ref().unwrap().options;
    assert_eq!(initial[0].native_id(),crate::identity::EventOptionId::RoundTeaPartyInitialEnjoyTea);
    assert_eq!(initial[1].native_id(),crate::identity::EventOptionId::RoundTeaPartyInitialPickFight);
    mgr.take_event_choice("pick_fight").unwrap();
    assert_eq!(mgr.run_ctx().hp,40);
    assert_eq!(mgr.run_ctx().relics.len(),relics);
    assert_eq!(mgr.current_event.as_ref().unwrap().options[0].option_id,"continue_fight");
    assert_eq!(mgr.current_event.as_ref().unwrap().options.page.id,crate::identity::EventPageId::RoundTeaPartyPickFight);
    assert_eq!(mgr.current_event.as_ref().unwrap().options[0].native_id(),crate::identity::EventOptionId::RoundTeaPartyPickFightContinueFight);
    mgr.take_event_choice("continue_fight").unwrap();
    assert_eq!(mgr.run_ctx().hp,29);
    assert_eq!(mgr.run_ctx().relics.len(),relics+1);
    assert!(mgr.current_event.is_none());
}

#[test]
fn tanx_observation_maps_live_option_to_retained_relic_without_rng() {
    let mut mgr=manager();
    let mut event=EventState::new(EventId::Tanx,Rng::new(91));
    super::super::tanx::options(&mut event,&mgr.run_ctx().deck);
    let expected=event.relics.clone();
    let rng=event.rng.clone();
    mgr.current_event=Some(event);
    for (i,id) in expected.iter().enumerate() {
        assert_eq!(mgr.observation_ancient_relic(["relic_1","relic_2","relic_3"][i]),Some(*id));
    }
    assert_eq!(mgr.observation_ancient_relic("relic_4"),None);
    assert_eq!(mgr.current_event.as_ref().unwrap().rng,rng);
}

#[test]
fn defect_scroll_boxes_checks_each_claw_bundle_before_ordinary_selection() {
    use super::super::mp::Character;
    let base=RunManager::new_for_character(7,RunOptions::default(),Character::Defect);
    let pool=super::super::card_pools::cards(CardPool::Defect,false).unwrap();
    let commons=pool.iter().filter(|&&id|card_info(id,false).unwrap().rarity==CardRarity::Common).count();
    let uncommons=pool.iter().filter(|&&id|card_info(id,false).unwrap().rarity==CardRarity::Uncommon).count();
    let mut saw_claw=false;
    for seed in 0..512 {
        let mut mgr=base.clone();
        mgr.run_ctx_mut().rngs.rewards=Rng::new(seed);
        let mut expected=Rng::new(seed);
        let mut normal=0;
        let mut claw=[false;2];
        for is_claw in &mut claw {
            *is_claw=expected.next_int_exclusive(0,100)<1;
            if *is_claw {saw_claw=true;continue;}
            expected.choice_index(commons-2*normal);
            expected.choice_index(commons-2*normal-1);
            expected.choice_index(uncommons-normal);
            normal+=1;
        }
        mgr.offer_neow_card_bundles().unwrap();
        assert_eq!(mgr.run_ctx().rngs.rewards,expected);
        let RewardItemKind::CardBundles{bundles}=&mgr.reward.items[0].kind else {panic!("missing bundles");};
        for (i,is_claw) in claw.into_iter().enumerate() {
            if is_claw {assert!(bundles[i].iter().all(|c|c.id==CardId::Claw));}
        }
    }
    assert!(saw_claw);
}

#[test]
fn floor_gated_events_use_history_before_destination_room_is_appended() {
    let mut mgr=manager();
    mgr.run_ctx_mut().total_floor=6;
    assert!(!mgr.event_is_allowed(EventId::PunchOff));
    mgr.run_ctx_mut().total_floor=7;
    assert!(mgr.event_is_allowed(EventId::PunchOff));
    assert!(!mgr.event_is_allowed(EventId::SlipperyBridge));
    mgr.run_ctx_mut().total_floor=8;
    assert!(mgr.event_is_allowed(EventId::SlipperyBridge));
}

#[test]
fn shiv_run_action_tracks_fan_of_knives_live_target_type() {
    // C# Shiv.TargetType changes from AnyEnemy to AllEnemies while its owner
    // has FanOfKnives. The run action must agree with the combat wire binding.
    let mut mgr=manager();
    let mut combat=Combat::new(mgr.take_run_from_owner(),1);
    combat.setup_encounter("TUNNELER_WEAK",&mut Rng::new(1));
    combat.start_combat();
    combat.players[0].hand.clear();
    let shiv=combat.new_card(0,CardId::Shiv,false).unwrap();
    combat.players[0].hand.push(shiv);
    mgr.combat=Some(combat);
    mgr.phase=RunPhase::Combat;
    let targeted=*mgr.available_actions().iter()
        .find(|a|a.kind==RunActionKind::PlayCard && a.card_id==Some(CardId::Shiv)).unwrap();
    assert_eq!(targeted.target_index,Some(0));
    let combat=mgr.combat.as_mut().unwrap();
    combat.apply_power_to(0,PowerId::FanOfKnives,1,Some(0));
    assert_eq!(combat.card_target_type(shiv),TargetType::AllEnemies);
    let plays:Vec<_>=mgr.available_actions().iter().copied()
        .filter(|a|a.kind==RunActionKind::PlayCard && a.card_id==Some(CardId::Shiv)).collect();
    assert_eq!(plays.len(),1);
    assert_eq!(plays[0].target_index,None);
    mgr.take_action(plays[0]).unwrap();
}

#[test]
fn shiv_canonical_vars_are_only_the_csharp_damage_var() {
    // Models/Cards/Shiv.cs: CanonicalVars is DamageVar(4); OnUpgrade adds 2.
    // The legacy table's calc_base/calc_extra are not permanent mutations.
    for (upgraded, damage) in [(false, 4), (true, 6)] {
        let card = Card::from_factory(CardId::Shiv, upgraded, 1).unwrap();
        assert_eq!(card.damage, Some(damage));
        assert_eq!(card.vars.as_slice(), &[(crate::tables::VarKey::Damage, damage)]);
    }
}

#[test]
fn entering_next_act_does_not_report_the_unvisited_start_as_current_location() {
    // RunManager.cs EnterAct / SetActInternal clears visited coordinates and
    // enters MapRoom; only EnterMapCoord enters the next Ancient's coordinate.
    let mut mgr = manager();
    assert!(mgr.observation_map_position().is_some());
    mgr.transition_next_act().unwrap();
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
    assert_eq!(mgr.observation_map_position(), None);
    let travel = *mgr.available_actions().iter()
        .find(|a| a.kind == RunActionKind::Move).unwrap();
    let coord = travel.coord.unwrap();
    mgr.take_action(travel).unwrap();
    assert_eq!(mgr.observation_map_position(), Some((coord.col, coord.row)));
}

#[test]
fn lasting_candy_survives_real_combat_dispatch_and_reward_claims() {
    let mut mgr = manager();
    mgr.run_ctx_mut().hp = 1000;
    mgr.run_ctx_mut().max_hp = 1000;
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::LastingCandy));
    for completed in 0..3 {
        // Set up a real encounter, but do not call reward generators/hooks or
        // mutate enemies. All fighting and combat completion use RunAction.
        mgr.enter_combat(RoomType::Monster).unwrap();
        assert_eq!(mgr.run_ctx().relics.iter().find(|r| r.id == RelicId::LastingCandy).unwrap().counter, completed);
        for _ in 0..1000 {
            if mgr.phase() != RunPhase::Combat { break; }
            let actions = mgr.available_actions();
            let action = actions.iter().find(|a| a.enabled && a.kind == RunActionKind::PlayCard)
                .or_else(|| actions.iter().find(|a| a.enabled && a.kind == RunActionKind::EndTurn)).unwrap();
            mgr.take_action(*action).unwrap();
            assert_eq!(mgr.unported, None);
        }
        assert_eq!(mgr.phase(), RunPhase::CardReward);
        assert!(mgr.combat().unwrap().won);
        let cards = mgr.reward.items.iter().find_map(|r| match &r.kind {
            RewardItemKind::Card { cards, .. } => Some(cards), _ => None,
        }).unwrap();
        assert_eq!(cards.len(), if completed == 1 { 4 } else { 3 });
        if completed == 1 { assert_eq!(cards[3].card_type, CardType::Power); }
        assert_eq!(mgr.run_ctx().relics.iter().find(|r| r.id == RelicId::LastingCandy).unwrap().counter, completed + 1);
        for _ in 0..10 {
            if mgr.phase() != RunPhase::CardReward { break; }
            let skip = *mgr.available_actions().iter().find(|a|
                matches!(a.kind, RunActionKind::Skip | RunActionKind::SkipRewards)).unwrap();
            mgr.take_action(skip).unwrap();
        }
        assert_eq!(mgr.phase(), RunPhase::MapChoice);
        assert_eq!(mgr.run_ctx().relics.iter().find(|r| r.id == RelicId::LastingCandy).unwrap().counter, completed + 1);
    }
}

#[test]
fn orrery_prismatic_pool_matches_owner_union_and_independent_rewards_rng() {
    for owner in [0, 1] {
        let mut mgr = manager();
        mgr.phase = RunPhase::Shop;
        mgr.run_ctx_mut().room_type = RoomType::Shop;
        mgr.run_ctx_mut().gold = 1000;
        let mut gem = Relic::new(RelicId::PrismaticGem);
        gem.owner = owner;
        mgr.run_ctx_mut().relics.push(gem);
        mgr.shop.relics.push(shop::ShopRelicEntry {
            relic_id: RelicId::Orrery, rarity: RelicRarity::Shop, price: 175,
        });
        let characters: &[crate::character::Character] = if owner == 0 { &orobas::CHARACTERS } else { &[crate::character::Character::Ironclad] };
        let pool: Vec<_> = characters.iter().flat_map(|c|
            crate::run::card_pools::cards((*c).into(), false).unwrap()).collect();
        let mut rng = mgr.run_ctx().rngs.rewards.clone();
        let mut expected = Vec::new();
        for _ in 0..5 {
            let mut chosen = Vec::new();
            for _ in 0..3 {
                let roll = rng.next_float(1.0);
                let rarity = if roll < 0.0149 { CardRarity::Rare }
                    else if roll < 0.37 + 0.0149 { CardRarity::Uncommon } else { CardRarity::Common };
                let candidates: Vec<_> = pool.iter().copied().filter(|id|
                    card_info(*id, false).unwrap().rarity == rarity && !chosen.contains(id)).collect();
                chosen.push(candidates[rng.choice_index(candidates.len())]);
                rng.next_float(1.0); // C# always consumes the upgrade roll
            }
            expected.push(chosen);
        }
        let buy = *mgr.available_actions().iter().find(|a| a.kind == RunActionKind::BuyRelic).unwrap();
        mgr.take_action(buy).unwrap();
        assert_eq!(mgr.unported, None);
        for (item, ids) in mgr.reward.items.iter().zip(expected) {
            let RewardItemKind::Card { cards, .. } = &item.kind else { unreachable!() };
            assert_eq!(cards.iter().map(|c| c.id).collect::<Vec<_>>(), ids);
        }
        assert_eq!(mgr.run_ctx().rngs.rewards.counter, rng.counter);
        assert_eq!(mgr.reward.items.len(), 5);
    }
}

#[test]
fn prismatic_gem_does_not_replace_colorless_reward_pools() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::PrismaticGem));
    assert_eq!(mgr.reward_card_pool(crate::character::CardPool::Colorless).unwrap(),
        crate::run::card_pools::cards(CardPool::Colorless, false).unwrap());
}

#[test]
fn lasting_candy_alternates_combat_rewards_and_counts_once_per_set() {
    let mut mgr = manager();
    mgr.run_ctx_mut().room_type = RoomType::Monster;
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::LastingCandy));
    for reward in 0..4 {
        mgr.generate_combat_rewards(RoomType::Monster);
        assert_eq!(mgr.unported, None);
        let cards = mgr.reward.items.iter().find_map(|item| match &item.kind {
            RewardItemKind::Card { cards, .. } => Some(cards), _ => None,
        }).unwrap();
        assert_eq!(cards.len(), if reward % 2 == 1 { 4 } else { 3 });
        if cards.len() == 4 {
            assert_eq!(cards[3].card_type, CardType::Power);
            assert!(cards[..3].iter().all(|c| c.id != cards[3].id));
        }
        assert_eq!(mgr.run_ctx().relics.last().unwrap().counter, reward + 1);
    }
}

#[test]
fn lasting_candy_ignores_other_sources_and_other_owners() {
    for owner in [0, 1] {
        let mut mgr = manager();
        mgr.run_ctx_mut().room_type = RoomType::Monster;
        let mut candy = Relic::new(RelicId::LastingCandy);
        candy.owner = owner;
        candy.counter = 1;
        mgr.run_ctx_mut().relics.push(candy);
        let item = mgr.make_noncombat_card_reward(3, CardPool::Ironclad, None, true, true, false).unwrap();
        let RewardItemKind::Card { cards, .. } = item.kind else { unreachable!() };
        assert_eq!(cards.len(), 3);
        assert_eq!(mgr.run_ctx().relics.last().unwrap().counter, 1);
        if owner == 1 {
            mgr.generate_combat_rewards(RoomType::Monster);
            assert_eq!(mgr.run_ctx().relics.last().unwrap().counter, 1);
        }
        assert_eq!(mgr.unported, None);
    }
}

#[test]
fn lasting_candy_retains_boss_odds_without_changing_encounter_rarity_offset() {
    let mut mgr = manager();
    mgr.run_ctx_mut().room_type = RoomType::Elite;
    let mut candy = Relic::new(RelicId::LastingCandy);
    candy.counter = 1;
    mgr.run_ctx_mut().relics.push(candy);
    mgr.card_rarity_offset = 0.123;
    let mut cards = ArrayVec::new();
    cards.push(Card::from_factory(CardId::StrikeIronclad, false, 100).unwrap());
    let rng = mgr.run_ctx().rngs.rewards.counter;
    mgr.apply_encounter_reward_relic_hooks(&mut cards, RoomType::Boss);
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[1].card_type, CardType::Power);
    assert_eq!(cards[1].rarity, CardRarity::Rare);
    assert_eq!(mgr.card_rarity_offset, 0.123);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, rng + 3);
    assert_eq!(mgr.run_ctx().relics.last().unwrap().counter, 1);
    assert_eq!(mgr.unported, None);
}

#[test]
fn lava_lamp_reward_upgrade_respects_room_damage_and_owner_without_rng() {
    for room in [RoomType::Monster, RoomType::Elite, RoomType::Boss, RoomType::Shop] {
        for damaged in [false, true] { for owner in [0, 1] { for card_reward in [false, true] {
            let mut mgr = manager();
            mgr.run_ctx_mut().room_type = room;
            let mut lamp = Relic::new(RelicId::LavaLamp);
            lamp.owner = owner;
            lamp.flag1 = damaged;
            mgr.run_ctx_mut().relics.push(lamp);
            let mut cards = ArrayVec::new();
            cards.push(Card::from_factory(CardId::StrikeIronclad, false, 100).unwrap());
            cards.push(Card::from_factory(CardId::Inflame, true, 101).unwrap());
            let rng = mgr.run_ctx().rngs.rewards.counter;
            mgr.apply_card_reward_relic_hooks(&mut cards, card_reward);
            assert_eq!(cards[0].upgraded, owner == 0 && !damaged && room != RoomType::Shop);
            assert!(cards[1].upgraded);
            assert_eq!(cards[0].instance_id, 100);
            assert_eq!(mgr.run_ctx().rngs.rewards.counter, rng);
            assert_eq!(mgr.run_ctx().relics.last().unwrap().counter, 0);
            assert_eq!(mgr.unported, None);
        }}}
    }
}

#[test]
fn spoils_map_overrides_only_act_two_while_advancing_the_standard_map_stream() {
    for held in [false, true] {
        let mut mgr = RunManager::new(4100134, RunOptions::default());
        mgr.unported = None;
        if held {
            mgr.add_card_to_deck(CardId::from_name("SPOILS_MAP").unwrap()).unwrap();
        }
        let counter = mgr.map_rngs[1].counter;
        mgr.transition_next_act().unwrap();
        assert!(mgr.map_rngs[1].counter > counter);
        let map = mgr.acts[1].map.as_ref().unwrap();
        let columns: Vec<_> = map.row_indices(1).iter()
            .map(|&i| map.points[i as usize].coord.col).collect();
        // Frozen standard and Spoils generators independently give these rows.
        assert_eq!(columns, if held { vec![0, 3, 5, 6] } else { vec![0, 2, 4, 6] });
        mgr.transition_next_act().unwrap();
        assert!(mgr.acts[2].map.as_ref().unwrap().second_boss.is_some());
    }
}

pub(super) fn finish_nr_combat(mgr: &mut RunManager, room: RoomType) {
    dispatch_nr_combat(mgr, room, 1);
    assert_eq!(mgr.phase(), RunPhase::CardReward);
    assert!(mgr.unported.is_none(), "{:?}", mgr.unported);
}

#[test]
fn spoils_treasure_quest_pays_and_removes_registered_cards_through_normal_dispatch() {
    for registered in [0, 1, 2] {
        let mut mgr = RunManager::new(4100134, RunOptions::default());
        mgr.unported = None;
        let spoils = CardId::from_name("SPOILS_MAP").unwrap();
        for _ in 0..registered { mgr.add_card_to_deck(spoils).unwrap(); }
        mgr.transition_next_act().unwrap();
        // A later-added quest must not claim an unmarked ordinary treasure.
        if registered == 0 { mgr.add_card_to_deck(spoils).unwrap(); }
        let map = mgr.acts[1].map.as_ref().unwrap();
        let target = map.row_indices(map.map_length - 7)[0] as usize;
        assert_eq!(map.points[target].point_type, MapPointType::Treasure);
        assert_eq!(map.points[target].spoils_quests.len(), registered);
        let coord = map.points[target].coord;
        let parent = map.points[target].parents[0];
        // Stage the saved run immediately before this room; entry and collection
        // use the same public action dispatcher as a recorded run.
        mgr.current_point = parent;
        mgr.visited_points.push(parent);
        let mut action = RunAction::new(RunActionKind::Move);
        action.coord = Some(coord);
        mgr.take_action(action).unwrap();
        assert_eq!(mgr.phase, RunPhase::Treasure);
        // Isolate quest completion from arbitrary relic pickup continuations.
        mgr.treasure_relic = Some(RelicId::Vajra);
        let before_gold = mgr.run_ctx().gold;
        let mut rewards = mgr.run_ctx().rngs.rewards.clone();
        // C# OneOffSynchronizer: NextInt(42,53), then Poverty's 0.75.
        let chest_gold = (rewards.next_int_exclusive(42, 53) as f64 * 0.75) as i32;
        mgr.take_action(RunAction::new(RunActionKind::Collect)).unwrap();
        assert_eq!(mgr.phase, RunPhase::MapChoice);
        assert!(mgr.unported.is_none(), "{:?}", mgr.unported);
        assert_eq!(mgr.run_ctx().gold, before_gold + chest_gold + 600 * registered as i32);
        assert_eq!(mgr.run_ctx().rngs.rewards.counter, rewards.counter);
        assert_eq!(mgr.run_ctx().deck.iter().filter(|card| card.id == spoils).count(),
            usize::from(registered == 0));
        assert!(mgr.acts[1].map.as_ref().unwrap().points[target].spoils_quests.is_empty());
        let gold = mgr.run_ctx().gold;
        assert!(mgr.take_action(RunAction::new(RunActionKind::Collect)).is_err());
        assert_eq!(mgr.run_ctx().gold, gold);
    }
}

fn dispatch_nr_combat(mgr: &mut RunManager, room: RoomType, players: usize) {
    let mut run = mgr.take_run_from_owner();
    run.room_type = room;
    let mut c = Combat::new(run, 1);
    for _ in 1..players { c.add_player(50, 50, 3, 0, &[]); }
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    c.start_combat();
    c.players[0].hand.clear();
    for enemy in c.enemies.clone() { c.cr_mut(enemy).hp = 1; }
    let k = c.new_card(0, CardId::Thunderclap, false).unwrap();
    c.players[0].hand.push(k);
    c.players[0].energy = 3;
    mgr.combat = Some(c);
    mgr.phase = RunPhase::Combat;
    let mut action = RunAction::new(RunActionKind::PlayCard);
    action.hand_index = Some(0);
    action.target_index = Some(0);
    mgr.take_action(action).unwrap();
}

#[test]
fn pantograph_heals_once_at_combat_start_not_again_on_run_room_entry() {
    let mut mgr = manager();
    mgr.run_ctx_mut().hp = 40;
    mgr.run_ctx_mut().max_hp = 100;
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::Pantograph));
    // Pantograph.cs:16-24 only updates display status on room entry;
    // BeforeCombatStart is the single 25-HP heal (27-33).
    rest::after_room_entered(mgr.run_ctx_mut(), RoomType::Boss, MapPointType::Boss);
    assert_eq!(mgr.run_ctx().hp, 40);
    mgr.enter_encounter(RoomType::Boss, EncounterId::TheKinBoss).unwrap();
    assert_eq!(mgr.combat().unwrap().player().hp, 65);
}

#[test]
fn punch_dagger_shop_pickup_enchants_one_eligible_attack_and_resumes_shop() {
    // C# PunchDagger.AfterObtained / Momentum.CanEnchantCardType.
    for eligible in [true, false] {
        let mut mgr = manager();
        mgr.phase = RunPhase::Shop;
        mgr.run_ctx_mut().room_type = RoomType::Shop;
        mgr.run_ctx_mut().gold = 1000;
        let defend = Card::from_factory(CardId::DefendIronclad, false, 1).unwrap();
        let mut enchanted = Card::from_factory(CardId::Bash, false, 2).unwrap();
        assert!(crate::card::enchant::apply_static_enchantment(&mut enchanted, "Momentum", 5));
        mgr.run_ctx_mut().deck = vec![defend, enchanted];
        if eligible {
            mgr.run_ctx_mut().deck.push(Card::from_factory(CardId::StrikeIronclad, false, 3).unwrap());
        }
        mgr.run_ctx_mut().next_card_id = 4;
        mgr.shop.relics.push(shop::ShopRelicEntry {
            relic_id: RelicId::PunchDagger, rarity: RelicRarity::Shop, price: 175,
        });
        mgr.buy_shop_relic(0).unwrap();
        assert_eq!(mgr.unported, None);
        assert_eq!(mgr.run_ctx().gold, 825);
        assert!(mgr.run_ctx().has_owned_relic(crate::combat::PLAYER, RelicId::PunchDagger));
        assert_eq!(mgr.shop.relics[0].price, shop::SOLD_OUT);
        if eligible {
            // CardSelectCmd.FromDeckForEnchantment auto-selects a singleton.
            assert!(mgr.observation_run_choice().is_none());
            assert_eq!(mgr.run_ctx().deck[2].enchantments.as_slice(), &[(crate::identity::EnchantmentId::Momentum, 5)]);
        } else {
            assert!(mgr.observation_run_choice().is_none());
        }
        assert_eq!(mgr.phase(), RunPhase::Shop);
        assert!(mgr.run_ctx().deck[0].enchantments.is_empty());
        assert_eq!(mgr.run_ctx().deck[1].enchantments.as_slice(), &[(crate::identity::EnchantmentId::Momentum, 5)]);
    }
}

#[test]
fn cauldron_shop_pickup_populates_five_independent_potions_and_returns_to_shop() {
    let mut mgr = manager();
    mgr.run_ctx_mut().room_type = RoomType::Shop;
    mgr.run_ctx_mut().gold = 1000;
    mgr.phase = RunPhase::Shop;
    mgr.shop.relics.push(shop::ShopRelicEntry {
        relic_id: RelicId::Cauldron, rarity: RelicRarity::Shop, price: 175,
    });
    let draws = mgr.run_ctx().rngs.rewards.counter;
    mgr.buy_shop_relic(0).unwrap();
    assert!(mgr.unported.is_none());
    assert_eq!(mgr.phase(), RunPhase::CardReward);
    assert_eq!(mgr.run_ctx().gold, 825);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, draws + 10);
    assert_eq!(mgr.reward.items.len(), 5);
    assert!(mgr.reward.active.is_none());
    assert!(mgr.reward.items.iter().all(|r| matches!(r.kind, RewardItemKind::Potion(_))));
    let mut skipped = mgr.clone();
    let skip = *skipped.available_actions().iter().find(|a| a.kind == RunActionKind::SkipRewards).unwrap();
    skipped.take_action(skip).unwrap();
    assert_eq!(skipped.phase(), RunPhase::Shop);
    mgr.run_ctx_mut().potions.clear();
    mgr.run_ctx_mut().max_potion_slots = 5;
    for remaining in (1..=5).rev() {
        let actions = mgr.available_actions();
        assert_eq!(actions.iter().filter(|a| a.kind == RunActionKind::PickPotion).count(), remaining);
        assert!(actions.iter().all(|a| a.kind != RunActionKind::SkipPotion));
        let pick = *actions.iter().find(|a| a.kind == RunActionKind::PickPotion).unwrap();
        mgr.take_action(pick).unwrap();
        assert!(mgr.reward.active.is_none(), "claim must not auto-open the next potion");
        if remaining > 1 {
            assert_eq!(mgr.phase(), RunPhase::CardReward);
        }
    }
    assert_eq!(mgr.phase(), RunPhase::Shop);
    assert_eq!(mgr.shop.relics[0].price, shop::SOLD_OUT);
}

#[test]
fn orrery_shop_pickup_offers_five_character_rewards_and_returns_to_shop() {
    let mut mgr = manager();
    mgr.run_ctx_mut().room_type = RoomType::Shop;
    mgr.run_ctx_mut().gold = 1000;
    mgr.phase = RunPhase::Shop;
    mgr.shop.relics.push(shop::ShopRelicEntry {
        relic_id: RelicId::Orrery, rarity: RelicRarity::Shop, price: 175,
    });
    let before = mgr.run_ctx().deck.len();
    let buy = *mgr.available_actions().iter().find(|a| a.kind == RunActionKind::BuyRelic).unwrap();
    mgr.take_action(buy).unwrap();
    assert_eq!(mgr.unported, None);
    assert_eq!(mgr.phase(), RunPhase::CardReward);
    assert_eq!(mgr.run_ctx().gold, 825);
    assert_eq!(mgr.reward.items.len(), 5);
    let pool = crate::run::card_pools::cards(CardPool::Ironclad, false).unwrap();
    for item in &mgr.reward.items {
        let RewardItemKind::Card { cards, .. } = &item.kind else { panic!("not a card reward") };
        assert_eq!(cards.len(), 3);
        assert!(cards.iter().all(|c| pool.contains(&c.id)));
    }
    for remaining in (1..=5).rev() {
        let actions = mgr.available_actions();
        assert_eq!(actions.iter().filter(|a| a.kind == RunActionKind::OpenReward).count(), remaining);
        let open = *actions.iter().find(|a| a.kind == RunActionKind::OpenReward).unwrap();
        mgr.take_action(open).unwrap();
        let pick = *mgr.available_actions().iter().find(|a| a.kind == RunActionKind::PickCard).unwrap();
        mgr.take_action(pick).unwrap();
        assert!(mgr.reward.active.is_none());
        assert_eq!(mgr.unported, None);
    }
    assert_eq!(mgr.phase(), RunPhase::Shop);
    assert_eq!(mgr.run_ctx().deck.len(), before + 5);
    assert_eq!(mgr.shop.relics[0].price, shop::SOLD_OUT);
}

#[test]
fn paels_wing_adds_sacrifice_alternative_without_modifying_reward_cards() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::PaelsWing));
    mgr.phase = RunPhase::Combat;
    mgr.generate_combat_rewards(RoomType::Monster);
    assert_eq!(mgr.phase(), RunPhase::CardReward);
    assert_eq!(mgr.unported, None);
    let sacrifice = *mgr.available_actions().iter()
        .find(|a| a.kind == RunActionKind::SacrificeCardReward).unwrap();
    mgr.take_action(sacrifice).unwrap();
    assert_eq!(mgr.run_ctx().relics.iter().find(|r| r.id == RelicId::PaelsWing).unwrap().counter, 1);
}

#[test]
fn selector_screen_identity_covers_candidates_selection_purpose_phase_and_routes() {
    let mut mgr=manager();
    mgr.begin_run_card_choice("test.remove_two", "Choose 2 cards to remove",&[0,1],2,2,false,
        RunChoiceEffect::Remove {damage_after:0},ChoiceResume::MapChoice);
    let c=mgr.observation_run_choice().unwrap();
    let key=mgr.selector_shape_key(&c);
    let mut changed=c.clone();
    changed.options.swap(0,1);
    assert_ne!(key,mgr.selector_shape_key(&changed),"ordered candidates");
    changed=c.clone();
    changed.options[0].card_id=CardId::ExpectAFight;
    assert_ne!(key,mgr.selector_shape_key(&changed),"same-count C# option");
    changed=c.clone();
    changed.selected.push(0);
    assert_ne!(key,mgr.selector_shape_key(&changed),"selection state");
    changed=c.clone();
    changed.prompt="Choose a card to upgrade";
    assert_ne!(key,mgr.selector_shape_key(&changed),"purpose");
    let old=mgr.phase;
    mgr.phase=RunPhase::RestSite;
    assert_ne!(key,mgr.selector_shape_key(&c),"phase");
    mgr.phase=old;
    mgr.run_ctx_mut().deck[0].instance_id+=10000;
    assert_ne!(key,mgr.selector_shape_key(&c),"instance routing");
}

#[test]
fn selector_inventory_key_includes_exact_actions_and_cannot_borrow_bare_proof() {
    let mut mgr=manager();
    mgr.begin_run_card_choice("test.remove_two", "Choose 2 cards to remove",&[0,1],2,2,false,
        RunChoiceEffect::Remove {damage_after:0},ChoiceResume::MapChoice);
    let c=mgr.observation_run_choice().unwrap();
    mgr.run_ctx_mut().potions.clear();
    let bare=mgr.selector_shape_key(&c);
    assert!(!bare.contains(";inventory-candidates-v1="));
    mgr.run_ctx_mut().potions.push(Some(PotionId::Speedpotion));
    let augmented=mgr.selector_shape_key(&c);
    let actions=mgr.available_actions();
    assert!(augmented.ends_with(&format!(";inventory-candidates-v1={actions:?}")));
    assert!(actions.iter().any(|a|a.kind==RunActionKind::DiscardPotion && a.slot_index==Some(0)));
    assert_ne!(bare,augmented);
    mgr.run_ctx_mut().potions.insert(0,None);
    let shifted=mgr.selector_shape_key(&c);
    assert_ne!(shifted,augmented,"same potion must not borrow another slot route");
    assert!(mgr.available_actions().iter().any(|a|
        a.kind==RunActionKind::DiscardPotion && a.slot_index==Some(1)));
    assert!(!mgr.observation_verified_selector(),"no inventory-complete witness yet");
}

#[test]
fn nonzero_owner_pickup_paths_fail_before_touching_the_local_run() {
    for relic in [RelicId::PrecariousShears, RelicId::PaelsGrowth,
        RelicId::Whetstone, RelicId::PandorasBox, RelicId::NeowsTalisman,
        RelicId::PotionBelt] {
        for kind in [RunActionKind::EventChoice, RunActionKind::BuyRelic,
            RunActionKind::PickRelicReward, RunActionKind::Collect,
            RunActionKind::RestOption, RunActionKind::Choose,
            RunActionKind::ConfirmChoice, RunActionKind::PickCard] {
            for seat in 1..4 {
                let mut mgr = manager();
                let before = format!("{:?}", mgr.run_ctx());
                let mut action = RunAction::new(kind);
                action.player_id = Some(seat + PLAYER_ID);
                action.relic_id = Some(relic);
                assert_eq!(mgr.take_action(action), Err("owner pickup needs MP run layer"));
                assert_eq!(mgr.unported, Some("owner pickup needs MP run layer"));
                assert_eq!(format!("{:?}", mgr.run_ctx()), before);
            }
        }
    }
}

#[test]
fn b11_noncombat_relic_actions_refuse_teammate_requests_before_mutation() {
    for relic in [RelicId::PrayerWheel, RelicId::Orrery, RelicId::Pear,
        RelicId::PreciseScissors, RelicId::PaelsHorn, RelicId::PreservedFog] {
        for seat in 1..4 {
            for kind in [RunActionKind::BuyRelic, RunActionKind::EventChoice,
                RunActionKind::PickRelicReward, RunActionKind::Choose,
                RunActionKind::ConfirmChoice] {
                let mut mgr = manager();
                let before = format!("{:?}", mgr.run_ctx());
                let reward = format!("{:?}", mgr.reward);
                let mut action = RunAction::new(kind);
                action.player_id = Some(PLAYER_ID + seat);
                action.relic_id = Some(relic);
                assert_eq!(mgr.take_action(action), Err("owner pickup needs MP run layer"));
                assert_eq!(format!("{:?}", mgr.run_ctx()), before);
                assert_eq!(format!("{:?}", mgr.reward), reward);
            }
        }
    }
}

#[test]
fn b11_pear_shop_action_adds_ten_hp_without_rng_and_horn_event_adds_two_relax() {
    let mut mgr = manager();
    mgr.phase = RunPhase::Shop;
    mgr.run_ctx_mut().gold = 1000;
    mgr.run_ctx_mut().hp = 20;
    let max_hp = mgr.run_ctx().max_hp;
    let rewards = mgr.run_ctx().rngs.rewards.counter;
    let niche = mgr.run_ctx().rngs.niche.counter;
    mgr.shop.relics.push(shop::ShopRelicEntry {
        relic_id: RelicId::Pear, rarity: RelicRarity::Uncommon, price: 150,
    });
    let buy = mgr.available_actions().into_iter()
        .find(|a| a.kind == RunActionKind::BuyRelic && a.relic_id == Some(RelicId::Pear)).unwrap();
    mgr.take_action(buy).unwrap();
    assert_eq!((mgr.run_ctx().hp, mgr.run_ctx().max_hp), (30, max_hp + 10));
    assert_eq!(mgr.run_ctx().gold, 850);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, rewards);
    assert_eq!(mgr.run_ctx().rngs.niche.counter, niche);
    assert!(mgr.run_ctx().has_owned_relic(0, RelicId::Pear));
    assert!(mgr.unported.is_none());

    let mut mgr = manager();
    let before = mgr.run_ctx().deck.len();
    let rewards = mgr.run_ctx().rngs.rewards.counter;
    let niche = mgr.run_ctx().rngs.niche.counter;
    let mut event = EventState::new(EventId::Pael, Rng::new(7));
    event.options.push(super::super::events::EventOption::new("relic_1", "Relic", "Gain a Pael relic"));
    event.relics.push(RelicId::PaelsHorn);
    mgr.run_ctx_mut().total_floor = 18;
    mgr.current_event = Some(event);
    mgr.phase = RunPhase::Event;
    let pick = *mgr.available_actions().iter().find(|a| a.kind == RunActionKind::EventChoice).unwrap();
    mgr.take_action(pick).unwrap();
    assert_eq!(mgr.run_ctx().deck.len(), before + 2);
    assert!(mgr.run_ctx().deck[before..].iter().all(|c| c.id == CardId::Relax && c.owner == 0));
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, rewards);
    assert_eq!(mgr.run_ctx().rngs.niche.counter, niche);
    assert!(mgr.unported.is_none());
}

#[test]
fn b11_scissors_normal_neow_and_choice_actions_remove_one_without_rng() {
    let mut mgr = manager();
    mgr.neow[0].relic = RelicId::PreciseScissors;
    let option = mgr.neow[0].option_id;
    let before = mgr.run_ctx().deck.len();
    let rewards = mgr.run_ctx().rngs.rewards.counter;
    let niche = mgr.run_ctx().rngs.niche.counter;
    let pick = *mgr.available_actions().iter().find(|a|
        a.kind == RunActionKind::EventChoice && a.option_id == Some(option.legacy_id())).unwrap();
    mgr.take_action(pick).unwrap();
    let remove = *mgr.available_actions().iter().find(|a| a.kind == RunActionKind::Choose).unwrap();
    mgr.take_action(remove).unwrap();
    assert_eq!(mgr.run_ctx().deck.len(), before - 1);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, rewards);
    assert_eq!(mgr.run_ctx().rngs.niche.counter, niche);
    assert!(mgr.unported.is_none());
}

#[test]
fn b11_foreign_prayer_wheel_normal_dispatch_refuses_mp_run_before_rewards() {
    for owner in 1..4 {
        let mut baseline = manager();
        let mut actual = baseline.clone();
        let mut wheel = Relic::new(RelicId::PrayerWheel);
        wheel.owner = owner;
        actual.run_ctx_mut().relics.push(wheel);
        // A foreign relic needs a real owner in the combat roster. The legacy
        // solo run manager then refuses that extended roster before rewards.
        dispatch_nr_combat(&mut baseline, RoomType::Monster, owner as usize + 1);
        dispatch_nr_combat(&mut actual, RoomType::Monster, owner as usize + 1);
        assert_eq!(format!("{:?}", actual.reward), format!("{:?}", baseline.reward));
        assert_eq!(actual.run_ctx().rngs.rewards.counter, baseline.run_ctx().rngs.rewards.counter);
        assert_eq!(actual.run_ctx().next_card_id, baseline.run_ctx().next_card_id);
        assert_eq!(actual.unported, Some("multiplayer run construction is unported"));
        assert!(actual.reward.items.is_empty());
    }
}

#[test]
fn foreign_regal_pillow_does_not_change_seat_zero_rest_action_healing() {
    for owner in 1..4 {
        let mut mgr = manager();
        let mut relic = Relic::new(RelicId::RegalPillow); relic.owner = owner;
        mgr.run_ctx_mut().relics.push(relic);
        mgr.run_ctx_mut().max_hp = 100; mgr.run_ctx_mut().hp = 10;
        let mut state = rest::RestState::default();
        rest::generate(&mut state, mgr.run_ctx_mut());
        mgr.rest = state; mgr.phase = RunPhase::RestSite;
        let action = mgr.available_actions().into_iter()
            .find(|a| a.kind == RunActionKind::RestOption && a.option_id == Some("HEAL")).unwrap();
        mgr.take_action(action).unwrap();
        assert_eq!(mgr.run_ctx().hp, 40);
        assert!(mgr.unported.is_none());
    }
}

#[test]
fn amethyst_fixed_reward_adds_gold_without_an_extra_rewards_rng_draw() {
    let mut base = manager();
    let mut aubergine = base.clone();
    aubergine.run_ctx_mut().relics.push(Relic::new(RelicId::AmethystAubergine));
    base.generate_combat_rewards(RoomType::Monster);
    aubergine.generate_combat_rewards(RoomType::Monster);
    assert_eq!(aubergine.run_ctx().gold, base.run_ctx().gold + 15);
    assert_eq!(aubergine.run_ctx().rngs.rewards.counter, base.run_ctx().rngs.rewards.counter);
}

#[test]
fn pael_direct_deck_selectors_keep_event_phase_until_resolved() {
    for id in [RelicId::PaelsTooth, RelicId::PaelsGrowth] {
        let mut mgr = manager();
        pick_pael(&mut mgr, id);
        assert_eq!(mgr.phase(), RunPhase::Event);
        assert!(mgr.pending_run_choice.is_some());
        if id == RelicId::PaelsTooth {
            assert_eq!(mgr.pending_run_choice.as_ref().unwrap().prompt, "Choose 5 cards for Pael's Tooth");
            for i in 0..5 { mgr.resolve_run_choice(i).unwrap(); }
        } else {
            mgr.resolve_run_choice(0).unwrap();
        }
        assert_eq!(mgr.phase(), RunPhase::MapChoice);
    }
}

#[test]
fn foreign_wing_charm_does_not_block_or_modify_seat_zero_card_rewards() {
    for owner in 1..4 {
        let mut mgr = manager();
        let mut baseline = mgr.clone();
        let mut relic = Relic::new(RelicId::WingCharm); relic.owner = owner;
        mgr.run_ctx_mut().relics.push(relic);
        baseline.generate_combat_rewards(RoomType::Monster);
        mgr.generate_combat_rewards(RoomType::Monster);
        assert_eq!(format!("{:?}", mgr.reward), format!("{:?}", baseline.reward));
        assert_eq!(mgr.run_ctx().gold, baseline.run_ctx().gold);
        assert_eq!(mgr.run_ctx().rngs.niche.counter, baseline.run_ctx().rngs.niche.counter);
        assert!(mgr.unported.is_none());
    }
}

#[test]
fn foreign_courier_has_no_discount_or_refill_on_normal_shop_actions() {
    for owner in 1..4 {
        let mut mgr = manager(); mgr.run_ctx_mut().gold = 9999;
        let mut baseline = mgr.clone();
        let mut relic = Relic::new(RelicId::TheCourier); relic.owner = owner;
        mgr.run_ctx_mut().relics.push(relic);
        baseline.enter_shop(MapPointType::Shop).unwrap();
        mgr.enter_shop(MapPointType::Shop).unwrap();
        assert_eq!(format!("{:?}", mgr.shop), format!("{:?}", baseline.shop));
        let action = mgr.available_actions().into_iter().find(|a| a.kind == RunActionKind::BuyCard).unwrap();
        let base_action = baseline.available_actions().into_iter().find(|a| a.kind == RunActionKind::BuyCard).unwrap();
        mgr.take_action(action).unwrap(); baseline.take_action(base_action).unwrap();
        assert_eq!(format!("{:?}", mgr.shop), format!("{:?}", baseline.shop));
        assert_eq!(mgr.run_ctx().gold, baseline.run_ctx().gold);
        assert_eq!(format!("{:?}", mgr.run_ctx().deck), format!("{:?}", baseline.run_ctx().deck));
        assert!(mgr.unported.is_none());
    }
}

#[test]
fn foreign_silver_crucible_does_not_modify_normal_rewards_or_treasure() {
    for owner in 1..4 {
        let mut mgr = manager(); let mut baseline = mgr.clone();
        let mut relic = Relic::new(RelicId::SilverCrucible); relic.owner = owner;
        mgr.run_ctx_mut().relics.push(relic);
        baseline.generate_combat_rewards(RoomType::Monster);
        mgr.generate_combat_rewards(RoomType::Monster);
        assert_eq!(format!("{:?}", mgr.reward), format!("{:?}", baseline.reward));
        assert_eq!(mgr.run_ctx().owned_relics(owner).next().unwrap().counter, 0);
        assert_eq!(mgr.run_ctx().rngs.niche.counter, baseline.run_ctx().rngs.niche.counter);
        assert_eq!(treasure::should_generate(mgr.run_ctx()), treasure::should_generate(baseline.run_ctx()));
        rest::after_room_entered(mgr.run_ctx_mut(), RoomType::Treasure, MapPointType::Treasure);
        assert_eq!(mgr.run_ctx().owned_relics(owner).next().unwrap().counter2, 0);
        assert!(mgr.unported.is_none());
    }
}

#[test]
fn touch_of_orobas_replaces_only_local_starter_even_when_teammates_is_first() {
    for owner in 1..4 {
        let mut mgr = manager();
        let mut relic = Relic::new(RelicId::RingOfTheSnake); relic.owner = owner;
        mgr.run_ctx_mut().relics.insert(0, relic);
        mgr.current_event = Some(EventState::new(EventId::Orobas, Rng::new(1)));
        mgr.phase = RunPhase::Event;
        mgr.initialize_orobas().unwrap();
        mgr.current_event.as_mut().unwrap().relics.clear();
        mgr.current_event.as_mut().unwrap().relics.push(RelicId::TouchOfOrobas);
        let option = mgr.current_event.as_ref().unwrap().options[0].option_id;
        mgr.take_event_choice(option.legacy_id()).unwrap();
        assert!(mgr.run_ctx().has_owned_relic(owner, RelicId::RingOfTheSnake));
        assert!(!mgr.run_ctx().has_owned_relic(owner, RelicId::RingOfTheDrake));
        assert!(mgr.run_ctx().has_owned_relic(0, RelicId::BlackBlood));
        assert!(!mgr.run_ctx().has_owned_relic(0, RelicId::BurningBlood));
        assert!(mgr.unported.is_none());
    }
}

#[test]
fn maw_bank_room_gold_uses_player_gold_modifiers() {
    for blocker in [false, true] {
        let mut mgr = manager();
        mgr.run_ctx_mut().relics.push(Relic::new(RelicId::MawBank));
        mgr.run_ctx_mut().relics.push(Relic::new(RelicId::BowlerHat));
        if blocker { mgr.run_ctx_mut().relics.push(Relic::new(RelicId::Ectoplasm)); }
        let gold = mgr.run_ctx().gold;
        rest::after_room_entered(mgr.run_ctx_mut(), RoomType::RestSite, MapPointType::RestSite);
        assert_eq!(mgr.run_ctx().gold, gold + if blocker { 0 } else { 15 });
    }
}

#[test]
fn teammate_relic_does_not_exclude_stock_refills_or_seat_zero_purchase() {
    for owner in 1..4 {
        let mut mgr = manager();
        let mut relic = Relic::new(RelicId::WingCharm); relic.owner = owner;
        mgr.run_ctx_mut().relics.push(relic);
        mgr.relic_bags.player[0] = std::array::from_fn(|_| vec![RelicId::WingCharm]);
        let mut state = shop::ShopState::default();
        let mut bags = mgr.relic_bags.clone();
        shop::generate(&mut state, mgr.run_ctx_mut(), Character::Ironclad, false, &mut bags, 0.0, 0).unwrap();
        assert_eq!(state.relics[0].relic_id, RelicId::WingCharm);
        // Refill must use acquiring owner's inventory too, not party ownership.
        state.relics[0].price = shop::SOLD_OUT;
        let mut bags = mgr.relic_bags.clone();
        shop::refill_relic(&mut state, 0, mgr.run_ctx_mut(), &mut bags).unwrap();
        assert_eq!(state.relics[0].relic_id, RelicId::WingCharm);
        // Normal shop action acquires a separate seat-zero instance.
        mgr.shop = state; mgr.phase = RunPhase::Shop; mgr.run_ctx_mut().gold = 9999;
        let action = mgr.available_actions().into_iter()
            .find(|a| a.kind == RunActionKind::BuyRelic && a.relic_id == Some(RelicId::WingCharm)).unwrap();
        mgr.take_action(action).unwrap();
        assert_eq!(mgr.run_ctx().relics.iter().filter(|r| r.id == RelicId::WingCharm && r.owner == 0).count(), 1);
        assert_eq!(mgr.run_ctx().relics.iter().filter(|r| r.id == RelicId::WingCharm && r.owner == owner).count(), 1);
        assert!(mgr.unported.is_none());
    }
}

#[test]
fn paid_and_parasol_shop_cards_fire_deck_entry_once() {
    for parasol in [false, true] {
        let mut mgr = manager();
        mgr.run_ctx_mut().gold = 100;
        mgr.run_ctx_mut().relics.push(Relic::new(RelicId::LuckyFysh));
        mgr.shop.cards.push(shop::ShopCardEntry {
            card: Some(Card::from_factory(CardId::Anger, false, 100).unwrap()),
            card_type: "Attack", on_sale: false, price: 20,
        });
        let before = mgr.run_ctx().deck.len();
        if parasol { mgr.auto_purchase_shop_inventory().unwrap(); }
        else { mgr.buy_shop_card(0).unwrap(); }
        assert_eq!(mgr.run_ctx().deck.len(), before + 1);
        assert_eq!(mgr.run_ctx().gold, if parasol { 115 } else { 95 });
    }
}

#[test]
fn clone_tooth_and_transform_fire_deck_entry_gold() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::LuckyFysh));
    let gold = mgr.run_ctx().gold;
    crate::card::enchant::apply_static_enchantment(&mut mgr.run_ctx_mut().deck[0], "Clone", 4);
    assert_eq!(rest::execute_clone(mgr.run_ctx_mut()), 1);
    assert_eq!(mgr.run_ctx().gold, gold + 15);
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::PaelsTooth));
    let tooth = mgr.run_ctx().relics.len() - 1;
    mgr.run_ctx_mut().paels_tooth_cards.push(Card::from_factory(CardId::Anger, false, 500).unwrap());
    mgr.run_ctx_mut().paels_tooth_return_one(tooth);
    assert!(mgr.run_ctx().deck.last().unwrap().upgraded);
    assert_eq!(mgr.run_ctx().gold, gold + 30);
    let id = mgr.run_ctx().deck.last().unwrap().instance_id;
    mgr.transform_deck_card_with_rng(id, &mut Rng::new(17)).unwrap();
    assert_eq!(mgr.run_ctx().gold, gold + 45);
}

#[test]
fn chest_gold_precedes_new_bowler_hat_and_keeps_poverty_truncation() {
    // Original C#: Multiplayer/Game/OneOffSynchronizer.cs:118-123 pays gold
    // on chest open, before relic selection. A newly selected Bowler Hat does
    // not multiply that gold; the previous opposite expectation was obsolete.
    for enabled in [false, true] {
        let mut mgr = manager();
        mgr.run_ctx_mut().ascension = 10;
        let mut rng = mgr.run_ctx().rngs.rewards.clone();
        let base = rng.next_int_exclusive(42, 53) as i32 * 3 / 4;
        let expected = base;
        let before = mgr.run_ctx().gold;
        let draws = mgr.run_ctx().rngs.rewards.counter;
        mgr.treasure_relic = Some(RelicId::BowlerHat);
        mgr.phase = RunPhase::Treasure;
        if enabled {
            mgr.collect_treasure().unwrap();
        } else {
            let mut hat = Relic::new(RelicId::BowlerHat);
            hat.enabled = false;
            mgr.run_ctx_mut().relics.push(hat);
            treasure::chest_gold(mgr.run_ctx_mut(), RelicId::Strawberry);
        }
        assert_eq!(mgr.run_ctx().gold, before + expected);
        assert_eq!(mgr.run_ctx().rngs.rewards.counter, draws + 1);
    }
}

#[test]
fn combat_choice_actions_include_every_candidate_and_confirmation() {
    use crate::choice::{ChoiceSrc, PendingChoice, Resolver};
    use crate::combat::PileKind;
    for (count,confirm) in [(63,true),(64,true),(64,false),(65,false),(300,true),(300,false)] {
        let mut mgr=manager();
        let card=mgr.run_ctx().deck[0].clone();
        let mut combat=Combat::new(mgr.take_run_from_owner(),1);
        combat.cards.push(card);
        combat.choice_options=vec![0;count];
        combat.pending_choice=Some(PendingChoice {
            player:0,source:PileKind::Draw,allow_skip:confirm,
            min:0,max:count as u16,single:!confirm,
            resolver:Resolver {src:ChoiceSrc::Card(CardId::Headbutt),k:0,
                target:None,step:0,arg:0},
        });
        mgr.combat=Some(combat);
        mgr.phase=RunPhase::Combat;
        let actions=mgr.available_actions();
        assert_eq!(actions.len(),count+usize::from(confirm));
        assert_eq!(mgr.unported,None);
    }
}

#[test]
fn run_gold_gains_share_bowler_and_ectoplasm_modifiers_without_rng_draws() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::BowlerHat));
    let gold = mgr.run_ctx().gold;
    let draws = mgr.run_ctx().rngs.rewards.counter;
    mgr.apply_event_effect(EventEffect::GainGold(7)).unwrap();
    assert_eq!(mgr.run_ctx().gold, gold + 8);
    mgr.obtain_event_relic(RelicId::OldCoin).unwrap();
    assert_eq!(mgr.run_ctx().gold, gold + 8 + 375);
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::Ectoplasm));
    mgr.apply_event_effect(EventEffect::GainGold(100)).unwrap();
    assert_eq!(mgr.run_ctx().gold, gold + 8 + 375);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, draws);
}

#[test]
fn lucky_fysh_pays_only_when_a_card_enters_the_deck_and_uses_gold_modifiers() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::LuckyFysh));
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::BowlerHat));
    let gold = mgr.run_ctx().gold;
    mgr.add_card_to_deck(CardId::Anger).unwrap();
    assert_eq!(mgr.run_ctx().gold, gold + 18);
    mgr.add_card_to_deck(CardId::Injury).unwrap();
    assert_eq!(mgr.run_ctx().gold, gold + 36);
    mgr.run_ctx_mut().relics.iter_mut().find(|r| r.id == RelicId::LuckyFysh).unwrap().enabled = false;
    mgr.add_card_to_deck(CardId::Anger).unwrap();
    assert_eq!(mgr.run_ctx().gold, gold + 36);
}

#[test]
fn progress_menu_encodes_special_card_and_card_removal_rewards() {
    let mut mgr = manager();
    mgr.reward = RewardState::default();
    mgr.reward.items.push(item(RewardItemKind::SpecialCard {
        owner: crate::combat::PLAYER,
        card: Card::from_factory(CardId::StrikeIronclad, false, 1).unwrap(),
    }));
    mgr.reward.items.push(item(RewardItemKind::CardRemoval { owner: crate::combat::PLAYER }));
    assert_eq!(mgr.observation_progress_menu(),
        vec![("ADD_CARD", "", "", 0, 0), ("REMOVE_CARD", "", "", 0, 0)]);
}

#[test]
fn lava_rock_adds_two_first_act_boss_relics_once_with_normal_rng() {
    let mut mgr=manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::LavaRock));
    let mut baseline=mgr.clone();
    baseline.run_ctx_mut().relics.retain(|r|r.id!=RelicId::LavaRock);
    baseline.generate_combat_rewards(RoomType::Boss);
    let expected=[baseline.pull_player_relic(None,false),baseline.pull_player_relic(None,false)];
    mgr.generate_combat_rewards(RoomType::Boss);
    let actual:Vec<_>=mgr.reward.items.iter().filter_map(|i|
        if let RewardItemKind::Relic(id)=i.kind {Some(id)}else{None}).collect();
    assert_eq!(actual,expected);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter,baseline.run_ctx().rngs.rewards.counter);
    let relic=mgr.run_ctx().relics.iter().find(|r|r.id==RelicId::LavaRock).unwrap();
    assert!(relic.flag1);
    assert!(!relic.enabled);
    mgr.generate_combat_rewards(RoomType::Boss);
    assert!(!mgr.reward.items.iter().any(|i|matches!(i.kind,RewardItemKind::Relic(_))));
    let mut regular=manager();
    regular.run_ctx_mut().relics.push(Relic::new(RelicId::LavaRock));
    regular.generate_combat_rewards(RoomType::Monster);
    assert!(!regular.run_ctx().relics.iter().find(|r|r.id==RelicId::LavaRock).unwrap().flag1);
}

#[test]
fn whispering_hollow_potions_use_rewards_and_hug_uses_event_rng_before_damage() {
    let mut mgr = manager();
    mgr.run_ctx_mut().gold = 100;
    enter_event(&mut mgr, EventId::WhisperingHollow);
    let cost = mgr.current_event.as_ref().unwrap().ints[0];
    assert!((26..=44).contains(&cost));
    assert_eq!(mgr.current_event.as_ref().unwrap().rng.counter, 1);
    let mut expected = mgr.clone();
    let ids = [expected.roll_potion().unwrap(), expected.roll_potion().unwrap()];
    let before = mgr.run_ctx().rngs.rewards.counter;
    mgr.take_event_choice("gold").unwrap();
    assert_eq!(mgr.run_ctx().gold, 100 - cost);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, before + 4);
    assert!(mgr.reward.active.is_none());
    for (item, id) in mgr.reward.items.iter().zip(ids) {
        assert!(matches!(item.kind, RewardItemKind::Potion(p) if p == id));
    }
    let mut mgr = manager();
    mgr.run_ctx_mut().hp = 9;
    enter_event(&mut mgr, EventId::WhisperingHollow);
    let mut event_rng = mgr.current_event.as_ref().unwrap().rng.clone();
    let original = mgr.run_ctx().deck[0].clone();
    let pool = super::super::card_pools::transformations(&original, false).unwrap();
    let expected = pool[event_rng.choice_index(pool.len())];
    let draws = mgr.run_ctx().rngs.transformations.counter;
    let count = mgr.run_ctx().deck.len();
    mgr.take_event_choice("hug").unwrap();
    assert_eq!(mgr.run_ctx().hp, 9, "damage awaits selection");
    mgr.resolve_run_choice(0).unwrap();
    assert_eq!(mgr.run_ctx().deck.len(), count);
    assert_eq!(mgr.run_ctx().deck.last().unwrap().id, expected);
    assert_eq!(mgr.run_ctx().rngs.transformations.counter, draws);
    assert_eq!(mgr.phase(), RunPhase::RunOver, "transform precedes fatal damage");
}

#[test]
fn jungle_maze_rolls_fractional_variance_before_truncating_and_applies_damage_first() {
    for solo in [false, true] {
        let mut mgr = manager();
        enter_event(&mut mgr, EventId::JungleMazeAdventure);
        let event = mgr.current_event.as_ref().unwrap();
        assert_eq!(event.rng.counter, 2);
        assert!((135..165).contains(&event.ints[0]));
        assert!((35..65).contains(&event.ints[1]));
        let gold = event.ints[usize::from(!solo)];
        let before = mgr.run_ctx().gold;
        let hp = mgr.run_ctx().hp;
        mgr.take_event_choice(if solo { "solo" } else { "join" }).unwrap();
        assert_eq!(mgr.run_ctx().gold, before + gold);
        assert_eq!(mgr.run_ctx().hp, hp - if solo { 18 } else { 0 });
        assert_eq!(mgr.phase(), RunPhase::MapChoice);
    }
    let mut mgr = manager();
    enter_event(&mut mgr, EventId::JungleMazeAdventure);
    mgr.run_ctx_mut().hp = 18;
    let gold = mgr.run_ctx().gold;
    mgr.take_event_choice("solo").unwrap();
    assert_eq!(mgr.run_ctx().gold, gold);
    assert_eq!(mgr.phase(), RunPhase::RunOver);
}

#[test]
fn architect_dialogue_only_proceeds_on_last_line_and_finishes_run() {
    let mut mgr = manager();
    let mut event = EventState::new(EventId::TheArchitect, Rng::new(0));
    event.ints[0] = 2;
    event.options.push(EventOption::new("advance", "Continue", ""));
    mgr.current_event = Some(event);
    mgr.phase = RunPhase::Event;
    assert!(mgr.take_event_choice("proceed").is_err());
    mgr.take_event_choice("advance").unwrap();
    assert_eq!(mgr.phase(), RunPhase::Event);
    assert_eq!(mgr.current_event.as_ref().unwrap().options[0].option_id, "proceed");
    assert!(mgr.take_event_choice("advance").is_err());
    mgr.take_event_choice("proceed").unwrap();
    assert_eq!(mgr.phase(), RunPhase::RunOver);
}

#[test]
fn nimble_candidate_certificate_requires_the_exact_csharp_extra_and_complete_legacy_set() {
    let mut mgr = manager();
    mgr.run_ctx_mut().deck = [
        CardId::DefendIronclad, CardId::ExpectAFight, CardId::DefendIronclad, CardId::Inflame,
    ].into_iter().enumerate().map(|(i, id)|
        Card::from_factory(id, false, i as u32 + 1).unwrap()).collect();
    enter_event(&mut mgr, EventId::SelfHelpBook);
    mgr.take_event_choice("read_passage").unwrap();
    assert_eq!(mgr.verify_self_help_nimble_candidate_set(&[1,3]), Some(vec![0,2]));
    for wrong in [vec![], vec![1], vec![3,1], vec![1,1], vec![1,2,3], vec![1,3,4]] {
        assert_eq!(mgr.verify_self_help_nimble_candidate_set(&wrong), None);
    }
    let mut changed = mgr.clone();
    changed.pending_run_choice.as_mut().unwrap().options.pop();
    assert_eq!(changed.verify_self_help_nimble_candidate_set(&[1,3]), None);
    let mut changed = mgr.clone();
    changed.pending_run_choice.as_mut().unwrap().options.swap(0,1);
    assert_eq!(changed.verify_self_help_nimble_candidate_set(&[1,3]), None);
    let mut changed = mgr.clone();
    changed.pending_run_choice.as_mut().unwrap().selected.push(0);
    assert_eq!(changed.verify_self_help_nimble_candidate_set(&[1,3]), None);
    let mut changed = mgr.clone();
    changed.pending_run_choice.as_mut().unwrap().wire_source = Some("event.symbiote.select");
    assert_eq!(changed.verify_self_help_nimble_candidate_set(&[1,3]), None);
    let mut changed = mgr;
    changed.run_ctx_mut().deck[1] = Card::from_factory(CardId::ShrugItOff, false, 2).unwrap();
    assert_eq!(changed.verify_self_help_nimble_candidate_set(&[1,3]), None);
}

#[test]
fn self_help_book_rule_sources_separate_enchantment_candidate_sets() {
    for (card, option, source) in [
        (CardId::StrikeIronclad, "read_back", "event.self_help_book.sharp"),
        (CardId::DefendIronclad, "read_passage", "event.self_help_book.nimble"),
        (CardId::Inflame, "read_entire", "event.self_help_book.swift"),
    ] {
        let mut mgr = manager();
        mgr.run_ctx_mut().deck = (1..=2)
            .map(|instance| Card::from_factory(card, false, instance).unwrap()).collect();
        enter_event(&mut mgr, EventId::SelfHelpBook);
        mgr.take_event_choice(option).unwrap();
        assert_eq!(mgr.observation_selector_source(), Some(source));
        assert_eq!(mgr.pending_run_choice.as_ref().unwrap().options.len(), 2);
        assert!(mgr.observation_selector_shape().unwrap()
            .starts_with("screen-v2;source=event.self_help_book.enchant;"));
    }
}

#[test]
fn self_help_book_dispatches_all_three_types_and_auto_selects_a_single_candidate() {
    for (card, option, enchantment) in [
        (CardId::StrikeIronclad, "read_back", "Sharp"),
        (CardId::DefendIronclad, "read_passage", "Nimble"),
        (CardId::Inflame, "read_entire", "Swift"),
    ] {
        let mut mgr = manager();
        mgr.run_ctx_mut().deck = vec![Card::from_factory(card, false, 1).unwrap()];
        let rng = mgr.run_ctx().rngs.rewards.counter;
        enter_event(&mut mgr, EventId::SelfHelpBook);
        mgr.refresh_event_option_locks();
        assert_eq!(mgr.current_event.as_ref().unwrap().options.iter().filter(|o| o.enabled).count(), 1);
        mgr.take_event_choice(option).unwrap();
        assert!(mgr.pending_run_choice.is_none());
        assert_eq!(mgr.phase(), RunPhase::MapChoice);
        assert!(mgr.run_ctx().deck[0].enchantments.contains(&(EnchantmentId::from_name(enchantment).unwrap(), 2)));
        assert_eq!(mgr.run_ctx().rngs.rewards.counter, rng);
    }
}

fn pick_pael(mgr: &mut RunManager, relic: RelicId) {
    let mut event = EventState::new(EventId::Pael, Rng::new(7));
    event.options.push(super::super::events::EventOption::new("relic_1", "Relic", "Gain a Pael relic"));
    event.relics.push(relic);
    mgr.phase = RunPhase::Event;
    mgr.current_event = Some(event);
    mgr.take_event_choice("relic_1").unwrap();
}

#[test]
fn pael_source_ordered_weighted_pools_draw_three_times_and_heal_as_an_ancient() {
    for empty_with_pet in [false, true] {
        let mut mgr = manager();
        mgr.run_ctx_mut().hp = 1;
        if empty_with_pet {
            mgr.run_ctx_mut().deck.clear();
            mgr.run_ctx_mut().relics.push(Relic::new(RelicId::Byrdpip));
        }
        let event = EventState::new(EventId::Pael, Rng::new(7));
        let mut rng = event.rng.clone();
        let first = [RelicId::PaelsFlesh, RelicId::PaelsHorn, RelicId::PaelsTears];
        let second = if empty_with_pet {
            vec![RelicId::PaelsWing, RelicId::PaelsWing, RelicId::PaelsGrowth]
        } else {
            vec![RelicId::PaelsWing, RelicId::PaelsClaw, RelicId::PaelsTooth,
                RelicId::PaelsWing, RelicId::PaelsClaw, RelicId::PaelsTooth, RelicId::PaelsGrowth]
        };
        let third = if empty_with_pet { vec![RelicId::PaelsEye, RelicId::PaelsBlood] }
            else { vec![RelicId::PaelsEye, RelicId::PaelsBlood, RelicId::PaelsLegion] };
        let expected = [first[rng.choice_index(first.len())],
            second[rng.choice_index(second.len())], third[rng.choice_index(third.len())]];
        mgr.current_event = Some(event);
        mgr.initialize_pael().unwrap();
        let event = mgr.current_event.as_ref().unwrap();
        assert_eq!(event.relics.as_slice(), expected);
        assert_eq!(event.rng.counter, 3);
        assert_eq!(mgr.run_ctx().hp, 64);
    }
}

#[test]
fn starting_and_neow_relics_record_original_acquisition_floor() {
    let mut mgr=RunManager::new(1,RunOptions::default());
    assert_eq!(mgr.run_ctx().relics[0].floor_added_to_deck,1);
    let choice=mgr.neow[0];
    mgr.resolve_neow_choice(choice.option_id).unwrap();
    assert_eq!(mgr.run_ctx().relics.iter().find(|r|r.id==choice.relic)
        .expect("chosen Neow relic").floor_added_to_deck,1);
    let mut later=mgr.run_ctx().clone();
    later.total_floor=10;
    shop::add_relic(&mut later,RelicId::Strawberry).unwrap();
    assert_eq!(later.relics.iter().find(|r|r.id==RelicId::Strawberry)
        .expect("later acquired relic").floor_added_to_deck,later.total_floor);
    let replacement=shop::obtained_relic(&later,RelicId::BlackBlood);
    assert_eq!(replacement.floor_added_to_deck,later.total_floor);
}


#[test]
#[should_panic(expected="selector site must have a stable name")]
fn selector_constructor_rejects_unnamed_site() {
    manager().begin_run_card_choice("", "Choose a card", &[0,1], 1, 1, false,
        RunChoiceEffect::Upgrade, ChoiceResume::MapChoice);
}

#[test]
fn pael_pickups_apply_claw_to_all_defends_horn_twice_and_growth_at_four() {
    let mut mgr = manager();
    let eligible = mgr.run_ctx().deck.iter().filter(|c| c.has_tag(CardTag::Defend)).count();
    pick_pael(&mut mgr, RelicId::PaelsClaw);
    let enchanted: Vec<_> = mgr.run_ctx().deck.iter().filter(|c| c.enchantments.contains(&(crate::identity::EnchantmentId::Goopy,1))).collect();
    assert_eq!(enchanted.len(), eligible);
    assert!(enchanted.iter().all(|c| c.has_kw(KW_EXHAUST)));
    let mut mgr = manager();
    let before = mgr.run_ctx().deck.len();
    pick_pael(&mut mgr, RelicId::PaelsHorn);
    assert_eq!(mgr.run_ctx().deck.len(), before + 2);
    assert!(mgr.run_ctx().deck[before..].iter().all(|c| c.id == CardId::Relax));
    let mut mgr = manager();
    pick_pael(&mut mgr, RelicId::PaelsGrowth);
    mgr.resolve_run_choice(0).unwrap();
    assert!(mgr.run_ctx().deck.iter().any(|c| c.enchantments.contains(&(crate::identity::EnchantmentId::Clone,4))));
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
}

#[test]
fn pael_tooth_removes_five_upgradable_cards_into_persistent_storage() {
    let mut mgr = manager();
    let before = mgr.run_ctx().deck.len();
    pick_pael(&mut mgr, RelicId::PaelsTooth);
    let choice = mgr.pending_run_choice.as_ref().unwrap();
    assert_eq!((choice.min_choices, choice.max_choices), (5,5));
    assert!(choice.options.iter().all(|c| c.card_id != CardId::AscendersBane));
    for index in 0..5 { mgr.resolve_run_choice(index).unwrap(); }
    assert!(mgr.pending_run_choice.is_none());
    assert_eq!(mgr.run_ctx().deck.len(), before - 5);
    assert_eq!(mgr.run_ctx().paels_tooth_cards.len(), 5);
    assert_eq!(mgr.run_ctx().relics.last().unwrap().counter, 5);
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
}

#[test]
fn tablet_of_truth_escalates_costs_and_only_draws_for_random_upgrades() {
    let mut mgr = manager();
    mgr.run_ctx_mut().deck = vec![Card::from_factory(CardId::StrikeIronclad, false, 1).unwrap()];
    mgr.run_ctx_mut().max_hp = 80;
    mgr.run_ctx_mut().hp = 80;
    enter_event(&mut mgr, EventId::TabletOfTruth);
    for max_hp in [77,71,59,35] {
        mgr.take_event_choice("decipher").unwrap();
        assert_eq!(mgr.run_ctx().max_hp, max_hp);
        assert_eq!(mgr.current_event.as_ref().unwrap().rng.counter, 1);
    }
    mgr.add_card_to_deck(CardId::DefendIronclad).unwrap();
    let draws = mgr.run_ctx().rngs.niche.counter;
    mgr.take_event_choice("decipher").unwrap();
    assert_eq!((mgr.run_ctx().max_hp, mgr.run_ctx().hp), (1,1));
    assert!(mgr.run_ctx().deck.iter().all(|c| c.upgraded));
    assert_eq!(mgr.run_ctx().rngs.niche.counter, draws);
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
    let mut mgr = manager();
    mgr.run_ctx_mut().max_hp = 3;
    mgr.run_ctx_mut().hp = 3;
    enter_event(&mut mgr, EventId::TabletOfTruth);
    mgr.take_event_choice("decipher").unwrap();
    assert_eq!((mgr.run_ctx().max_hp, mgr.run_ctx().hp), (1,0));
    assert_eq!(mgr.phase(), RunPhase::RunOver);
}

#[test]
fn sapphire_seed_heals_before_upgrade_and_sown_is_a_typed_deck_choice() {
    for plant in [false, true] {
        let mut mgr = manager();
        mgr.run_ctx_mut().hp = 10;
        let rewards = mgr.run_ctx().rngs.rewards.counter;
        enter_event(&mut mgr, EventId::SapphireSeed);
        mgr.take_event_choice(if plant { "plant" } else { "eat" }).unwrap();
        assert_eq!(mgr.run_ctx().hp, if plant { 10 } else { 19 });
        let choice = mgr.pending_run_choice.as_ref().unwrap();
        assert!(choice.options.iter().all(|c| c.card_id != CardId::AscendersBane));
        let identity = choice.options[0].instance_id;
        mgr.resolve_run_choice(0).unwrap();
        let card = mgr.run_ctx().deck.iter().find(|c| c.instance_id == identity).unwrap();
        if plant { assert!(card.enchantments.contains(&(crate::identity::EnchantmentId::Sown, 1))); }
        else { assert!(card.upgraded); }
        assert_eq!(mgr.run_ctx().rngs.rewards.counter, rewards);
        assert_eq!(mgr.phase(), RunPhase::MapChoice);
    }
    let mut mgr = manager();
    mgr.run_ctx_mut().deck.clear();
    mgr.run_ctx_mut().hp = 10;
    enter_event(&mut mgr, EventId::SapphireSeed);
    mgr.take_event_choice("eat").unwrap();
    assert_eq!(mgr.run_ctx().hp, 19);
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
}

#[test]
fn self_help_book_keeps_type_restrictions_and_no_options_fallback() {
    let mut mgr = manager();
    enter_event(&mut mgr, EventId::SelfHelpBook);
    mgr.refresh_event_option_locks();
    assert!(mgr.current_event.as_ref().unwrap().options[0].enabled);
    assert!(!mgr.current_event.as_ref().unwrap().options[2].enabled);
    assert!(mgr.available_actions().iter().all(|a| a.option_id != Some("read_entire")));
    assert!(mgr.available_actions().iter().all(|a| a.enabled));
    mgr.take_event_choice("read_back").unwrap();
    let choice = mgr.pending_run_choice.as_ref().unwrap();
    assert_eq!((choice.min_choices, choice.max_choices), (1,1));
    assert!(choice.options.iter().all(|c| mgr.run_ctx().deck.iter()
        .find(|d| d.id == c.card_id).unwrap().card_type == CardType::Attack));
    mgr.resolve_run_choice(0).unwrap();
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
    assert!(mgr.run_ctx().deck.iter().any(|c| c.enchantments.iter().any(|&(name,n)| name == EnchantmentId::Sharp && n == 2)));
    let mut mgr = manager();
    mgr.run_ctx_mut().deck.clear();
    enter_event(&mut mgr, EventId::SelfHelpBook);
    mgr.refresh_event_option_locks();
    assert_eq!(mgr.current_event.as_ref().unwrap().options[0].option_id, "skip_book");
    mgr.take_event_choice("skip_book").unwrap();
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
}

#[test]
fn egg_excludes_event_pet_offers_and_single_enchant_upgrade_selectors_return_immediately() {
    let mut mgr = manager();
    mgr.add_card_to_deck(CardId::ByrdonisEgg).unwrap();
    assert!(mgr.has_event_pet());
    assert!(!mgr.event_is_allowed(EventId::ByrdonisNest));
    for plant in [false, true] {
        let mut mgr = manager();
        mgr.run_ctx_mut().deck.truncate(1);
        enter_event(&mut mgr, EventId::SapphireSeed);
        mgr.take_event_choice(if plant { "plant" } else { "eat" }).unwrap();
        assert!(mgr.pending_run_choice.is_none());
        assert_eq!(mgr.phase(), RunPhase::MapChoice);
        if plant { assert!(mgr.run_ctx().deck[0].enchantments.contains(&(crate::identity::EnchantmentId::Sown, 1))); }
        else { assert!(mgr.run_ctx().deck[0].upgraded); }
    }
    let mut mgr = manager();
    mgr.run_ctx_mut().deck.truncate(1);
    pick_pael(&mut mgr, RelicId::PaelsGrowth);
    assert!(mgr.pending_run_choice.is_none());
    assert!(mgr.run_ctx().deck[0].enchantments.contains(&(crate::identity::EnchantmentId::Clone, 4)));
}

#[test]
fn brain_leech_rip_and_share_use_no_upgrade_draws() {
    for rip in [false, true] {
        let mut mgr = manager();
        let hp = mgr.run_ctx().hp;
        let draws = mgr.run_ctx().rngs.rewards.counter;
        enter_event(&mut mgr, EventId::BrainLeech);
        mgr.take_event_choice(if rip { "rip" } else { "share_knowledge" }).unwrap();
        assert_eq!(mgr.run_ctx().rngs.rewards.counter, draws + if rip { 6 } else { 10 });
        assert_eq!(mgr.run_ctx().hp, hp - if rip { 5 } else { 0 });
        if rip {
            let RewardItemKind::Card { cards, .. } = &mgr.reward.items[0].kind else { panic!() };
            assert_eq!(cards.len(), 3);
            assert!(cards.iter().all(|c| !c.upgraded && c.rarity != CardRarity::Common));
        } else {
            let choice = mgr.pending_run_choice.as_ref().unwrap();
            assert_eq!(choice.reward_cards.len(), 5);
            assert_eq!((choice.min_choices, choice.max_choices, choice.allow_skip), (1,1,false));
            assert!(choice.reward_cards.iter().all(|c| !c.upgraded));
            let picks: Vec<_> = mgr.available_actions().into_iter()
                .filter(|a| a.kind == RunActionKind::PickCard).collect();
            assert_eq!(picks.len(), 5);
            let count = mgr.run_ctx().deck.len();
            mgr.take_action(picks[0]).unwrap();
            assert_eq!(mgr.run_ctx().deck.len(), count + 1);
            assert_eq!(mgr.phase(), RunPhase::MapChoice);
        }
    }
}

#[test]
fn luminous_choir_retains_event_price_and_obtains_relic_after_spending() {
    let mut mgr = manager();
    mgr.run_ctx_mut().gold = 149;
    for bag in mgr.relic_bags.player[0].iter_mut().chain(mgr.relic_bags.shared.iter_mut()) {
        *bag = vec![RelicId::Strawberry];
    }
    enter_event(&mut mgr, EventId::LuminousChoir);
    let event = mgr.current_event.as_ref().unwrap();
    let cost = event.ints[0];
    assert!((100..=149).contains(&cost));
    assert_eq!(event.rng.counter, 1);
    mgr.take_event_choice("tribute").unwrap();
    assert_eq!(mgr.run_ctx().gold, 149 - cost);
    assert!(mgr.run_ctx().relics.iter().any(|r| r.id == RelicId::Strawberry));
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
}

#[test]
fn luminous_choir_confirms_selection_and_autoselects_small_decks() {
    for size in 0..=3 {
        let mut mgr = manager();
        mgr.run_ctx_mut().deck = (0..size).map(|i|
            Card::from_factory(CardId::StrikeIronclad, false, i+1).unwrap()).collect();
        enter_event(&mut mgr, EventId::LuminousChoir);
        mgr.take_event_choice("reach").unwrap();
        if size > 2 {
            mgr.resolve_run_choice(0).unwrap();
            assert_eq!(mgr.phase(), RunPhase::CardReward);
            mgr.resolve_run_choice(1).unwrap();
        }
        assert!(mgr.pending_run_choice.is_none());
        assert_eq!(mgr.phase(), RunPhase::MapChoice);
        assert_eq!(mgr.run_ctx().deck.len(), size.saturating_sub(2) as usize + 1);
        assert!(mgr.run_ctx().deck.iter().any(|c| c.id.name() == "SPORE_MIND"));
    }
}

#[test]
fn luminous_choir_eligibility_uses_only_owner_relic_grab_bag() {
    let mut mgr = manager();
    mgr.run_ctx_mut().gold = 149;
    for bag in &mut mgr.relic_bags.player[0] { bag.clear(); }
    for bag in &mut mgr.relic_bags.shared { *bag = vec![RelicId::Strawberry]; }
    assert!(!mgr.event_is_allowed(EventId::LuminousChoir));
    mgr.relic_bags.player[0][0].push(RelicId::Strawberry);
    assert!(mgr.event_is_allowed(EventId::LuminousChoir));
    mgr.run_ctx_mut().total_floor = 41;
    for bag in &mut mgr.relic_bags.player[0] { bag.clear(); }
    mgr.relic_bags.player[0][0].push(RelicId::OldCoin);
    assert!(!mgr.event_is_allowed(EventId::LuminousChoir));
    mgr.prune_player_relic_bag();
    assert!(mgr.relic_bags.player[0].iter().all(Vec::is_empty));
    mgr.relic_bags.player[0][0].push(RelicId::Strawberry);
    assert!(mgr.event_is_allowed(EventId::LuminousChoir));
}

#[test]
#[ignore = "set STS2_SIM_CSHARP_EVENTS to the audited Models/Events directory"]
fn unported_event_random_play_census() {
    let root = std::env::var("STS2_SIM_CSHARP_EVENTS").unwrap();
    let mut counts = std::collections::BTreeMap::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("cs") { continue; }
        let name = path.file_stem().unwrap().to_str().unwrap();
        let mut id = String::new();
        for (i,c) in name.chars().enumerate() {
            if c.is_uppercase() && i > 0 { id.push('_'); }
            id.extend(c.to_uppercase());
        }
        // These are dispatched directly by RunManager, not events::initialize.
        if matches!(id.as_str(), "NEOW" | "OROBAS") { continue; }
        let id: &'static str = Box::leak(id.into_boxed_str());
        if super::super::events::initialize(crate::identity::EventId::from_name(id).expect("catalog event identity"), 0, 149, 24, 80, 80).is_none() {
            counts.insert(id, 0usize);
        }
    }
    // Same seeds, xorshift64 policy, horizon and legal-mask indexing as
    // tests/lane_memory.rs::lane_measurements; omit only allocation profiling.
    let runs: u64 = std::env::var("STS2_SIM_EVENT_CENSUS_RUNS")
        .map(|s| s.parse().unwrap()).unwrap_or(16);
    println!("CENSUS,seeds=4100000..{},policy=lane_memory_xorshift64,horizon=20000", 4100000 + runs);
    for seed in 4100000..4100000 + runs {
        let mut run = RunManager::new(seed, RunOptions::default());
        let mut random = seed;
        let mut previous_event = None;
        for _ in 0..20000 {
            let event = run.current_event_id();
            if run.phase() == RunPhase::Event && event != previous_event
                && let Some(count) = event.and_then(|id| counts.get_mut(id))
            { *count += 1; }
            previous_event = if run.phase() == RunPhase::Event { event } else { None };
            let actions = run.available_actions();
            if actions.is_empty() { break; }
            random ^= random << 13; random ^= random >> 7; random ^= random << 17;
            if run.take_action(actions[random as usize % actions.len()]).is_err() { break; }
        }
    }
    for (event, count) in counts {
        println!("UNPORTED_EVENT,{event},{count}");
    }
}

#[test]
fn driftwood_rerolls_each_reward_once_with_retained_boss_options() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::Driftwood));
    let mut cards = ArrayVec::new();
    let mut chosen = ArrayVec::new();
    for _ in 0..3 {
        let rarity = mgr.roll_card_rarity(RoomType::Boss);
        let card = mgr.generate_reward_card(rarity, &chosen).unwrap();
        chosen.push(card.id);
        cards.push(card);
    }
    let old_ids: Vec<_> = cards.iter().map(|c| c.instance_id).collect();
    mgr.reward.items.push(RewardItem {
        kind: RewardItemKind::Card { cards, closed: false,
            reroll: Some(CardRewardRerollOptions::Encounter(RoomType::Boss)), progress: Default::default() },
        consumed: false, skippable: true, skip_completes: false,
        add_injury_after: false, neows_bones_final: false,
    });
    mgr.reward.active = Some(0);
    mgr.phase = RunPhase::CardReward;
    let before = mgr.run_ctx().rngs.rewards.counter;
    assert_eq!(mgr.observation_card_reward_counters(),vec![(0,0)]);
    assert!(mgr.available_actions().iter().any(|a| a.kind == RunActionKind::RerollCardReward));
    mgr.take_action(RunAction::new(RunActionKind::RerollCardReward)).unwrap();
    // C# CardFactory: rarity NextFloat, candidate NextItem, upgrade NextFloat,
    // in the owner's Rewards stream, once for each of three cards.
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, before + 9);
    assert_eq!(mgr.observation_card_reward_counters(),vec![(0,1)]);
    assert!(mgr.observation_progress_menu().iter().any(|r| r.0=="CARD" && r.3==0 && r.4==1),
        "the semantic snapshot must retain the accepted reroll count");
    let RewardItemKind::Card { cards, .. } = &mgr.reward.items[0].kind else { panic!() };
    assert!(cards.iter().all(|c| c.rarity == CardRarity::Rare));
    assert!(cards.iter().all(|c| !old_ids.contains(&c.instance_id)));
    assert_eq!(mgr.reward.active, Some(0));
    assert!(!mgr.reward.items[0].consumed);
    assert!(!mgr.available_actions().iter().any(|a| a.kind == RunActionKind::RerollCardReward));
    assert!(mgr.take_action(RunAction::new(RunActionKind::RerollCardReward)).is_err());
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, before + 9);
    assert_eq!(mgr.observation_card_reward_counters(),vec![(0,1)],"failed reroll is not counted");
    mgr.pick_card(0).unwrap();
    assert_eq!(mgr.observation_card_reward_counters(),vec![(1,1)],"successful pick is counted once");
}

#[test]
fn dream_catcher_reroll_preserves_encounter_rarity_offset() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::DreamCatcher));
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::Driftwood));
    mgr.generate_rest_heal_rewards();
    mgr.card_rarity_offset = 0.4;
    let before = mgr.run_ctx().rngs.rewards.counter;
    let mut expected = manager();
    expected.card_rarity_offset = mgr.card_rarity_offset;
    expected.run_ctx_mut().rngs.rewards = mgr.run_ctx().rngs.rewards.clone();
    let mut chosen = ArrayVec::new();
    let mut ids = Vec::new();
    for _ in 0..3 {
        let rarity = expected.roll_card_rarity(RoomType::Monster);
        let card = expected.generate_reward_card(rarity, &chosen).unwrap();
        chosen.push(card.id);
        ids.push(card.id);
    }
    mgr.reroll_card_reward().unwrap();
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, before + 9);
    assert_eq!(mgr.card_rarity_offset, expected.card_rarity_offset);
    let RewardItemKind::Card { cards, .. } = &mgr.reward.items[mgr.reward.active.unwrap()].kind else { panic!() };
    assert_eq!(cards.iter().map(|c| c.id).collect::<Vec<_>>(), ids);
}

#[test]
fn event_damage_with_death_prevention_continues_before_terminal_guards() {
    for event in [crate::identity::EventId::RoomFullOfCheese, crate::identity::EventId::HungryForMushrooms] {
        for fairy in [false, true] {
            let mut mgr = manager();
            mgr.run_ctx_mut().hp = 14;
            if fairy {
                mgr.run_ctx_mut().potions = vec![Some(PotionId::Fairyinabottle)];
            } else {
                mgr.run_ctx_mut().relics.push(Relic::new(RelicId::LizardTail));
            }
            enter_event(&mut mgr, event);
            mgr.take_event_choice(
                if event == EventId::RoomFullOfCheese { "search" } else { "fragrant" }
            ).unwrap();
            assert!(mgr.run_ctx().hp>0);
            assert!(mgr.unported.is_none());
            assert_ne!(mgr.phase(), RunPhase::RunOver);
            if fairy {assert!(!mgr.run_ctx().potions.contains(&Some(PotionId::Fairyinabottle)));}
            else {assert!(mgr.run_ctx().relics.iter().find(|r|r.id==RelicId::LizardTail).unwrap().flag1);}
        }
    }
    let mut mgr = manager();
    mgr.run_ctx_mut().deck.clear();
    mgr.run_ctx_mut().hp = 16;
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::LizardTail));
    mgr.neow[0].relic = RelicId::PrecariousShears;
    mgr.resolve_neow_choice(mgr.neow[0].option_id).unwrap();
    assert!(mgr.run_ctx().hp>0);
    assert_ne!(mgr.phase(), RunPhase::RunOver);
}

#[test]
fn spent_or_disabled_lizard_tail_does_not_block_event_damage() {
    for spent in [false, true] {
        let mut mgr = manager();
        let mut tail = Relic::new(RelicId::LizardTail);
        tail.flag1 = spent;
        tail.enabled = spent; // disabled+unused, or enabled+spent
        mgr.run_ctx_mut().relics.push(tail);
        let hp = mgr.run_ctx().hp;
        mgr.apply_event_effect(EventEffect::LoseHp(6)).unwrap();
        assert_eq!(mgr.run_ctx().hp, hp - 6);
        assert!(mgr.unported.is_none());
    }
}

#[test]
fn nonlethal_event_damage_keeps_revivers_and_accounts_for_tungsten_rod() {
    for fairy in [false, true] {
        let mut mgr = manager();
        mgr.run_ctx_mut().hp = 20;
        if fairy {
            mgr.run_ctx_mut().potions = vec![Some(PotionId::Fairyinabottle)];
        } else {
            mgr.run_ctx_mut().relics.push(Relic::new(RelicId::LizardTail));
        }
        mgr.apply_event_effect(EventEffect::LoseHp(14)).unwrap();
        assert_eq!(mgr.run_ctx().hp, 6);
        assert!(mgr.unported.is_none());
        if fairy {
            assert_eq!(mgr.run_ctx().potions[0], Some(PotionId::Fairyinabottle));
        } else {
            assert!(!mgr.run_ctx().relics.last().unwrap().flag1);
        }
    }
    let mut mgr = manager();
    mgr.run_ctx_mut().hp = 14;
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::LizardTail));
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::TungstenRod));
    mgr.apply_event_effect(EventEffect::LoseHp(14)).unwrap();
    assert_eq!(mgr.run_ctx().hp, 1);
    assert!(mgr.unported.is_none());
}

#[test]
fn driftwood_does_not_offer_reroll_on_hefty_tablet() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::Driftwood));
    let item = mgr.make_uniform_card_reward(
        3, CardPool::Ironclad, CardRarity::Rare, true, true).unwrap();
    mgr.reward.items.push(item);
    mgr.reward.active = Some(0);
    mgr.phase = RunPhase::CardReward;
    assert!(!mgr.available_actions().iter().any(|a| a.kind == RunActionKind::RerollCardReward));
}

#[test]
fn driftwood_glass_eye_retains_uniform_pool_and_no_upgrade_roll() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::Driftwood));
    pick_orobas(&mut mgr, RelicId::GlassEye);
    let before = mgr.run_ctx().rngs.rewards.counter;
    let offset = mgr.card_rarity_offset;
    mgr.reroll_card_reward().unwrap();
    // GlassEye: uniform Common pool + NoUpgradeRoll => just three NextItem calls.
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, before + 3);
    assert_eq!(mgr.card_rarity_offset, offset);
    let active = mgr.reward.active.unwrap();
    let RewardItemKind::Card { cards, .. } = &mgr.reward.items[active].kind else { panic!() };
    assert_eq!(cards.len(), 3);
    assert!(cards.iter().all(|c| c.rarity == CardRarity::Common && !c.upgraded));
    mgr.pick_card(0).unwrap();
    assert!(mgr.available_actions().iter().any(|a| a.kind == RunActionKind::RerollCardReward));
}

fn pick_orobas(mgr: &mut RunManager, relic: RelicId) {
    let mut event = EventState::new(EventId::Orobas, Rng::new(7));
    event.options.push(super::super::events::EventOption::new("relic_1", "Relic", "Gain a relic"));
    event.relics.push(relic);
    event.ints[0] = 4; // C# ModelDb.AllCharacters: Defect
    mgr.current_event = Some(event);
    mgr.phase = RunPhase::Event;
    mgr.take_event_choice("relic_1").unwrap();
}

#[test]
fn dingy_rug_glass_eye_and_retained_rerolls_union_colorless_but_grids_do_not() {
    let colorless = super::super::card_pools::cards(CardPool::Colorless, false).unwrap();
    let mut saw_colorless = [false; 2];
    for owner in 0..4 {
        for seed in 1..=20 {
            let mut mgr = manager();
            mgr.run_ctx_mut().relics.clear();
            mgr.run_ctx_mut().rngs.rewards = Rng::new(seed);
            let mut baseline = manager();
            baseline.run_ctx_mut().relics.clear();
            baseline.run_ctx_mut().rngs.rewards = Rng::new(seed);
            let mut rug = Relic::new(RelicId::DingyRug); rug.owner = owner;
            mgr.run_ctx_mut().relics.push(rug);
            mgr.run_ctx_mut().relics.push(Relic::new(RelicId::Driftwood));
            baseline.run_ctx_mut().relics.push(Relic::new(RelicId::Driftwood));
            pick_orobas(&mut mgr, RelicId::GlassEye);
            pick_orobas(&mut baseline, RelicId::GlassEye);
            // GlassEye emits Common, Common, Uncommon, Uncommon, Rare.
            // Reroll an Uncommon reward: Colorless has no Common candidates.
            mgr.reward.active = Some(2);
            baseline.reward.active = Some(2);
            for stage in 0..2 {
                assert!(mgr.unported.is_none(), "{:?}", mgr.unported);
                if owner != 0 {
                    assert_eq!(format!("{:?}", mgr.reward), format!("{:?}", baseline.reward));
                }
                for (index, item) in mgr.reward.items.iter().enumerate() {
                    if let RewardItemKind::Card { cards, .. } = &item.kind {
                        if owner == 0 && (stage == 0 || index == 2) {
                            saw_colorless[stage] |= cards.iter().any(|c| colorless.contains(&c.id));
                        }
                    }
                }
                if stage == 0 {
                    mgr.take_action(RunAction::new(RunActionKind::RerollCardReward)).unwrap();
                    baseline.take_action(RunAction::new(RunActionKind::RerollCardReward)).unwrap();
                }
            }
            // Non-reward uniform creation (SeaGlass/Cheese/Tablet) is invariant.
            mgr.run_ctx_mut().rngs.rewards = Rng::new(seed);
            baseline.run_ctx_mut().rngs.rewards = Rng::new(seed);
            let a = mgr.generate_uniform_cards(8, CardPool::Ironclad, CardRarity::Uncommon, false, false).unwrap();
            let b = baseline.generate_uniform_cards(8, CardPool::Ironclad, CardRarity::Uncommon, false, false).unwrap();
            assert_eq!(a.iter().map(|c| c.id).collect::<Vec<_>>(), b.iter().map(|c| c.id).collect::<Vec<_>>());
        }
    }
    assert!(saw_colorless.into_iter().all(|seen| seen));
}

#[test]
fn cheese_grid_selects_two_distinct_common_cards_and_search_obtains_cheese() {
    let mut mgr = manager();
    enter_event(&mut mgr, EventId::RoomFullOfCheese);
    let before = mgr.run_ctx().deck.len();
    let next_id = mgr.run_ctx().next_card_id;
    mgr.take_event_choice("gorge").unwrap();
    assert_eq!(mgr.run_ctx().next_card_id, next_id + 8);
    let choice = mgr.pending_run_choice.as_ref().unwrap();
    assert_eq!((choice.min_choices, choice.max_choices), (2, 2));
    assert_eq!(choice.options.len(), 8);
    assert!(choice.reward_cards.iter().all(|c| c.rarity == CardRarity::Common));
    let ids: std::collections::HashSet<_> = choice.reward_cards.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), 8);
    let expected_ids = [choice.options[1].instance_id, choice.options[0].instance_id];
    mgr.resolve_run_choice(1).unwrap();
    assert_eq!(mgr.run_ctx().deck.len(), before);
    mgr.resolve_run_choice(1).unwrap(); // selected cards can be deselected
    assert!(mgr.pending_run_choice.as_ref().unwrap().selected.is_empty());
    mgr.resolve_run_choice(1).unwrap();
    mgr.resolve_run_choice(0).unwrap();
    assert!(mgr.pending_run_choice.is_none());
    assert!(mgr.confirm_pending_run_choice().is_err());
    assert_eq!(mgr.run_ctx().deck.len(), before + 2);
    assert_eq!(mgr.run_ctx().deck[before..].iter().map(|c| c.instance_id).collect::<Vec<_>>(), expected_ids);
    assert_eq!(mgr.phase, RunPhase::MapChoice);
    enter_event(&mut mgr, EventId::RoomFullOfCheese);
    let hp = mgr.run_ctx().hp;
    mgr.take_event_choice("search").unwrap();
    assert_eq!(mgr.run_ctx().hp, hp - 14);
    assert!(mgr.run_ctx().relics.iter().any(|r| r.id == RelicId::ChosenCheese));
}

#[test]
fn map_position_does_not_publish_the_unvisited_later_act_routing_cursor() {
    // Original RunManager.EnterAct enters StartingMapPoint only for act zero;
    // later acts enter MapRoom after SetActInternal clears visited coordinates.
    let mut mgr=manager();
    assert!(mgr.run_ctx().coord.is_some());
    assert_eq!(mgr.observation_map_position(),mgr.run_ctx().coord);
    mgr.transition_next_act().unwrap();
    assert_eq!(mgr.run_ctx().coord,None);
    assert_eq!(mgr.observation_map_position(),None);
    let action=*mgr.available_actions().iter().find(|a|a.kind==RunActionKind::Move).unwrap();
    let coord=action.coord.unwrap();
    mgr.take_action(action).unwrap();
    assert_eq!(mgr.observation_map_position(),Some((coord.col,coord.row)));
}

#[test]
fn cheese_grid_applies_silver_crucible_once_to_all_eight_reward_options() {
    let mut mgr = manager();
    let mut crucible = Relic::new(RelicId::SilverCrucible);
    crucible.counter = 2;
    mgr.run_ctx_mut().relics.push(crucible);
    enter_event(&mut mgr, EventId::RoomFullOfCheese);
    mgr.take_event_choice("gorge").unwrap();
    let choice = mgr.pending_run_choice.as_ref().unwrap();
    assert_eq!(choice.reward_cards.len(), 8);
    assert!(choice.reward_cards.iter().all(|card| card.upgraded));
    assert_eq!(
        mgr.run_ctx().relics.iter().find(|relic| relic.id == RelicId::SilverCrucible).unwrap().counter,
        3,
    );
}

#[test]
fn orobas_option_generation_consumes_source_draws_and_locks_unavailable_pool_three() {
    let mut mgr = manager();
    mgr.run_ctx_mut().hp = 1;
    mgr.current_event = Some(EventState::new(EventId::Orobas, Rng::new(7)));
    mgr.initialize_orobas().unwrap();
    assert_eq!(mgr.run_ctx().hp, 64); // 1 + floor((80 - 1) * 0.8)
    let event = mgr.current_event.as_ref().unwrap();
    assert_eq!(event.rng.counter, 5);
    assert_eq!(event.options.len(), 3);
    assert!(event.options.iter().all(|o| o.enabled));
    mgr.run_ctx_mut().deck.clear();
    mgr.run_ctx_mut().relics.retain(|r| RELIC_INFO[r.id.index()].rarity != RelicRarity::Starter);
    mgr.current_event = Some(EventState::new(EventId::Orobas, Rng::new(7)));
    mgr.initialize_orobas().unwrap();
    let event = mgr.current_event.as_ref().unwrap();
    assert!(!event.options[2].enabled);
    assert_eq!(event.relics.len(), 2);
}

#[test]
fn orobas_glass_eye_and_sea_glass_keep_their_distinct_reward_forms() {
    let mut mgr = manager();
    let before = mgr.run_ctx().deck.len();
    pick_orobas(&mut mgr, RelicId::GlassEye);
    assert_eq!(mgr.reward.items.len(), 5);
    for rarity in [CardRarity::Common, CardRarity::Common, CardRarity::Uncommon,
        CardRarity::Uncommon, CardRarity::Rare]
    {
        let RewardItemKind::Card { cards, .. } = &mgr.reward.items[mgr.reward.active.unwrap()].kind else {
            panic!("Glass Eye card reward");
        };
        assert!(cards.iter().all(|c| c.rarity == rarity));
        mgr.pick_card(0).unwrap();
    }
    assert_eq!(mgr.run_ctx().deck.len(), before + 5);
    let mut mgr = manager();
    pick_orobas(&mut mgr, RelicId::SeaGlass);
    let choice = mgr.pending_run_choice.as_ref().unwrap();
    assert_eq!((choice.min_choices, choice.max_choices), (0, 15));
    assert_eq!(choice.reward_cards.len(), 15);
    let pool = super::super::card_pools::cards(CardPool::Defect, false).unwrap();
    assert!(choice.reward_cards.iter().all(|c| pool.contains(&c.id)));
    mgr.confirm_pending_run_choice().unwrap();
    assert_eq!(mgr.phase, RunPhase::MapChoice);
}

#[test]
fn orobas_coffer_castle_and_starter_refinements_apply_their_pickup_effects() {
    let mut mgr = manager();
    let slots = mgr.run_ctx().max_potion_slots as usize;
    let counter = mgr.run_ctx().rngs.combat_potion_generation.counter;
    pick_orobas(&mut mgr, RelicId::AlchemicalCoffer);
    assert_eq!(mgr.run_ctx().max_potion_slots as usize, slots + 4);
    assert!(mgr.run_ctx().potions[slots..].iter().all(Option::is_some));
    assert_eq!(mgr.run_ctx().rngs.combat_potion_generation.counter, counter + 8);
    let mut mgr = manager();
    pick_orobas(&mut mgr, RelicId::SandCastle);
    assert_eq!(mgr.run_ctx().deck.iter().filter(|c| c.upgraded).count(), 6);
    let mut mgr = manager();
    pick_orobas(&mut mgr, RelicId::TouchOfOrobas);
    assert!(mgr.run_ctx().relics.iter().any(|r| r.id == RelicId::BlackBlood));
    assert!(!mgr.run_ctx().relics.iter().any(|r| r.id == RelicId::BurningBlood));
    let mut mgr = manager();
    let index = mgr.run_ctx().deck.iter().position(|c| c.id == CardId::Bash).unwrap();
    mgr.run_ctx_mut().upgrade_deck_card(index);
    let original_floor = mgr.run_ctx().deck[index].floor_added_to_deck;
    pick_orobas(&mut mgr, RelicId::ArchaicTooth);
    assert!(!mgr.run_ctx().deck.iter().any(|c| c.id == CardId::Bash));
    assert!(mgr.run_ctx().deck.last().unwrap().upgraded);
    assert_eq!(mgr.run_ctx().deck.last().unwrap().id, CardId::from_name("BREAK").unwrap());
    let saved = mgr.run_ctx().relics.iter().find(|r|r.id == RelicId::ArchaicTooth)
        .unwrap().saved_cards.as_ref().unwrap();
    assert_eq!(saved[0].id, CardId::Bash);
    assert!(saved[0].upgraded && saved[1].upgraded);
    assert_eq!(saved[0].floor_added_to_deck, original_floor);
    assert_eq!(saved[1].floor_added_to_deck, None);
    let mut cloned = mgr.run_ctx().relics.iter().find(|r|r.id == RelicId::ArchaicTooth).unwrap().clone();
    cloned.saved_cards.as_mut().unwrap()[0].upgraded = false;
    assert!(saved[0].upgraded);
}

#[test]
fn bones_curse_waits_for_both_pickups_when_the_second_relic_is_taken_first() {
    let mut mgr = manager();
    mgr.reward.items.push(RewardItem { skippable: false,
        ..item(RewardItemKind::Relic(RelicId::Kaleidoscope)) });
    mgr.reward.items.push(RewardItem { skippable: false, neows_bones_final: true,
        ..item(RewardItemKind::Relic(RelicId::GoldenPearl)) });
    let before = mgr.run_ctx().deck.len();
    mgr.advance_rewards();
    assert_eq!(mgr.skip_reward_set(), Err("rewards cannot be skipped"));
    mgr.pick_relic_reward(Some(1)).unwrap();
    assert_eq!(mgr.run_ctx().deck.len(), before);
    mgr.pick_relic_reward(None).unwrap();
    assert_eq!(mgr.run_ctx().deck.len(), before);
    mgr.skip_card_reward().unwrap();
    mgr.open_reward(1).unwrap();
    mgr.skip_card_reward().unwrap();
    mgr.skip_reward_set().unwrap();
    assert_eq!(mgr.run_ctx().deck.len(), before + 1);
    assert_eq!(mgr.run_ctx().deck.last().unwrap().card_type, CardType::Curse);
    assert_eq!(mgr.phase, RunPhase::MapChoice);
}

#[test]
fn failed_shop_relic_owner_validation_does_not_spend_gold() {
    let mut mgr = manager();
    mgr.run_ctx_mut().gold = 9999;
    mgr.enter_shop(MapPointType::Shop).unwrap();
    let before = mgr.run_ctx().gold;
    mgr.relic_bags.player.clear();
    assert_eq!(mgr.buy_shop_relic(0), Err("relic bag: missing owner"));
    assert_eq!(mgr.run_ctx().gold, before);
}

#[test]
fn kaleidoscope_offers_two_separate_cross_character_card_rewards() {
    let mut mgr = manager();
    mgr.apply_neow_pickup(RelicId::Kaleidoscope).unwrap();
    assert_eq!(mgr.phase, RunPhase::CardReward);
    assert_eq!(mgr.reward.items.len(), 2);
    let own_pool = super::super::card_pools::cards(CardPool::Ironclad, false).unwrap();
    for item in &mgr.reward.items {
        let RewardItemKind::Card { cards, .. } = &item.kind else { panic!("card reward"); };
        assert_eq!(cards.len(), 3);
        assert!(cards.iter().all(|c| !own_pool.contains(&c.id)));
    }
    mgr.skip_card_reward().unwrap();
    assert_eq!(mgr.reward.active, None);
    mgr.open_reward(1).unwrap();
    mgr.skip_card_reward().unwrap();
    mgr.skip_reward_set().unwrap();
    assert_eq!(mgr.phase, RunPhase::MapChoice);
}

#[test]
fn bones_suspends_parent_rewards_until_nested_pickup_finishes_then_adds_curse() {
    let mut mgr = manager();
    mgr.reward.items.push(RewardItem {
        skippable: false, neows_bones_final: false,
        ..item(RewardItemKind::Relic(RelicId::Kaleidoscope))
    });
    mgr.reward.items.push(RewardItem {
        skippable: false, neows_bones_final: true,
        ..item(RewardItemKind::Relic(RelicId::GoldenPearl))
    });
    let before = mgr.run_ctx().deck.len();
    mgr.advance_rewards();
    assert!(!mgr.available_actions().iter().any(|a| a.kind == RunActionKind::SkipRewards));
    mgr.pick_relic_reward(None).unwrap();
    assert_eq!(mgr.reward_parents.len(), 1);
    mgr.skip_card_reward().unwrap();
    mgr.open_reward(1).unwrap();
    mgr.skip_card_reward().unwrap();
    mgr.skip_reward_set().unwrap();
    assert_eq!(mgr.reward_parents.len(), 0);
    assert_eq!(mgr.run_ctx().deck.len(), before);
    mgr.pick_relic_reward(None).unwrap();
    assert_eq!(mgr.phase, RunPhase::MapChoice);
    assert_eq!(mgr.run_ctx().deck.len(), before + 1);
    assert_eq!(mgr.run_ctx().deck.last().unwrap().card_type, CardType::Curse);
}

#[test]
fn dense_vegetation_enters_csharp_wriggler_encounter_and_gives_monster_rewards() {
    let mut mgr = manager();
    enter_event(&mut mgr, EventId::DenseVegetation);
    mgr.take_event_choice("rest").unwrap();
    mgr.take_event_choice("fight").unwrap();
    assert_eq!(mgr.phase, RunPhase::Combat);
    let c = mgr.combat.as_mut().unwrap();
    assert_eq!(c.alive_enemies().len(), 4);
    for e in c.alive_enemies() {
        // C# Wriggler.ToughEnemies HP, not r7's unscaled event wrapper.
        assert_eq!(c.cr(e).ai.as_ref().unwrap().kind, MonsterKind::Wriggler);
        assert!((18..=22).contains(&c.cr(e).max_hp));
        c.kill_creature(e);
    }
    c.check_combat_end();
    mgr.after_combat_action().unwrap();
    assert_eq!(mgr.phase, RunPhase::CardReward);
    assert!(mgr.reward.items.iter().any(|item| matches!(item.kind, RewardItemKind::Card { .. })));
}

#[test]
fn obtained_relic_leaves_only_owners_and_shared_bags() {
    let mut mgr = manager();
    let bag = std::array::from_fn(|_| vec![RelicId::Strawberry]);
    mgr.relic_bags.player = vec![bag.clone(), bag.clone()];
    mgr.relic_bags.shared = bag;
    mgr.relic_bags.remove_obtained(1, RelicId::Strawberry).unwrap();
    assert!(mgr.relic_bags.player[0].iter().all(|b| b.contains(&RelicId::Strawberry)));
    assert!(mgr.relic_bags.player[1].iter().all(Vec::is_empty));
    assert!(mgr.relic_bags.shared.iter().all(Vec::is_empty));
    assert_eq!(mgr.relic_bags.remove_obtained(2, RelicId::Strawberry),
        Err("relic bag: missing owner"));
}

#[test]
fn adding_a_deck_card_does_not_read_the_reference_template() {
    let mut mgr = manager();
    mgr.add_card_to_deck(CardId::Guilty).unwrap();
    assert!(mgr.run_ctx_mut().ref_cache_miss(CardId::Guilty, false));
}

#[test]
fn fat_escape_gold_proportion_follows_theft_history() {
    let mut mgr = manager();
    let mut c = Combat::new(mgr.take_run_from_owner(), 17);
    let (fat, ai) = c.create_monster(MonsterKind::FatGremlin, 0);
    let id = c.add_enemy_in_slot(fat, ai, Some("fat"));
    assert_eq!(c.gold_proportion(), 1.0);
    c.kill_creature(id);
    assert_eq!(c.gold_proportion(), 1.0);
    let (fat, ai) = c.create_monster(MonsterKind::FatGremlin, 0);
    let id = c.add_enemy_in_slot(fat, ai, Some("fat"));
    c.escape_monster(id);
    assert_eq!(c.gold_proportion(), 0.5);
    c.gremlin_gold_stolen = true;
    assert_eq!(c.gold_proportion(), 0.0);
}

#[test]
fn fixed_extra_gold_rewards_are_populated_and_paid_after_base_rewards() {
    let mut mgr = manager();
    let mut combat = Combat::new(mgr.take_run_from_owner(), 1);
    let mut owners = Vec::new();
    for (slot, amount) in [("fat1", 23), ("fat2", 17)] {
        let (fat, ai) = combat.create_monster(MonsterKind::FatGremlin, 0);
        let owner = combat.add_enemy_in_slot(fat, ai, Some(slot));
        combat.apply_targeted_power(owner, PowerId::Heist, amount, None, crate::combat::PLAYER);
        owners.push(owner);
    }
    mgr.combat = Some(combat);
    let mut baseline = mgr.clone();
    for owner in owners { mgr.combat.as_mut().unwrap().kill_creature(owner); }
    assert_eq!(mgr.run_ctx().extra_gold_reward, 40);
    baseline.generate_combat_rewards(RoomType::Monster);
    let mut expected_rng = baseline.run_ctx().rngs.rewards.clone();
    expected_rng.next_int(23, 23);
    expected_rng.next_int(17, 17);
    mgr.generate_combat_rewards(RoomType::Monster);
    assert_eq!(mgr.run_ctx().gold, baseline.run_ctx().gold + 40);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, expected_rng.counter);
    assert_eq!(mgr.run_ctx().extra_gold_reward, 0);
    assert!(mgr.combat.as_ref().unwrap().end_rewards.is_empty());
}

#[test]
fn combat_end_rewards_preserve_mixed_listener_order_and_owners() {
    use crate::combat::EndReward::*;
    let mut mgr = manager();
    let mut c = Combat::new(mgr.take_run_from_owner(), 1);
    c.add_player(50, 50, 3, 0, &[]);
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    c.start_combat();
    let enemy = c.enemies[0];
    c.apply_targeted_power(enemy, PowerId::Heist, 7, Some(enemy), 1);
    let card = c.run.deck.remove(0);
    let identity = card.instance_id;
    c.players[0].stolen_deck_cards.push(Some(card));
    let swipe = c.apply_targeted_power(enemy, PowerId::Swipe, 1, Some(enemy), 0).unwrap();
    c.powers[swipe as usize].vars[0] = 1;
    let k = c.new_card(0, CardId::ForbiddenGrimoire, false).unwrap();
    c.play_card_effect(k, None);
    c.creature_apply_power(1, PowerId::Royalties, 9, Some(1), None, false, false);
    for enemy in c.enemies.clone() { c.kill_creature(enemy); }
    c.check_combat_end();
    // CombatRoom.AddExtraReward appends death emissions before AfterCombatEnd;
    // the latter visits player zero's Grimoire before player one's Royalties.
    assert!(matches!(c.end_rewards.as_slice(), [
        Gold { owner: 1, amount: 7 },
        SpecialCard { owner: 0, card },
        CardRemoval { owner: 0 },
        Gold { owner: 1, amount: 9 },
    ] if card.instance_id == identity));
    assert_eq!(c.fault, None);
}

#[test]
fn the_hunt_emits_owned_room_cards_after_death_rewards_without_drawing_rng() {
    use crate::combat::EndReward;
    for owner in 0..4 {
        for room in [RoomType::Monster, RoomType::Elite, RoomType::Boss] {
            let mut mgr = manager();
            let mut c = Combat::new(mgr.take_run_from_owner(), 1);
            for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
            c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
            c.start_combat();
            c.run.room_type = room;
            let enemy = c.enemies[0];
            c.cr_mut(enemy).hp = 1;
            c.apply_targeted_power(enemy, PowerId::Heist, 7, Some(enemy), owner);
            let before = c.run.rngs.rewards.counter;
            let card = c.new_card(owner, CardId::TheHunt, false).unwrap();
            c.play_card_effect(card, Some(enemy));
            assert!(matches!(c.end_rewards.as_slice(), [
                EndReward::Gold { owner: a, amount: 7 },
                EndReward::Card { owner: b, room: r },
            ] if *a == owner && *b == owner && *r == room));
            assert_eq!(c.run.rngs.rewards.counter, before);
            assert_eq!(c.fault, None);
        }
    }
}

#[test]
fn the_hunt_room_reward_consumes_nine_draws_in_emission_order_and_keeps_foreign_cards() {
    use crate::combat::EndReward;
    let mut mgr = manager();
    let mut c = Combat::new(mgr.take_run_from_owner(), 1);
    c.add_player(50, 50, 3, 0, &[]);
    c.end_rewards = vec![
        EndReward::CardRemoval { owner: 0 },
        EndReward::Card { owner: 0, room: RoomType::Boss },
        EndReward::Gold { owner: 0, amount: 7 },
        EndReward::Card { owner: 1, room: RoomType::Monster },
    ];
    mgr.combat = Some(c);
    let mut expected = mgr.clone();
    let mut chosen = ArrayVec::new();
    let mut ids = Vec::new();
    for _ in 0..3 {
        let rarity = expected.roll_card_rarity(RoomType::Boss);
        let card = expected.generate_reward_card(rarity, &chosen).unwrap();
        chosen.push(card.id);
        ids.push(card.id);
    }
    expected.run_ctx_mut().rngs.rewards.next_int(7, 7);
    let before = mgr.run_ctx().rngs.rewards.counter;
    let gold = mgr.run_ctx().gold;
    mgr.append_combat_end_rewards();
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, before + 10);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, expected.run_ctx().rngs.rewards.counter);
    assert_eq!(mgr.run_ctx().gold, gold + 7);
    let RewardItemKind::Card { cards, .. } = &mgr.reward.items[1].kind else { panic!() };
    assert_eq!(cards.iter().map(|c| c.id).collect::<Vec<_>>(), ids);
    assert!(cards.iter().all(|c| c.rarity == CardRarity::Rare));
    assert!(matches!(mgr.combat.as_ref().unwrap().end_rewards.as_slice(),
        [EndReward::Card { owner: 1, room: RoomType::Monster }]));
}

#[test]
fn room_end_rewards_are_ordered_skippable_and_do_not_leak_foreign_seats() {
    use crate::combat::EndReward;
    let mut mgr = manager();
    let mut c = Combat::new(mgr.take_run_from_owner(), 1);
    c.add_player(50, 50, 3, 0, &[]);
    let original = c.run.deck.remove(0);
    let identity = original.instance_id;
    let cost = original.cost;
    let mut foreign = original.clone();
    foreign.owner = 1;
    c.end_rewards = vec![
        EndReward::CardRemoval { owner: 0 },
        EndReward::SpecialCard { owner: 0, card: original },
        EndReward::Gold { owner: 0, amount: 11 },
        EndReward::Gold { owner: 1, amount: 19 },
        EndReward::SpecialCard { owner: 1, card: foreign },
        EndReward::CardRemoval { owner: 1 },
    ];
    mgr.combat = Some(c);
    let deck = mgr.run_ctx().deck.len();
    let mut baseline = mgr.clone();
    baseline.combat.as_mut().unwrap().end_rewards.clear();
    baseline.generate_combat_rewards(RoomType::Monster);
    mgr.generate_combat_rewards(RoomType::Monster);
    assert_eq!(mgr.run_ctx().gold, baseline.run_ctx().gold + 11);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, baseline.run_ctx().rngs.rewards.counter + 1);
    assert_eq!(mgr.run_ctx().deck.len(), deck, "special cards are not auto-returned");
    assert!(matches!(mgr.combat.as_ref().unwrap().end_rewards.as_slice(), [
        EndReward::Gold { owner: 1, amount: 19 },
        EndReward::SpecialCard { owner: 1, .. }, EndReward::CardRemoval { owner: 1 },
    ]));
    let order: Vec<_> = mgr.ordered_remaining_reward_indices().into_iter()
        .filter_map(|i| match mgr.reward.items[i].kind {
            RewardItemKind::SpecialCard { .. } => Some(4),
            RewardItemKind::Card { .. } => Some(5),
            RewardItemKind::CardRemoval { .. } => Some(7), _ => None,
        }).collect();
    assert_eq!(order, [4,5,7]);
    mgr.skip_card_reward().unwrap();
    let pick = *mgr.available_actions().iter().find(|a| a.kind == RunActionKind::PickSpecialCard).unwrap();
    mgr.take_action(pick).unwrap();
    let returned = mgr.run_ctx().deck.iter().find(|c| c.instance_id == identity).unwrap();
    assert_eq!((returned.cost, returned.owner), (cost, 0));
    mgr.skip_reward_set().unwrap();
    assert_eq!(mgr.run_ctx().deck.len(), deck + 1, "skipped removal never edits deck");
}

#[test]
fn card_removal_reward_cancels_without_consuming_and_requires_confirmation() {
    let mut mgr = manager();
    let mut c = Combat::new(mgr.take_run_from_owner(), 1);
    c.end_rewards.push(crate::combat::EndReward::CardRemoval { owner: 0 });
    mgr.combat = Some(c);
    mgr.append_combat_end_rewards();
    mgr.phase = RunPhase::CardReward;
    let count = mgr.run_ctx().deck.len();
    mgr.open_reward(0).unwrap();
    mgr.confirm_pending_run_choice().unwrap(); // Cancel; keep the reward.
    assert!(!mgr.reward.items[0].consumed);
    assert_eq!(mgr.run_ctx().deck.len(), count);
    mgr.open_reward(0).unwrap();
    assert!(mgr.pending_run_choice.as_ref().unwrap().options.iter()
        .all(|c| c.card_id != CardId::AscendersBane));
    mgr.resolve_run_choice(0).unwrap();
    assert_eq!(mgr.run_ctx().deck.len(), count, "selection alone is not confirmation");
    mgr.confirm_pending_run_choice().unwrap();
    assert_eq!(mgr.run_ctx().deck.len(), count - 1);
    assert!(mgr.reward.items[0].consumed);
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
}

#[test]
fn secondary_owner_removal_reward_preserves_final_retained_guilty_properties() {
    let mut mgr=manager();
    let mut c=Combat::new(mgr.take_run_from_owner(),1);
    let mut guilty=Card::from_factory(CardId::Guilty,false,400).unwrap();guilty.owner=1;
    c.add_player(80,80,3,0,&[guilty.clone()]);
    let mut relic=Relic::new(RelicId::BingBong);relic.owner=1;
    relic.bing_bong_cards_to_skip=Some(Default::default());
    relic.bing_bong_cards_to_skip.as_mut().unwrap().insert(guilty);
    c.run.relics.push(relic);
    c.player_deck_mut(1)[0].vars.iter_mut().find(|(k,_)|*k==VarKey::Combats).unwrap().1=2;
    c.end_rewards.push(crate::combat::EndReward::CardRemoval{owner:1});
    mgr.combat=Some(c);mgr.phase=RunPhase::CardReward;
    mgr.reward=RewardState::default();
    mgr.reward.items.push(RewardItem {kind:RewardItemKind::CardRemoval{owner:1},
        consumed:false,skippable:true,skip_completes:true,add_injury_after:false,neows_bones_final:false});
    mgr.open_reward(0).unwrap();mgr.resolve_run_choice(0).unwrap();mgr.confirm_pending_run_choice().unwrap();
    let c=mgr.combat.as_ref().unwrap();
    assert!(c.player_deck(1).is_empty());
    let stored=c.run.relics.iter().find(|r|r.owner==1&&r.id==RelicId::BingBong).unwrap()
        .bing_bong_cards_to_skip.as_ref().unwrap().entries[0].as_ref().unwrap();
    assert_eq!(stored.var(VarKey::Combats),Some(2));
}

#[test]
fn skipped_special_card_reward_does_not_return_the_card() {
    let mut mgr = manager();
    let mut c = Combat::new(mgr.take_run_from_owner(), 1);
    let card = c.run.deck.remove(0);
    let identity = card.instance_id;
    c.end_rewards.push(crate::combat::EndReward::SpecialCard { owner: 0, card });
    mgr.combat = Some(c);
    mgr.append_combat_end_rewards();
    mgr.phase = RunPhase::CardReward;
    mgr.skip_reward_set().unwrap();
    assert!(mgr.run_ctx().deck.iter().all(|c| c.instance_id != identity));
}

#[test]
fn elite_gold_ignores_fat_escape_and_white_star_adds_boss_reward() {
    let mut mgr = manager();
    let mut c = Combat::new(mgr.take_run_from_owner(), 1);
    let (fat, ai) = c.create_monster(MonsterKind::FatGremlin, 0);
    let e = c.add_enemy_in_slot(fat, ai, Some("fat"));
    c.escape_monster(e);
    c.gremlin_gold_stolen = true;
    mgr.combat = Some(c);
    let mut baseline = mgr.clone();
    baseline.combat.as_mut().unwrap().escaped_enemies.clear();
    baseline.generate_combat_rewards(RoomType::Elite);
    mgr.generate_combat_rewards(RoomType::Elite);
    assert_eq!(mgr.run_ctx().gold, baseline.run_ctx().gold);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, baseline.run_ctx().rngs.rewards.counter);
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::WhiteStar));
    mgr.generate_combat_rewards(RoomType::Elite);
    assert!(mgr.unported.is_none());
    let RewardItemKind::Card { cards, .. } = &mgr.reward.items.last().unwrap().kind else {
        panic!("White Star card reward");
    };
    assert_eq!(cards.len(), 3);
    assert!(cards.iter().all(|card| card.rarity == CardRarity::Rare));
}

#[test]
fn multiplayer_pool_filter_survives_outside_combat() {
    let mut mgr = manager();
    mgr.player_count = 2;
    assert!(mgr.combat.is_none());
    assert!(mgr.is_multiplayer());
}

#[test]
fn guilty_after_combat_updates_every_players_deck() {
    let mut mgr = manager();
    mgr.run_ctx_mut().deck.clear();
    let mut combat = Combat::new(mgr.take_run_from_owner(), 17);
    combat.add_player(80, 80, 3, 0, &[]);
    for owner in 0..2 {
        combat.player_deck_mut(owner).push(
            Card::from_factory(CardId::Guilty, false, 100 + owner as u32).unwrap());
    }
    for remaining in (0..5).rev() {
        combat.fire_card_after_combat_end();
        for owner in 0..2 {
            let deck = combat.player_deck(owner);
            if remaining == 0 { assert!(deck.is_empty()); }
            else { assert_eq!(deck[0].var(VarKey::Combats), Some(remaining)); }
        }
    }
}

fn enter_event(mgr: &mut RunManager, id: crate::identity::EventId) {
    let run = mgr.run_ctx();
    mgr.current_event = super::super::events::initialize(
        id, run.rngs.seed, run.gold, 24, run.hp, run.max_hp,
    );
    mgr.phase = RunPhase::Event;
    mgr.refresh_event_option_locks();
}

#[test]
fn trade_and_inventory_metadata_getters_keep_typed_entry_state_without_rng_draws() {
    let mut mgr = manager();
    mgr.run_ctx_mut().total_floor = 20;
    mgr.run_ctx_mut().gold = 500;
    enter_event(&mut mgr,EventId::LuminousChoir);
    let price = mgr.observation_choir_price().unwrap();
    assert!((100..=149).contains(&price));
    let counter = mgr.current_event.as_ref().unwrap().rng.counter;
    assert_eq!(mgr.observation_choir_price(),Some(price));
    assert_eq!(mgr.current_event.as_ref().unwrap().rng.counter,counter);

    mgr.run_ctx_mut().relics = vec![Relic::new(RelicId::Anchor)];
    enter_event(&mut mgr,EventId::RelicTrader);
    mgr.initialize_relic_trader().unwrap();
    let before = mgr.run_ctx().rngs.rewards.counter;
    let offers = mgr.observation_relic_trader_offers().unwrap();
    assert_eq!(offers.len(),1);
    assert_eq!(offers[0].0,RelicId::Anchor);
    assert_eq!(offers[0].1,mgr.current_event.as_ref().unwrap().relics[0]);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter,before);

    mgr.run_ctx_mut().potions = vec![None,Some(PotionId::Fruitjuice)];
    enter_event(&mut mgr,EventId::StoneOfAllTime);
    mgr.initialize_stone_of_all_time().unwrap();
    let counter = mgr.current_event.as_ref().unwrap().rng.counter;
    assert_eq!(mgr.observation_stone_offers().unwrap().0,Some((1,PotionId::Fruitjuice)));
    assert_eq!(mgr.current_event.as_ref().unwrap().rng.counter,counter);

    enter_event(&mut mgr,EventId::FakeMerchant);
    mgr.initialize_fake_merchant().unwrap();
    let counter = mgr.run_ctx().rngs.shops.counter;
    let inventory = mgr.observation_fake_merchant_inventory().unwrap();
    assert_eq!(inventory.len(),6);
    assert!(inventory.iter().all(|&(_,_,stocked)| stocked));
    mgr.take_event_choice("buy_0").unwrap();
    let after = mgr.observation_fake_merchant_inventory().unwrap();
    assert_eq!((after[0].0,after[0].1),(inventory[0].0,inventory[0].1));
    assert!(!after[0].2);
    assert_eq!(mgr.run_ctx().rngs.shops.counter,counter);
    assert!(mgr.observation_relic_trader_offers().is_none());
    assert!(mgr.observation_stone_offers().is_none());
}

#[test]
fn event_metadata_getters_return_generated_targets_without_rng_or_hidden_item_leaks() {
    let mut mgr = manager();
    enter_event(&mut mgr, EventId::ColorfulPhilosophers);
    mgr.initialize_colorful_philosophers().unwrap();
    let counter = mgr.current_event.as_ref().unwrap().rng.counter;
    let pools: Vec<_> = (0..3).map(|i| mgr.observation_philosopher_pool(i).unwrap()).collect();
    assert!(!pools.contains(&crate::character::Character::Ironclad));
    assert!(mgr.observation_philosopher_pool(3).is_none());
    assert_eq!(mgr.current_event.as_ref().unwrap().rng.counter, counter);
    assert!(mgr.observation_ancient_relic("pool_1").is_none());

    enter_event(&mut mgr, EventId::DollRoom);
    assert!(mgr.observation_ancient_relic("random").is_none());
    mgr.take_event_choice("take_time").unwrap();
    for i in 0..2 {
        let event = mgr.current_event.as_ref().unwrap();
        assert_eq!(mgr.observation_ancient_relic(event.options[i].option_id.legacy_id()), Some(event.relics[i]));
    }
    assert!(mgr.observation_ancient_relic("doll_3").is_none());

    mgr.run_ctx_mut().gold = 200;
    enter_event(&mut mgr, EventId::CrystalSphere);
    mgr.take_event_choice("pay").unwrap();
    let counter = mgr.current_event.as_ref().unwrap().rng.counter;
    let hidden = mgr.observation_sphere_hidden_cells().unwrap();
    let event = mgr.current_event.as_ref().unwrap();
    assert_eq!(event.options.len(), hidden.len() * 2);
    assert_eq!(event.rng.counter, counter);
    assert!(mgr.observation_philosopher_pool(0).is_none());
}

fn recorded_event_manager(
    seed: u64,
    run_seed: u64,
    hp: i32,
    max_hp: i32,
    gold: i32,
    next_card_id: u32,
    rewards_counter: u64,
) -> RunManager {
    let mut mgr = RunManager::new(seed, RunOptions::default());
    mgr.unported = None;
    mgr.current_act = 1;
    let run = mgr.run_ctx_mut();
    run.rngs.seed = run_seed;
    run.hp = hp;
    run.max_hp = max_hp;
    run.gold = gold;
    run.next_card_id = next_card_id;
    run.rngs.rewards = Rng::with_counter(run_seed, Some("rewards"), rewards_counter);
    mgr
}

#[test]
fn explicitly_unknown_wongo_profile_remains_closed() {
    let mut mgr = manager();
    mgr.options.saved_wongo_points = None;
    let bags = mgr.relic_bags.player[0].clone();
    enter_event(&mut mgr, EventId::WelcomeToWongos);
    mgr.initialize_wongo().unwrap();
    assert!(mgr.current_event.as_ref().unwrap().options.is_empty());
    assert_eq!(mgr.relic_bags.player[0], bags);
    assert!(super::super::events::unported_reason(EventId::WelcomeToWongos).contains("SaveManager.Progress.WongoPoints"));
}

fn wongo_manager(points: i32) -> RunManager {
    let mut mgr = manager();
    mgr.options.saved_wongo_points = Some(points);
    mgr.run_ctx_mut().total_floor = 20;
    mgr.run_ctx_mut().gold = 500;
    mgr.relic_bags.player[0] = [vec![RelicId::Strawberry], vec![], vec![RelicId::TungstenRod], vec![]];
    enter_event(&mut mgr, EventId::WelcomeToWongos);
    mgr.initialize_wongo().unwrap();
    mgr
}

#[test]
fn wongo_saved_points_are_explicit_and_badge_uses_remainder_not_lifetime_total() {
    assert_eq!(RunOptions::default().saved_wongo_points,Some(0));
    for (saved, option, earned, cost) in [(1968,"bargain_bin",32,100),(1984,"featured",16,200),
        (1992,"mystery",8,300),(2000,"featured",16,200),(0,"bargain_bin",32,100)] {
        let mut mgr = wongo_manager(saved);
        let rewards = mgr.run_ctx().rngs.rewards.counter;
        mgr.take_event_choice(option).unwrap();
        assert_eq!(mgr.wongo_points_earned(), earned);
        assert_eq!(mgr.options.saved_wongo_points, Some(saved)); // saved profile never mutated
        assert_eq!(mgr.run_ctx().gold, 500 - cost);
        assert_eq!(mgr.run_ctx().has_owned_relic(crate::combat::PLAYER,
            RelicId::WongoCustomerAppreciationBadge), saved % 2000 + earned >= 2000);
        assert_eq!(mgr.run_ctx().rngs.rewards.counter, rewards);
        assert!(mgr.unported.is_none());
        assert!(mgr.current_event.is_none());
    }
}

#[test]
fn wongo_filters_shop_relics_from_front_and_waits_for_pickup_before_badge() {
    let mut mgr = wongo_manager(1984);
    mgr.relic_bags.player[0][2] = vec![RelicId::OldCoin, RelicId::Orrery, RelicId::TungstenRod];
    mgr.relic_bags.shared[2] = mgr.relic_bags.player[0][2].clone();
    // Reinitialize from a fresh event to check the entry pull itself.
    enter_event(&mut mgr, EventId::WelcomeToWongos);
    mgr.initialize_wongo().unwrap();
    assert_eq!(mgr.current_event.as_ref().unwrap().relics[0], RelicId::Orrery);
    assert_eq!(mgr.native_wongo_featured(),Some(RelicId::Orrery));
    mgr.current_event.as_mut().unwrap().options[1].enabled=false;
    assert_eq!(mgr.native_wongo_featured(),None,"locked options have no item title or hover tip");
    mgr.current_event.as_mut().unwrap().options[1].enabled=true;
    assert_eq!(mgr.relic_bags.player[0][2], vec![RelicId::OldCoin, RelicId::TungstenRod]);
    mgr.take_event_choice("featured").unwrap();
    assert_eq!(mgr.wongo_points_earned(), 0);
    assert!(!mgr.run_ctx().has_owned_relic(crate::combat::PLAYER, RelicId::WongoCustomerAppreciationBadge));
    for item in &mut mgr.reward.items { item.consumed = true; }
    mgr.reward.active = None;
    mgr.advance_rewards();
    assert_eq!(mgr.wongo_points_earned(), 16);
    assert!(mgr.run_ctx().has_owned_relic(crate::combat::PLAYER, RelicId::WongoCustomerAppreciationBadge));
}

#[test]
fn wongo_leave_draws_only_if_upgraded_and_keeps_basic_card_identity() {
    let mut mgr = wongo_manager(0);
    mgr.run_ctx_mut().deck = vec![Card::from_factory(CardId::StrikeIronclad, true, 77).unwrap()];
    let next = mgr.run_ctx().next_card_id;
    mgr.take_event_choice("leave").unwrap();
    assert!(!mgr.run_ctx().deck[0].upgraded);
    assert_eq!(mgr.run_ctx().deck[0].instance_id, 77);
    assert_eq!(mgr.run_ctx().next_card_id, next);
    assert_eq!(mgr.wongo_points_earned(), 0);
}

#[test]
fn wongo_downgrade_preserves_each_csharp_stateful_family_and_identity_without_allocating() {
    for &id in SELF_MUTATING_DAMAGE.iter().chain(SELF_MUTATING_BLOCK.iter()) {
        let mut mgr = wongo_manager(0);
        let mut card = Card::from_factory(id,true,78).unwrap();
        let base = Card::from_factory(id,false,0).unwrap();
        if SELF_MUTATING_DAMAGE.contains(&id) {
            card.damage = Some(card.damage.unwrap()+7);
            if let Some((_,v)) = card.vars.iter_mut().find(|(k,_)| *k == VarKey::Damage) { *v += 7; }
        } else {
            card.block = Some(card.block.unwrap()+7);
            if let Some((_,v)) = card.vars.iter_mut().find(|(k,_)| *k == VarKey::Block) { *v += 7; }
        }
        if id == CardId::SovereignBlade {
            if let Some((_,v)) = card.vars.iter_mut().find(|(k,_)| *k == VarKey::Repeat) { *v = 5; }
        }
        let damage = card.damage;
        mgr.run_ctx_mut().deck = vec![card];
        let next = mgr.run_ctx().next_card_id;
        mgr.take_event_choice("leave").unwrap();
        let after = &mgr.run_ctx().deck[0];
        assert!(!after.upgraded);
        assert_eq!(after.instance_id,78);
        assert_eq!(mgr.run_ctx().next_card_id,next);
        if SELF_MUTATING_DAMAGE.contains(&id) {
            assert_eq!(after.damage, if id == CardId::SovereignBlade { damage } else { base.damage.map(|d|d+7) });
        } else { assert_eq!(after.block,base.block.map(|b|b+7)); }
        if id == CardId::SovereignBlade { assert_eq!(after.var(VarKey::Repeat),Some(5)); }
        assert!(mgr.unported.is_none());
    }
}

#[test]
fn wongo_downgrade_reapplies_static_enchantments_after_resetting_keywords_and_cost() {
    for (id,name) in [(CardId::StrikeIronclad,"TezcatarasEmber"),(CardId::StrikeIronclad,"RoyallyApproved"),
        (CardId::StrikeIronclad,"Goopy"),(CardId::Bash,"Steady"),(CardId::Offering,"SoulsPower")] {
        let mut mgr = wongo_manager(0);
        let mut card = Card::from_factory(id,true,79).unwrap();
        crate::card::enchant::apply_static_enchantment(&mut card,name,1);
        let base_cost = Card::from_factory(id,false,79).unwrap().original_cost;
        mgr.run_ctx_mut().deck = vec![card];
        mgr.take_event_choice("leave").unwrap();
        let after = &mgr.run_ctx().deck[0];
        match name {
            "TezcatarasEmber" => assert!(after.has_kw(KW_ETERNAL)),
            "RoyallyApproved" => assert!(after.has_kw(KW_INNATE) && after.has_kw(KW_RETAIN)),
            "Goopy" => assert!(after.has_kw(KW_EXHAUST)),
            "Steady" => assert!(after.has_kw(KW_RETAIN)),
            "SoulsPower" => assert!(!after.has_kw(KW_EXHAUST)),
            _ => unreachable!(),
        }
        assert_eq!(after.original_cost,if name == "TezcatarasEmber" { 0 } else { base_cost });
        assert_eq!(after.enchantments.as_slice(),&[(EnchantmentId::from_name(name).unwrap(),1)]);
        assert!(!after.upgraded);
    }
}

#[test]
fn wongo_downgrade_preserves_affliction_instance_and_cost_modifiers_but_resets_local_keywords() {
    let mut mgr = wongo_manager(0);
    let mut card = Card::from_factory(CardId::Bash,true,80).unwrap();
    card.cv.affliction = Some(crate::card::CardAffliction::Hexed);
    card.cv.affliction_amount = 3;
    card.cv.affliction_added_ethereal = true;
    card.keywords |= KW_ETHEREAL;
    card.set_energy_cost(1,crate::card::CostExpiry::Combat,false);
    mgr.run_ctx_mut().deck = vec![card];
    mgr.take_event_choice("leave").unwrap();
    let after = &mgr.run_ctx().deck[0];
    assert_eq!(after.cv.affliction,Some(crate::card::CardAffliction::Hexed));
    assert_eq!(after.cv.affliction_amount,3);
    assert!(!after.has_kw(KW_ETHEREAL));
    assert_eq!(after.cost,1);
    assert_eq!(after.original_cost,2);
    assert!(!after.upgraded);
    assert_eq!(mgr.wongo_points_earned(), 0);
}

#[test]
fn wongo_ticket_adds_three_relic_rewards_only_after_five_combats_once() {
    let mut mgr = wongo_manager(0);
    mgr.take_event_choice("mystery").unwrap();
    let index = mgr.run_ctx().relics.iter().position(|r| r.id == RelicId::WongosMysteryTicket).unwrap();
    mgr.run_ctx_mut().relics[index].counter = 4;
    mgr.generate_combat_rewards(RoomType::Monster);
    assert!(!mgr.run_ctx().relics[index].flag1);
    mgr.run_ctx_mut().relics[index].counter = 5;
    mgr.generate_combat_rewards(RoomType::Monster);
    assert_eq!(mgr.reward.items.iter().filter(|item| matches!(item.kind, RewardItemKind::Relic(_))).count(), 3);
    assert!(mgr.run_ctx().relics[index].flag1);
    assert!(!mgr.run_ctx().relics[index].enabled);
    mgr.generate_combat_rewards(RoomType::Monster);
    assert_eq!(mgr.reward.items.iter().filter(|item| matches!(item.kind, RewardItemKind::Relic(_))).count(), 0);
}

fn fake_merchant_manager() -> RunManager {
    let mut mgr = manager();
    mgr.run_ctx_mut().total_floor = 20;
    mgr.run_ctx_mut().gold = 1000;
    enter_event(&mut mgr, EventId::FakeMerchant);
    mgr.initialize_fake_merchant().unwrap();
    mgr
}

#[test]
fn fake_merchant_shuffles_nine_then_prices_six_with_shops_rng_not_rewards() {
    let mut mgr = manager();
    mgr.run_ctx_mut().total_floor = 20;
    mgr.run_ctx_mut().gold = 1000;
    enter_event(&mut mgr, EventId::FakeMerchant);
    let mut event_rng = mgr.current_event.as_ref().unwrap().rng.clone();
    let mut shops = mgr.run_ctx().rngs.shops.clone();
    let rewards = mgr.run_ctx().rngs.rewards.counter;
    let mut relics = [RelicId::FakeAnchor, RelicId::FakeBloodVial, RelicId::FakeHappyFlower,
        RelicId::FakeLeesWaffle, RelicId::FakeMango, RelicId::FakeOrichalcum,
        RelicId::FakeSneckoEye, RelicId::FakeStrikeDummy, RelicId::FakeVenerableTeaSet];
    for i in (1..9).rev() {
        let j = event_rng.next_int_exclusive(0, i as i64 + 1) as usize;
        relics.swap(i, j);
    }
    let mut costs = vec![];
    for _ in 0..6 {
        // C# multiplication is float, widened only for Math.Round(double).
        let product = 50.0_f32 * shops.next_float_range(0.85, 1.15) as f32;
        costs.push((product as f64).round_ties_even() as i32);
    }
    mgr.initialize_fake_merchant().unwrap();
    let event = mgr.current_event.as_ref().unwrap();
    assert_eq!(event.rng.counter, 8);
    assert_eq!(event.relics.as_slice(), &relics[..6]);
    assert_eq!(&event.ints[..6], costs.as_slice());
    assert_eq!(mgr.run_ctx().rngs.shops.counter, shops.counter);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, rewards);
    let actions = mgr.available_actions();
    for i in 0..6 {
        assert_eq!(actions[i].price, Some(costs[i]));
        assert_eq!(actions[i].relic_id, Some(relics[i]));
    }
}

#[test]
fn fake_merchant_event_prices_ignore_membership_and_courier_never_restocks() {
    let mut plain = fake_merchant_manager();
    let mut modified = manager();
    modified.run_ctx_mut().total_floor = 20;
    modified.run_ctx_mut().gold = 1000;
    modified.run_ctx_mut().relics.extend([RelicId::MembershipCard, RelicId::TheCourier,
        RelicId::MawBank].into_iter().map(Relic::new));
    enter_event(&mut modified, EventId::FakeMerchant);
    modified.initialize_fake_merchant().unwrap();
    assert_eq!(plain.current_event.as_ref().unwrap().ints, modified.current_event.as_ref().unwrap().ints);
    let shops = modified.run_ctx().rngs.shops.counter;
    let (id, price) = {
        let event = modified.current_event.as_ref().unwrap();
        (event.relics[0], event.ints[0])
    };
    modified.take_event_choice("buy_0").unwrap();
    assert_eq!(modified.run_ctx().gold, 1000 - price);
    assert!(modified.run_ctx().has_owned_relic(crate::combat::PLAYER, id));
    assert_eq!(modified.run_ctx().rngs.shops.counter, shops);
    assert!(!modified.current_event.as_ref().unwrap().options[0].enabled);
    assert!(modified.take_event_choice("buy_0").is_err());
    assert!(modified.run_ctx().relics.iter().find(|r| r.id == RelicId::MawBank).unwrap().flag1);
    plain.take_event_choice("leave").unwrap();
    assert_eq!(plain.phase(), RunPhase::MapChoice);
    assert!(plain.current_event.is_none());
}

#[test]
fn fake_merchant_pickups_heal_ten_percent_or_gain_three_max_hp() {
    for id in [RelicId::FakeLeesWaffle, RelicId::FakeMango, RelicId::FakeSneckoEye] {
        let mut mgr = fake_merchant_manager();
        mgr.run_ctx_mut().hp = 30;
        mgr.run_ctx_mut().max_hp = 83;
        mgr.current_event.as_mut().unwrap().relics[0] = id;
        mgr.take_event_choice("buy_0").unwrap();
        assert_eq!((mgr.run_ctx().hp, mgr.run_ctx().max_hp), match id {
            RelicId::FakeLeesWaffle => (38, 83),
            RelicId::FakeMango => (33, 86),
            _ => (30, 83),
        });
        assert!(mgr.unported.is_none());
        assert_eq!(mgr.phase(), RunPhase::Event);
    }
}

#[test]
fn fake_merchant_foul_potion_starts_fight_and_awards_only_remaining_stock_after_win() {
    let mut mgr = fake_merchant_manager();
    mgr.run_ctx_mut().hp = 9999;
    mgr.run_ctx_mut().max_hp = 9999;
    let index = mgr.current_event.as_ref().unwrap().relics.iter()
        .position(|&id| id != RelicId::FakeSneckoEye).unwrap();
    let option = mgr.current_event.as_ref().unwrap().options[index].option_id;
    mgr.take_event_choice(option.legacy_id()).unwrap();
    let inventory = mgr.observation_fake_merchant_inventory().unwrap();
    let expected: Vec<_> = std::iter::once(RelicId::FakeMerchantsRug)
        .chain(inventory.iter().filter_map(|&(id,_,stocked)| stocked.then_some(id))).collect();
    mgr.run_ctx_mut().potions = vec![Some(PotionId::Foulpotion)];
    let gold = mgr.run_ctx().gold;
    let action = mgr.available_actions().into_iter()
        .find(|a| a.kind == RunActionKind::UsePotion && a.potion_id == Some(PotionId::Foulpotion)).unwrap();
    mgr.take_action(action).unwrap();
    assert_eq!(mgr.run_ctx().gold,gold); // not ordinary merchant's immediate +100
    assert_eq!(mgr.run_ctx().potions,vec![None]);
    assert_eq!(mgr.phase(),RunPhase::Combat);
    let combat = mgr.combat().unwrap();
    assert_eq!(combat.cr(combat.enemies[0]).max_hp,175);
    assert_eq!(combat.cr(combat.enemies[0]).monster_id,"FAKE_MERCHANT_MONSTER");
    assert!(mgr.current_event.is_none());
    for _ in 0..2000 {
        if mgr.phase() != RunPhase::Combat { break; }
        let actions = mgr.available_actions();
        let action = actions.iter().find(|a| a.enabled && a.kind == RunActionKind::PlayCard)
            .or_else(|| actions.iter().find(|a| a.kind == RunActionKind::EndTurn)).unwrap();
        mgr.take_action(*action).unwrap();
        assert!(mgr.unported.is_none(),"{:?}",mgr.unported);
    }
    assert_eq!(mgr.phase(),RunPhase::CardReward);
    assert!(mgr.combat().unwrap().won);
    assert_eq!(mgr.run_ctx().gold,gold+300);
    let actual: Vec<_> = mgr.reward.items.iter().filter_map(|item|
        if let RewardItemKind::Relic(id) = item.kind { Some(id) } else { None }).collect();
    assert_eq!(actual,expected);
    assert!(mgr.reward.items.iter().any(|item| matches!(item.kind,RewardItemKind::Card {..})));
    assert!(mgr.fake_merchant_loot.is_empty());
}

fn ranwid_manager() -> RunManager {
    let mut mgr = manager();
    mgr.run_ctx_mut().total_floor = 20;
    mgr.run_ctx_mut().relics = vec![Relic::new(RelicId::Anchor)];
    mgr.run_ctx_mut().potions = vec![None, Some(PotionId::Fairyinabottle)];
    mgr.relic_bags.player[0] = [vec![], vec![], vec![RelicId::Strawberry, RelicId::Pear], vec![]];
    enter_event(&mut mgr, EventId::RanwidTheElder);
    mgr.initialize_ranwid().unwrap();
    mgr
}

#[test]
fn ranwid_entry_uses_inventory_order_and_locks_potions_until_trade_finishes() {
    let mut mgr = ranwid_manager();
    let event = mgr.current_event.as_ref().unwrap();
    assert_eq!((event.ints[0], event.ints[1], event.rng.counter), (1, 0, 2));
    assert!(!mgr.available_actions().iter().any(|a|
        matches!(a.kind, RunActionKind::UsePotion | RunActionKind::DiscardPotion)));
    let mut discard = RunAction::new(RunActionKind::DiscardPotion);
    discard.slot_index = Some(1);
    assert!(mgr.take_action(discard).is_err());
    let before = mgr.run_ctx().rngs.rewards.counter;
    mgr.take_event_choice("potion").unwrap();
    assert_eq!(mgr.run_ctx().potions, vec![None, None]);
    assert!(mgr.run_ctx().has_owned_relic(crate::combat::PLAYER, RelicId::Strawberry));
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, before + 1);
    assert!(mgr.current_event.is_none());
    assert!(mgr.unported.is_none());
}

#[test]
fn ranwid_gold_pays_once_and_relic_trade_pulls_two_in_sequence() {
    for option in ["gold", "relic"] {
        let mut mgr = ranwid_manager();
        mgr.run_ctx_mut().gold = 150;
        let counter = mgr.run_ctx().rngs.rewards.counter;
        mgr.take_event_choice(option).unwrap();
        let relic_trade = option == "relic";
        assert_eq!(mgr.run_ctx().gold, if relic_trade { 150 } else { 50 });
        assert_eq!(mgr.run_ctx().rngs.rewards.counter, counter + if relic_trade { 2 } else { 1 });
        assert_eq!(mgr.run_ctx().has_owned_relic(crate::combat::PLAYER, RelicId::Anchor), !relic_trade);
        assert!(mgr.run_ctx().has_owned_relic(crate::combat::PLAYER, RelicId::Strawberry));
        assert_eq!(mgr.run_ctx().has_owned_relic(crate::combat::PLAYER, RelicId::Pear), relic_trade);
        assert_eq!(mgr.phase(), RunPhase::MapChoice);
        assert!(mgr.current_event.is_none());
    }
}

#[test]
fn ranwid_waits_for_first_pickup_selector_before_second_rng_roll_and_pickup() {
    let mut mgr = ranwid_manager();
    // Inject selector-producing pickups to exercise the awaited continuation.
    mgr.relic_bags.player[0][2] = vec![RelicId::PreservedFog, RelicId::Orrery];
    let counter = mgr.run_ctx().rngs.rewards.counter;
    let len = mgr.run_ctx().deck.len();
    mgr.take_event_choice("relic").unwrap();
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, counter + 1);
    assert_eq!(mgr.relic_bags.player[0][2], vec![RelicId::Orrery]);
    assert_eq!(mgr.run_ctx().deck.len(), len);
    let choice = mgr.pending_run_choice.take().unwrap();
    mgr.apply_run_choice(choice, &[0, 1, 2]).unwrap();
    // Orrery populates five card rewards only after the first selector resolves.
    assert!(mgr.run_ctx().rngs.rewards.counter > counter + 2);
    assert_eq!(mgr.run_ctx().deck.len(), len - 2);
    assert_eq!(mgr.reward.items.len(), 5);
    assert_eq!(mgr.phase(), RunPhase::CardReward);
    assert!(mgr.current_event.is_some());
    let counter = mgr.run_ctx().rngs.rewards.counter;
    for item in &mut mgr.reward.items { item.consumed = true; }
    mgr.reward.active = None;
    mgr.advance_rewards();
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, counter);
    assert!(mgr.current_event.is_none());
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
}

#[test]
fn ranwid_waits_for_cauldron_potions_before_second_bag_pull() {
    let mut mgr = ranwid_manager();
    mgr.relic_bags.player[0][2] = vec![RelicId::Cauldron, RelicId::Pear];
    let counter = mgr.run_ctx().rngs.rewards.counter;
    mgr.take_event_choice("relic").unwrap();
    assert!(mgr.unported.is_none());
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, counter + 11);
    assert_eq!(mgr.relic_bags.player[0][2], vec![RelicId::Pear]);
    assert!(mgr.current_event.is_some());
    assert_eq!(mgr.reward.items.len(), 5);
    mgr.skip_reward_set().unwrap();
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, counter + 12);
    assert!(mgr.run_ctx().has_owned_relic(crate::combat::PLAYER, RelicId::Pear));
    assert!(mgr.current_event.is_none());
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
}

#[test]
fn relic_trader_entry_shuffles_owned_then_pulls_all_three_before_choice() {
    // RelicTrader.OwnedRelics/NewRelics/CalculateVars and ListExtensions.StableShuffle.
    let mut mgr = manager();
    mgr.run_ctx_mut().relics = [
        RelicId::PenNib, RelicId::Lantern, RelicId::Anchor,
        RelicId::BagOfPreparation, RelicId::TungstenRod,
    ].into_iter().map(Relic::new).collect();
    let mut used = Relic::new(RelicId::LizardTail);
    used.flag1 = true;
    mgr.run_ctx_mut().relics.push(used);
    let mut spent = Relic::new(RelicId::MawBank);
    spent.flag1 = true;
    mgr.run_ctx_mut().relics.push(spent);
    mgr.relic_bags.player[0] = [vec![], vec![], vec![RelicId::Strawberry, RelicId::Pear, RelicId::Mango], vec![]];
    mgr.relic_bags.shared = [vec![RelicId::Pear, RelicId::Mango, RelicId::Strawberry], vec![], vec![], vec![]];
    enter_event(&mut mgr, EventId::RelicTrader);
    let mut rng = mgr.current_event.as_ref().unwrap().rng.clone();
    let mut owned = vec![RelicId::Anchor, RelicId::BagOfPreparation, RelicId::Lantern,
        RelicId::PenNib, RelicId::TungstenRod];
    for i in (1..owned.len()).rev() {
        let j = rng.next_int_exclusive(0, i as i64 + 1) as usize;
        owned.swap(i, j);
    }
    let rewards = mgr.run_ctx().rngs.rewards.counter;
    let niche = mgr.run_ctx().rngs.niche.counter;
    mgr.initialize_relic_trader().unwrap();
    let event = mgr.current_event.as_ref().unwrap();
    assert_eq!(event.rng.counter, 4);
    assert_eq!(event.relics.as_slice(), &[RelicId::Strawberry, RelicId::Pear, RelicId::Mango]);
    for i in 0..3 {
        assert_eq!(mgr.run_ctx().relics[event.ints[i] as usize].id, owned[i]);
    }
    let removed = mgr.run_ctx().relics[event.ints[1] as usize].id;
    let owned_count = mgr.run_ctx().relics.iter().filter(|r| r.id == removed).count();
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, rewards + 3);
    assert_eq!(mgr.run_ctx().rngs.niche.counter, niche);
    assert!(mgr.relic_bags.player[0].iter().all(Vec::is_empty));
    assert!(mgr.relic_bags.shared.iter().all(Vec::is_empty));
    let (hp, max_hp) = (mgr.run_ctx().hp, mgr.run_ctx().max_hp);
    mgr.take_event_choice("trade_1").unwrap();
    assert_eq!(mgr.run_ctx().relics.iter().filter(|r| r.id == removed).count(), owned_count - 1);
    assert!(mgr.run_ctx().relics.iter().any(|r| r.id == RelicId::Pear));
    assert_eq!((mgr.run_ctx().hp, mgr.run_ctx().max_hp), (hp + 10, max_hp + 10));
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, rewards + 3);
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
    assert!(mgr.unported.is_none());
}

#[test]
fn relic_trader_bags_prune_every_rarity_escalate_and_fall_back_without_extra_rng() {
    // GetAvailableDeque prunes ALL deques; rare exhaustion returns Circlet,
    // never wraps back down to a common relic. Solo has no MP fallback queue.
    let mut mgr = manager();
    mgr.run_ctx_mut().total_floor = 41;
    mgr.relic_bags.player[0] = [
        vec![RelicId::OldCoin], vec![RelicId::TheBoot],
        vec![RelicId::WhiteStar], vec![RelicId::Shovel],
    ];
    mgr.relic_bags.shared = [vec![RelicId::TheBoot], vec![], vec![], vec![]];
    let counter = mgr.run_ctx().rngs.rewards.counter;
    assert_eq!(mgr.pull_player_relic(Some(0), false), RelicId::TheBoot);
    assert!(mgr.relic_bags.player[0].iter().all(Vec::is_empty));
    assert!(mgr.relic_bags.shared.iter().all(Vec::is_empty));
    mgr.relic_bags.player[0][0].push(RelicId::Anchor);
    assert_eq!(mgr.pull_player_relic(Some(2), false), RelicId::Circlet);
    assert_eq!(mgr.relic_bags.player[0][0], vec![RelicId::Anchor]);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, counter);
}

#[test]
fn relic_trader_calculate_vars_with_one_owned_still_pulls_three_but_empty_pulls_none() {
    for count in [0, 1] {
        let mut mgr = manager();
        mgr.run_ctx_mut().relics = vec![Relic::new(RelicId::Anchor); count];
        mgr.relic_bags.player[0] = [vec![], vec![], vec![], vec![]];
        enter_event(&mut mgr, EventId::RelicTrader);
        let before = mgr.run_ctx().rngs.rewards.counter;
        mgr.initialize_relic_trader().unwrap();
        assert_eq!(mgr.run_ctx().rngs.rewards.counter, before + if count == 0 { 0 } else { 3 });
        let event = mgr.current_event.as_ref().unwrap();
        assert_eq!(event.rng.counter, 0);
        assert_eq!(event.options.len(), 1);
        assert_eq!(event.relics.len(), if count == 0 { 0 } else { 3 });
    }
}

#[test]
fn stone_lift_uses_nonempty_slots_discards_without_drinking_and_gains_ten_max_hp() {
    // StoneOfAllTime.GenerateInitialOptions/Lift: one NextItem on entry,
    // discard (NOT use), +10 max/current HP, then NextInt(100).
    let mut mgr = manager();
    mgr.run_ctx_mut().potions = vec![None, Some(PotionId::Fairyinabottle)];
    mgr.run_ctx_mut().hp = 30;
    mgr.run_ctx_mut().max_hp = 80;
    let rewards = mgr.run_ctx().rngs.rewards.counter;
    enter_event(&mut mgr, EventId::StoneOfAllTime);
    mgr.initialize_stone_of_all_time().unwrap();
    let event = mgr.current_event.as_ref().unwrap();
    assert_eq!((event.ints[0], event.rng.counter), (1, 1));
    assert!(!mgr.available_actions().iter().any(|a|
        matches!(a.kind, RunActionKind::UsePotion | RunActionKind::DiscardPotion)));
    let mut discard = RunAction::new(RunActionKind::DiscardPotion);
    discard.slot_index = Some(1);
    assert!(mgr.take_action(discard).is_err());
    mgr.take_event_choice("lift").unwrap();
    assert_eq!((mgr.run_ctx().hp, mgr.run_ctx().max_hp), (40, 90));
    assert_eq!(mgr.run_ctx().potions, vec![None, None]);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, rewards);
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
}

#[test]
fn nonupeipe_shuffles_complete_conditional_pool_and_heals_before_offering() {
    for count in [3,4] {
        let mut mgr=manager();
        mgr.options.ascension=0;
        mgr.run_ctx_mut().hp=1;
        mgr.run_ctx_mut().deck=(0..count).map(|i|
            Card::from_factory(CardId::StrikeIronclad,false,i+1).unwrap()).collect();
        let mut pool=vec![RelicId::BlessedAntler,RelicId::BrilliantScarf,RelicId::DelicateFrond,
            RelicId::DiamondDiadem,RelicId::FurCoat,RelicId::Glitter,RelicId::JewelryBox,
            RelicId::LoomingFruit,RelicId::SignetRing];
        if count==4 {pool.push(RelicId::BeautifulBracelet);}
        let mut rng=Rng::new(mgr.run_ctx().rngs.seed.wrapping_add(deterministic_hash64("NONUPEIPE")));
        for i in (1..pool.len()).rev() {
            let j=rng.next_int_exclusive(0,i as i64+1) as usize;
            pool.swap(i,j);
        }
        enter_event(&mut mgr,EventId::Nonupeipe);
        mgr.initialize_nonupeipe().unwrap();
        assert_eq!(mgr.run_ctx().hp,mgr.run_ctx().max_hp);
        let event=mgr.current_event.as_ref().unwrap();
        assert_eq!(event.rng.counter,if count==4 {9}else{8});
        assert_eq!(event.relics.as_slice(),&pool[..3]);
        for (i,option) in event.options.iter().enumerate() {
            assert_eq!(mgr.observation_ancient_relic(option.option_id.legacy_id()),Some(pool[i]));
        }
    }
}

#[test]
fn nonupeipe_pickups_use_csharp_amounts() {
    for id in [RelicId::JewelryBox,RelicId::LoomingFruit,RelicId::SignetRing,
        RelicId::BeautifulBracelet] {
        let mut mgr=manager();
        let hp=mgr.run_ctx().max_hp;
        let gold=mgr.run_ctx().gold;
        let niche=mgr.run_ctx().rngs.niche.counter;
        let eligible=mgr.run_ctx().deck.iter().filter(|c|RunManager::event_can_enchant(c,"Swift")).count();
        let mut event=EventState::new(EventId::Nonupeipe,Rng::new(1));
        event.options.push(EventOption::new("relic_1","Relic","Gain an ancient relic"));
        event.relics.push(id);
        mgr.current_event=Some(event);mgr.phase=RunPhase::Event;
        mgr.take_event_choice("relic_1").unwrap();
        assert_eq!(mgr.unported,None);
        assert_eq!(mgr.run_ctx().relics.last().unwrap().id,id);
        match id {
            RelicId::JewelryBox=>assert_eq!(mgr.run_ctx().deck.last().unwrap().id,CardId::Apotheosis),
            RelicId::LoomingFruit=>assert_eq!(mgr.run_ctx().max_hp,hp+31),
            RelicId::SignetRing=>assert_eq!(mgr.run_ctx().gold,gold+888),
            RelicId::BeautifulBracelet=>{
                assert_eq!(mgr.run_ctx().rngs.niche.counter,niche+eligible.saturating_sub(1) as u64);
                assert_eq!(mgr.run_ctx().deck.iter().filter(|c|c.enchantments.contains(&(crate::identity::EnchantmentId::Swift,2))).count(),4);
            }
            _=>unreachable!(),
        }
    }
}

#[test]
fn ranwid_public_offers_are_retained_instances_without_additional_rng() {
    let mut mgr=manager();
    mgr.run_ctx_mut().potions=vec![None,Some(PotionId::Bloodpotion)];
    mgr.run_ctx_mut().relics=vec![Relic::new(RelicId::Anchor)];
    enter_event(&mut mgr,EventId::RanwidTheElder);
    mgr.initialize_ranwid().unwrap();
    let counter=mgr.current_event.as_ref().unwrap().rng.counter;
    assert_eq!(mgr.observation_ranwid_offers(),Some((Some((1,PotionId::Bloodpotion)),Some((0,RelicId::Anchor)))));
    assert_eq!(mgr.current_event.as_ref().unwrap().rng.counter,counter);
}

fn choose_vakuu_relic(mgr: &mut RunManager, id: RelicId) {
    let mut event = EventState::new(EventId::Vakuu, Rng::new(1));
    event.options.push(EventOption::new("relic_1", "Relic", "Gain an ancient relic"));
    event.relics.push(id);
    mgr.current_event = Some(event);
    mgr.phase = RunPhase::Event;
    mgr.take_event_choice("relic_1").unwrap();
}

#[test]
fn vakuu_shuffles_every_pool_before_taking_first_and_exposes_typed_offers() {
    // Vakuu.cs GenerateInitialOptions: UnstableShuffle is descending Fisher-Yates.
    for seed in [1_u64, 4100000, 20261006] {
        let mut mgr = manager();
        let mut rng = Rng::new(seed.wrapping_add(deterministic_hash64("VAKUU")));
        let mut expected = Vec::new();
        for mut pool in [
            vec![RelicId::BloodSoakedRose, RelicId::WhisperingEarring, RelicId::Fiddle],
            vec![RelicId::PreservedFog, RelicId::SereTalon, RelicId::DistinguishedCape],
            vec![RelicId::ChoicesParadox, RelicId::MusicBox, RelicId::LordsParasol, RelicId::JeweledMask],
        ] {
            for i in (1..pool.len()).rev() {
                let j = rng.next_int_exclusive(0, i as i64 + 1) as usize;
                pool.swap(i, j);
            }
            expected.push(pool[0]);
        }
        mgr.current_event = super::super::events::initialize(EventId::Vakuu, seed, 100, 24, 80, 80);
        let event = mgr.current_event.as_ref().unwrap();
        assert_eq!(event.rng.counter, 7);
        assert_eq!(event.relics.as_slice(), expected.as_slice());
        for (index, option) in event.options.iter().enumerate() {
            assert_eq!(mgr.observation_ancient_relic(option.option_id.legacy_id()), Some(expected[index]));
        }
    }
}

#[test]
fn vakuu_cape_draws_distinct_curses_from_csharp_pool_then_appends_three_apparitions() {
    // CurseCardPool.GenerateAllCards minus the eight CanBeGeneratedByModifiers=false
    // models. No epoch filter override or multiplayer-only curses exists.
    let mut mgr = manager();
    mgr.run_ctx_mut().deck.clear();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::DarkstonePeriapt));
    let (hp, max_hp, next) = (mgr.run_ctx().hp, mgr.run_ctx().max_hp, mgr.run_ctx().next_card_id);
    let mut rng = mgr.run_ctx().rngs.niche.clone();
    let counter = rng.counter;
    let mut pool = vec![CardId::Clumsy, CardId::Debt, CardId::Decay, CardId::Doubt,
        CardId::Guilty, CardId::Injury, CardId::Normality, CardId::Regret, CardId::Shame, CardId::Writhe];
    let a = pool.remove(rng.next_int_exclusive(0, 10) as usize);
    let b = pool.remove(rng.next_int_exclusive(0, 9) as usize);
    choose_vakuu_relic(&mut mgr, RelicId::DistinguishedCape);
    assert_eq!(mgr.run_ctx().deck.iter().map(|c| c.id).collect::<Vec<_>>(),
        vec![a, b, CardId::Apparition, CardId::Apparition, CardId::Apparition]);
    assert_ne!(a, b);
    assert_eq!(mgr.run_ctx().rngs.niche.counter, counter + 2);
    assert_eq!(mgr.run_ctx().next_card_id, next + 5);
    assert_eq!((mgr.run_ctx().hp, mgr.run_ctx().max_hp), (hp + 12, max_hp + 12));
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
    assert!(mgr.unported.is_none());
}

#[test]
fn vakuu_rose_talon_and_passive_pickups_follow_csharp() {
    let mut rose = manager();
    let niche = rose.run_ctx().rngs.niche.counter;
    choose_vakuu_relic(&mut rose, RelicId::BloodSoakedRose);
    assert_eq!(rose.run_ctx().deck.last().unwrap().id, CardId::Enthralled);
    assert_eq!(rose.run_ctx().rngs.niche.counter, niche);
    let mut talon = manager();
    talon.run_ctx_mut().hp = 78;
    talon.run_ctx_mut().max_hp = 80;
    let len = talon.run_ctx().deck.len();
    choose_vakuu_relic(&mut talon, RelicId::SereTalon);
    assert_eq!((talon.run_ctx().hp, talon.run_ctx().max_hp), (71, 71));
    assert_eq!(talon.run_ctx().deck[len..].iter().map(|c| c.id).collect::<Vec<_>>(),
        vec![CardId::Wish; 3]);
    for id in [RelicId::WhisperingEarring, RelicId::Fiddle, RelicId::ChoicesParadox,
        RelicId::MusicBox, RelicId::LordsParasol, RelicId::JeweledMask] {
        let mut mgr = manager();
        let len = mgr.run_ctx().deck.len();
        choose_vakuu_relic(&mut mgr, id);
        assert!(mgr.run_ctx().has_owned_relic(crate::combat::PLAYER, id));
        assert_eq!(mgr.run_ctx().deck.len(), len);
        assert!(mgr.unported.is_none());
    }
}

#[test]
fn vakuu_fog_waits_for_removal_before_adding_folly_and_autoselects_small_decks() {
    let mut mgr = manager();
    let before = mgr.run_ctx().deck.len();
    choose_vakuu_relic(&mut mgr, RelicId::PreservedFog);
    assert_eq!(mgr.run_ctx().deck.len(), before);
    let choice = mgr.pending_run_choice.take().unwrap();
    assert_eq!((choice.min_choices, choice.max_choices, choice.allow_skip), (3, 3, false));
    mgr.apply_run_choice(choice, &[0, 1, 2]).unwrap();
    assert_eq!(mgr.run_ctx().deck.len(), before - 2);
    assert_eq!(mgr.run_ctx().deck.last().unwrap().id, CardId::Folly);
    for count in 0..=3 {
        let mut small = manager();
        small.run_ctx_mut().deck.truncate(count);
        choose_vakuu_relic(&mut small, RelicId::PreservedFog);
        assert!(small.pending_run_choice.is_none());
        assert_eq!(small.run_ctx().deck.len(), 1);
        assert_eq!(small.run_ctx().deck[0].id, CardId::Folly);
        assert_eq!(small.phase(), RunPhase::MapChoice);
    }
}

#[test]
fn vakuu_talon_lethal_max_hp_loss_revives_before_clamping_max_hp() {
    let mut mgr = manager();
    mgr.run_ctx_mut().hp = 8;
    mgr.run_ctx_mut().max_hp = 8;
    mgr.run_ctx_mut().potions.clear();
    choose_vakuu_relic(&mut mgr, RelicId::SereTalon);
    assert_eq!((mgr.run_ctx().hp, mgr.run_ctx().max_hp), (0, 1));
    assert_eq!(mgr.phase(), RunPhase::RunOver);
    assert!(mgr.run_ctx().deck.iter().rev().take(3).all(|c| c.id == CardId::Wish));

    // CreatureCmd.LoseMaxHp awaits Damage (and revival) before SetMaxHp.
    let mut mgr = manager();
    mgr.run_ctx_mut().hp = 8;
    mgr.run_ctx_mut().max_hp = 8;
    mgr.run_ctx_mut().potions = vec![Some(PotionId::Fairyinabottle)];
    let mut event = EventState::new(EventId::Vakuu, Rng::new(1));
    event.options.push(EventOption::new("relic_1", "Sere Talon", "Gain an ancient relic"));
    event.relics.push(RelicId::SereTalon);
    mgr.current_event = Some(event);
    mgr.phase = RunPhase::Event;
    mgr.take_event_choice("relic_1").unwrap();
    assert!(mgr.unported.is_none());
    assert_eq!((mgr.run_ctx().hp, mgr.run_ctx().max_hp), (1, 1));
    assert_eq!(mgr.run_ctx().potions, vec![None]);
}

#[test]
fn stone_push_revives_before_enchant_selection_and_fairy_precedes_tail() {
    for (fairy,expected_hp) in [(true,3),(false,6)] {
        let mut mgr=manager();
        mgr.run_ctx_mut().max_hp=13;
        mgr.run_ctx_mut().hp=5;
        mgr.run_ctx_mut().potions=if fairy {vec![None,Some(PotionId::Fairyinabottle)]}
            else{vec![Some(PotionId::Bloodpotion)]};
        mgr.run_ctx_mut().relics.push(Relic::new(RelicId::LizardTail));
        mgr.run_ctx_mut().deck=vec![
            Card::from_factory(CardId::StrikeIronclad,false,10).unwrap(),
            Card::from_factory(CardId::StrikeIronclad,false,11).unwrap()];
        enter_event(&mut mgr,EventId::StoneOfAllTime);
        mgr.initialize_stone_of_all_time().unwrap();
        mgr.take_event_choice("push").unwrap();
        assert_eq!(mgr.run_ctx().hp,expected_hp);
        assert!(!mgr.run_ctx().potions.contains(&Some(PotionId::Fairyinabottle)));
        assert_eq!(mgr.run_ctx().relics.iter().find(|r|r.id==RelicId::LizardTail).unwrap().flag1,!fairy);
        assert!(mgr.pending_run_choice.is_some());
        assert_eq!(mgr.current_event.as_ref().unwrap().rng.counter,1);
        mgr.resolve_run_choice(1).unwrap();
        assert!(mgr.run_ctx().deck[1].enchantments.contains(&(EnchantmentId::Vigorous,8)));
        assert_eq!(mgr.phase(),RunPhase::MapChoice);
        assert!(mgr.unported.is_none());
    }
}

#[test]
fn stone_push_damages_before_selector_and_enchants_only_attacks() {
    let mut mgr = manager();
    mgr.run_ctx_mut().potions = vec![Some(PotionId::Fairyinabottle)];
    mgr.run_ctx_mut().hp = 30;
    mgr.run_ctx_mut().deck = vec![
        Card::from_factory(CardId::StrikeIronclad, false, 10).unwrap(),
        Card::from_factory(CardId::DefendIronclad, false, 11).unwrap(),
        Card::from_factory(CardId::StrikeIronclad, false, 12).unwrap(),
    ];
    enter_event(&mut mgr, EventId::StoneOfAllTime);
    mgr.initialize_stone_of_all_time().unwrap();
    mgr.take_event_choice("push").unwrap();
    assert_eq!(mgr.run_ctx().hp, 24);
    assert_eq!(mgr.current_event.as_ref().unwrap().rng.counter, 1);
    let choice = mgr.pending_run_choice.as_ref().unwrap();
    assert_eq!(choice.options.iter().map(|c| c.instance_id).collect::<Vec<_>>(), vec![10, 12]);
    assert_eq!((choice.min_choices, choice.max_choices, choice.allow_skip), (1, 1, false));
    mgr.resolve_run_choice(1).unwrap();
    assert!(mgr.run_ctx().deck[2].enchantments.contains(&(crate::identity::EnchantmentId::Vigorous, 8)));
    assert!(mgr.run_ctx().deck[0].enchantments.is_empty());
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
    assert!(mgr.current_event.is_none());
}

#[test]
fn stone_locks_push_without_attacks_and_auto_selects_one_attack() {
    let mut mgr = manager();
    mgr.run_ctx_mut().potions = vec![Some(PotionId::Fairyinabottle)];
    mgr.run_ctx_mut().deck = vec![Card::from_factory(CardId::DefendIronclad, false, 10).unwrap()];
    enter_event(&mut mgr, EventId::StoneOfAllTime);
    mgr.initialize_stone_of_all_time().unwrap();
    assert!(!mgr.current_event.as_ref().unwrap().options[1].enabled);
    assert!(mgr.take_event_choice("push").is_err());
    mgr.run_ctx_mut().deck = vec![Card::from_factory(CardId::StrikeIronclad, false, 11).unwrap()];
    enter_event(&mut mgr, EventId::StoneOfAllTime);
    mgr.initialize_stone_of_all_time().unwrap();
    mgr.take_event_choice("push").unwrap();
    assert!(mgr.pending_run_choice.is_none());
    assert!(mgr.run_ctx().deck[0].enchantments.contains(&(crate::identity::EnchantmentId::Vigorous, 8)));
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
}

#[test]
fn r7_recorded_spirit_grafter_seed_20261010001000_matches_let_it_in() {
    // Frozen r7 export_rules_100_after_6ca05c9.jsonl decisions 530-531:
    // SpiritGrafter.LetItIn heals 25 and appends METAMORPHOSIS iid 456.
    // C# SpiritGrafter CanonicalVars independently fixes those values at 25/10.
    let mut mgr = recorded_event_manager(
        20261010001000, 14258048995074467820, 9435, 9996, 14, 455, 185,
    );
    enter_event(&mut mgr, EventId::SpiritGrafter);
    mgr.take_event_choice("let_it_in").unwrap();
    let added = mgr.run_ctx().deck.last().unwrap();
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
    assert_eq!(mgr.run_ctx().hp, 9460);
    assert_eq!((added.id, added.instance_id), (CardId::Metamorphosis, 456));
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, 185);
}

#[test]
fn r7_recorded_infested_automaton_seed_20261010001007_matches_study() {
    // Frozen r7 decisions 416-417: rewards counter 165 -> 167 and the generated
    // Power is VICIOUS_CARD iid 467. C# InfestedAutomaton exposes Study/TouchCore.
    let mut mgr = recorded_event_manager(
        20261010001007, 4458986100734180549, 9536, 10006, 557, 466, 165,
    );
    enter_event(&mut mgr, EventId::InfestedAutomaton);
    mgr.take_event_choice("study").unwrap();
    let added = mgr.run_ctx().deck.last().unwrap();
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
    assert_eq!(added.id, CardId::from_name("VICIOUS_CARD").unwrap());
    assert_eq!(added.instance_id, 467);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, 167);
}

#[test]
fn r7_recorded_doll_room_seed_20261010001023_matches_random_doll() {
    // Frozen r7 decisions 392-393 choose Random and obtain BING_BONG with no
    // rewards-stream draw. C# DollRoom owns the same three fixed doll relics.
    let mut mgr = recorded_event_manager(
        20261010001023, 2326403575307794235, 9727, 9996, 202, 339, 163,
    );
    enter_event(&mut mgr, EventId::DollRoom);
    mgr.take_event_choice("random").unwrap();
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
    assert!(mgr.run_ctx().has_owned_relic(crate::combat::PLAYER, RelicId::from_name("BING_BONG").unwrap()));
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, 163);
}

#[test]
fn r7_recorded_field_holes_seed_20261010001006_matches_perfect_fit_selector() {
    // Frozen r7 decisions 348-350 expose 15 enchantable deck rows; slot 11 is
    // TREMBLE iid 172 and selecting it finishes the event.
    let mut mgr = recorded_event_manager(
        20261010001006, 17385156150572937636, 9809, 9999, 121, 253, 163,
    );
    let cards = [
        ("STRIKE_IRONCLAD", false, 1), ("STRIKE_IRONCLAD", false, 2),
        ("STRIKE_IRONCLAD", false, 3), ("STRIKE_IRONCLAD", false, 4),
        ("STRIKE_IRONCLAD", false, 5), ("DEFEND_IRONCLAD", false, 6),
        ("BASH", false, 10), ("ASCENDERS_BANE", false, 12),
        ("TAUNT", false, 56), ("THUNDERCLAP", false, 90),
        ("VICIOUS_CARD", false, 114), ("VICIOUS_CARD", false, 145),
        ("TREMBLE", false, 172), ("BRAND", false, 212),
        ("HEADBUTT", false, 232), ("FLAME_BARRIER_CARD", false, 253),
    ];
    mgr.run_ctx_mut().deck = cards.into_iter().map(|(name, upgraded, iid)| {
        Card::from_factory(CardId::from_name(name).unwrap(), upgraded, iid).unwrap()
    }).collect();
    enter_event(&mut mgr, EventId::FieldOfManSizedHoles);
    mgr.take_event_choice("enter").unwrap();
    let choices: Vec<_> = mgr.available_actions().into_iter()
        .filter(|action| action.kind == RunActionKind::Choose).collect();
    assert_eq!(choices.len(), 15);
    assert_eq!(choices[11].card_id, Some(CardId::Tremble));
    mgr.take_action(choices[11]).unwrap();
    let tremble = mgr.run_ctx().deck.iter().find(|card| card.instance_id == 172).unwrap();
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
    assert_eq!(tremble.enchantments.as_slice(), &[(crate::identity::EnchantmentId::PerfectFit, 1)]);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, 163);
}

#[test]
fn r7_recorded_crystal_sphere_seed_20261010001030_matches_pay_path() {
    // Frozen r7 decisions 536-540: CalculateVars rolls 53g, then the recorded
    // small/big/small probes reveal exactly one FirePotion and no rewards-RNG draw.
    // C# CrystalSphere fixes base cost 50, random [1,50), and 3/6 picks.
    let mut mgr = recorded_event_manager(
        20261010001030, 4498108525305116790, 10006, 10006, 359, 459, 180,
    );
    enter_event(&mut mgr, EventId::CrystalSphere);
    let pay = mgr.available_actions().into_iter()
        .find(|action| action.option_id == Some("pay")).unwrap();
    assert_eq!(pay.label_text().as_deref(), Some("Uncover Future (53g)"));
    mgr.take_action(pay).unwrap();
    for option in ["sphere_small_4_6", "sphere_big_2_7", "sphere_small_7_4"] {
        mgr.take_event_choice(option).unwrap();
    }
    let potions: Vec<_> = mgr.reward.items.iter().filter_map(|item| match item.kind {
        RewardItemKind::Potion(id) => Some(id), _ => None,
    }).collect();
    assert_eq!(mgr.phase(), RunPhase::CardReward);
    assert_eq!(mgr.run_ctx().gold, 306);
    assert_eq!(potions, vec![PotionId::from_name("FirePotion").unwrap()]);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, 180);
}

#[test]
fn r7_recorded_colorful_philosophers_seed_20261010001047_matches_rewards() {
    // Frozen r7 decisions 479-480 choose Necrobinder Philosopher. It consumes
    // 18 Rewards draws and creates the exact Common/Uncommon/Rare triples below.
    // C# ColorfulPhilosophers uses a fixed cross-character CardPoolColorOrder.
    let mut mgr = recorded_event_manager(
        20261010001047, 17950978534546532948, 9405, 10002, 93, 406, 209,
    );
    enter_event(&mut mgr, EventId::ColorfulPhilosophers);
    mgr.initialize_colorful_philosophers().unwrap();
    let labels: Vec<_> = mgr.available_actions().into_iter().map(|a| a.label.unwrap()).collect();
    assert_eq!(labels, ["Necrobinder Philosopher", "Regent Philosopher", "Silent Philosopher"]);
    mgr.take_event_choice("pool_1").unwrap();
    let actual: Vec<Vec<CardId>> = mgr.reward.items.iter().map(|item| match &item.kind {
        RewardItemKind::Card { cards, .. } => cards.iter().map(|card| card.id).collect(),
        _ => panic!("Colorful Philosophers must emit only card rewards"),
    }).collect();
    let ids = |names: &[&str]| names.iter().map(|name| CardId::from_name(name).unwrap()).collect::<Vec<_>>();
    assert_eq!(actual, vec![
        ids(&["SNAP", "NEGATIVE_PULSE", "DEFY"]),
        ids(&["SPUR", "RIGHT_HAND_HAND", "ENFEEBLING_TOUCH"]),
        ids(&["SENTRY_MODE", "THE_SCYTHE", "OBLIVION"]),
    ]);
    assert_eq!(mgr.phase(), RunPhase::CardReward);
    assert_eq!(mgr.run_ctx().next_card_id, 415);
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, 227);
}

#[test]
fn lantern_key_fight_follows_original_creation_order_without_entry_card_allocations() {
    // Original DLL: TheLanternKey.Fight creates SpecialCardReward's LanternKey
    // before EventCombatSynchronizer.EnterCombat enters CombatRoom. Preparation
    // only calls CombatState.AddPlayer; player deck copies occur at combat start.
    // The fixture's allocator is INPUT, not a claimed original-DLL numeric ID.
    let mut mgr = recorded_event_manager(
        20261010001001, 1916505679134300222, 9672, 9999, 295, 411, 173,
    );
    let allocator_before_entry=mgr.run_ctx().next_card_id;
    enter_event(&mut mgr, EventId::TheLanternKey);
    // This helper bypasses EventRoom.Enter: prepare its original combat layout
    // explicitly, before either button is chosen.
    mgr.prepare_event_combat(EncounterId::MysteriousKnightEventEncounter).unwrap();
    mgr.take_event_choice("keep_key").unwrap();
    assert_eq!(mgr.available_actions().len(), 1);
    assert_eq!(mgr.available_actions()[0].option_id, Some("fight"));
    mgr.take_event_choice("fight").unwrap();
    assert_eq!(mgr.phase(), RunPhase::Combat);
    assert_eq!(mgr.lantern_key_reward.as_ref().map(|card| (card.id, card.instance_id)),
        Some((CardId::LanternKey, allocator_before_entry+1)));
    assert!(mgr.combat().unwrap().cards.iter()
        .all(|card|card.instance_id>allocator_before_entry+1));
    let combat = mgr.combat().unwrap();
    assert!(combat.enemies.iter().any(|&enemy| combat.cr(enemy).monster_id == "MYSTERIOUS_KNIGHT"));
    assert_eq!(mgr.run_ctx().rngs.rewards.counter, 173);
}

fn item(kind: RewardItemKind) -> RewardItem {
    RewardItem { kind, consumed: false, skippable: true, skip_completes: false,
        add_injury_after: false, neows_bones_final: false }
}

#[test]
fn guilty_counts_each_completed_combat_once_and_leaves_after_five() {
    let mut mgr = manager();
    mgr.add_card_to_deck(CardId::Guilty).unwrap();
    for remaining in (0..5).rev() {
        let mut run = mgr.take_run_from_owner();
        run.room_type = RoomType::Monster;
        let mut combat = Combat::new(run, 1);
        combat.setup_encounter("BOWLBUGS_WEAK", &mut Rng::new(17));
        combat.start_combat();
        for enemy in combat.alive_enemies() {
            combat.kill_creature(enemy);
        }
        combat.check_combat_end();
        assert!(combat.is_over && combat.won);
        mgr.combat = Some(combat);
        mgr.phase = RunPhase::Combat;
        mgr.after_combat_action().unwrap();
        let guilty = mgr.run_ctx().deck.iter().find(|c| c.id == CardId::Guilty);
        if remaining == 0 {
            assert!(guilty.is_none());
        } else {
            assert_eq!(guilty.unwrap().var(VarKey::Combats), Some(remaining));
        }
    }
}

#[test]
fn merchant_and_ancient_rewards_use_the_explicit_owner_pool() {
    let mut mgr = manager();
    mgr.character = Character::Silent;
    mgr.enter_shop(MapPointType::Shop).unwrap();
    let silent = super::super::card_pools::cards(CardPool::Silent, false).unwrap();
    assert_eq!(mgr.shop.cards.len(), 5);
    assert!(mgr.shop.cards.iter().all(|entry| silent.contains(&entry.card.as_ref().unwrap().id)));
    mgr.phase = RunPhase::Event;
    mgr.neow[0] = NeowChoice { option_id: super::super::EventOptionIdentity::neow(0), relic: RelicId::HeftyTablet };
    mgr.resolve_neow_choice(mgr.neow[0].option_id).unwrap();
    let RewardItemKind::Card { cards, .. } = &mgr.reward.items[0].kind else {
        panic!("Hefty Tablet must offer cards");
    };
    assert_eq!(cards.len(), 3);
    assert!(cards.iter().all(|card| silent.contains(&card.id) && card.rarity == CardRarity::Rare));
}

#[test]
fn run_combat_binds_owner_pool_even_with_cross_color_deck() {
    let mut mgr = manager();
    mgr.run_ctx_mut().deck.clear();
    mgr.run_ctx_mut().deck.push(Card::from_factory(CardId::Metamorphosis, false, 100).unwrap());
    mgr.enter_combat(RoomType::Monster).unwrap();
    let combat = mgr.combat.as_mut().unwrap();
    assert_eq!(combat.players[crate::combat::PLAYER].character_pool, Some(crate::character::CardPool::Ironclad));
    let cards = combat.generate_owner_cards(crate::combat::PLAYER, CardType::Attack, 3, true);
    assert_eq!(cards.len(), 3);
    assert!(combat.fault.is_none());
}

#[test]
fn leaving_boss_rewards_transitions_directly_to_next_act() {
    let mut mgr = manager();
    mgr.run_ctx_mut().room_type = RoomType::Boss;
    mgr.phase = RunPhase::CardReward;
    mgr.awaiting_next_act = true;
    mgr.skip_card_reward().unwrap();
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
    assert_eq!(mgr.current_act, 1);
    assert!(!mgr.awaiting_next_act);
}

#[test]
fn winged_boots_cannot_travel_to_removed_points() {
    let mut mgr = RunManager::new(4100156, RunOptions::default());
    mgr.unported = None;
    mgr.phase = RunPhase::MapChoice;
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::WingedBoots));
    let map = mgr.acts[0].map.as_ref().unwrap();
    let row = map.points[mgr.current_point as usize].coord.row + 1;
    let removed = map.points.iter()
        .find(|p| p.coord.row == row && map.point(p.coord).is_none()).unwrap().coord;
    let actions = mgr.available_actions();
    assert!(actions.iter().all(|a| a.coord != Some(removed)));
    assert!(actions.iter().filter_map(|a| a.coord)
        .all(|coord| mgr.acts[0].map.as_ref().unwrap().point(coord).is_some()));
}

#[test]
fn eternal_cards_are_not_removable_transformable_or_amalgamator_candidates() {
    let mut mgr = manager();
    for card in &mut mgr.run_ctx_mut().deck {
        card.keywords |= KW_ETERNAL;
    }
    assert!(mgr.removable_deck_indices().is_empty());
    assert!(mgr.transformable_deck_indices().is_empty());
    assert!(!mgr.event_is_allowed(EventId::Amalgamator));
    assert!(!mgr.event_is_allowed(EventId::WoodCarvings));
}

#[test]
fn enchantments_require_fresh_eligible_cards_and_lock_empty_choices() {
    let mut mgr = manager();
    let mut card = Card::from_factory(CardId::StrikeIronclad, false, 1).unwrap();
    assert!(!RunManager::event_can_enchant(&card, "SoulsPower"));
    card.keywords |= KW_EXHAUST;
    assert!(RunManager::event_can_enchant(&card, "SoulsPower"));
    card.enchantments.push((crate::identity::EnchantmentId::SoulsPower, 1));
    assert!(!RunManager::event_can_enchant(&card, "SoulsPower"));
    card.enchantments.clear();
    card.x_cost = true;
    assert!(!RunManager::event_can_enchant(&card, "Slither"));
    mgr.run_ctx_mut().deck.clear();
    for (event, option) in [
        (crate::identity::EventId::GraveOfTheForgotten, "confront"),
        (crate::identity::EventId::Symbiote, "approach"), (crate::identity::EventId::WoodCarvings, "snake"),
    ] {
        enter_event(&mut mgr, event);
        assert!(!mgr.current_event.as_ref().unwrap().options.iter()
            .find(|o| o.option_id == option).unwrap().enabled);
    }
}

#[test]
fn grave_and_tea_eligibility_check_every_player() {
    let mut mgr = manager();
    mgr.run_ctx_mut().deck[0].keywords |= KW_EXHAUST;
    let run = mgr.take_run_from_owner();
    let mut combat = Combat::new(run, 1);
    let card = Card::from_factory(CardId::StrikeSilent, false, 50).unwrap();
    let other = combat.add_player(80, 11, 3, 0, &[card]);
    mgr.combat = Some(combat);
    assert!(!mgr.event_is_allowed(EventId::GraveOfTheForgotten));
    assert!(!mgr.event_is_allowed(EventId::RoundTeaParty));
    let combat = mgr.combat.as_mut().unwrap();
    combat.player_deck_mut(other)[0].keywords |= KW_EXHAUST;
    combat.cr_mut(other).hp = 12;
    assert!(mgr.event_is_allowed(EventId::GraveOfTheForgotten));
    assert!(mgr.event_is_allowed(EventId::RoundTeaParty));
}

#[test]
fn eggs_and_lens_modify_new_deck_cards_and_reward_options() {
    for (relic, id) in [
        (RelicId::MoltenEgg, CardId::StrikeIronclad),
        (RelicId::ToxicEgg, CardId::DefendIronclad),
        (RelicId::FrozenEgg, CardId::Inflame),
    ] {
        let mut mgr = manager();
        mgr.run_ctx_mut().relics.push(Relic::new(relic));
        mgr.add_card_to_deck(id).unwrap();
        assert!(mgr.run_ctx().deck.last().unwrap().upgraded);
        let mut cards = ArrayVec::new();
        cards.push(Card::from_factory(id, false, 100).unwrap());
        let next_id = mgr.run_ctx().next_card_id;
        mgr.apply_card_reward_relic_hooks(&mut cards, true);
        assert!(cards[0].upgraded);
        assert_eq!(cards[0].instance_id, 100);
        assert_eq!(mgr.run_ctx().next_card_id, next_id + 1);
    }
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::FresnelLens));
    mgr.add_card_to_deck(CardId::DefendIronclad).unwrap();
    assert!(mgr.run_ctx().deck.last().unwrap().enchantments.contains(&(crate::identity::EnchantmentId::Nimble, 2)));
    let next_id = mgr.run_ctx().next_card_id;
    let mut card = Card::from_factory(CardId::DefendIronclad, false, 100).unwrap();
    mgr.modify_card_for_relic(&mut card, RelicId::FresnelLens).unwrap();
    assert_eq!(card.instance_id, 100);
    assert_eq!(mgr.run_ctx().next_card_id, next_id);
    mgr.add_card_to_deck(CardId::StrikeIronclad).unwrap();
    assert!(mgr.run_ctx().deck.last().unwrap().enchantments.is_empty());
}

#[test]
fn hm_teammate_pickups_refuse_before_mutating_local_run_and_egg_is_source_owned() {
    for owner in 1..4 {
        for id in [RelicId::Kifuda, RelicId::LeadPaperweight, RelicId::LeesWaffle,
            RelicId::JewelryBox, RelicId::LostCoffer, RelicId::Kaleidoscope,
            RelicId::LargeCapsule, RelicId::Mango, RelicId::MassiveScroll] {
            let mut mgr = manager();
            let mut r = Relic::new(id); r.owner = owner;
            mgr.run_ctx_mut().relics.push(r);
            let before = (mgr.run_ctx().hp, mgr.run_ctx().max_hp,
                mgr.run_ctx().deck.len(), mgr.run_ctx().next_card_id, mgr.run_ctx().relics.len());
            mgr.neow[0].relic = id;
            let mut action = RunAction::new(RunActionKind::EventChoice);
            action.player_id = Some(i32::from(owner) + 1);
            action.option_id = Some(mgr.neow[0].option_id.legacy_id());
            assert_eq!(mgr.take_action(action), Err("owner pickup needs MP run layer"));
            assert_eq!(before, (mgr.run_ctx().hp, mgr.run_ctx().max_hp,
                mgr.run_ctx().deck.len(), mgr.run_ctx().next_card_id, mgr.run_ctx().relics.len()));
        }
        let mut mgr = manager();
        let mut r = Relic::new(RelicId::MoltenEgg); r.owner = owner;
        mgr.run_ctx_mut().relics.push(r);
        mgr.add_card_to_deck(CardId::StrikeIronclad).unwrap();
        assert!(!mgr.run_ctx().deck.last().unwrap().upgraded);
        let mut cards = ArrayVec::new();
        cards.push(Card::from_factory(CardId::StrikeIronclad, false, 100).unwrap());
        mgr.apply_card_reward_relic_hooks(&mut cards, true);
        assert!(!cards[0].upgraded);
        assert!(!RELIC_INFO[RelicId::MoltenEgg.index()].upon_pickup);
    }
}

#[test]
fn lead_paperweight_is_an_awaited_optional_card_grid_not_a_reward_set() {
    for selected in [vec![],vec![1]] {
        let mut mgr=manager();
        let before=mgr.run_ctx().deck.len();
        mgr.offer_lead_paperweight_choice(2).unwrap();
        assert!(mgr.reward.items.is_empty());
        let choice=mgr.pending_run_choice.take().unwrap();
        assert_eq!(choice.wire_source,Some("relic:LEAD_PAPERWEIGHT"));
        assert_eq!((choice.min_choices,choice.max_choices),(0,1));
        assert!(choice.allow_skip);
        assert_eq!(choice.reward_cards.len(),2);
        let expected=selected.first().map(|&i|choice.reward_cards[i].id);
        mgr.apply_run_choice(choice,&selected).unwrap();
        assert_eq!(mgr.run_ctx().deck.len(),before+selected.len());
        if let Some(id)=expected {assert_eq!(mgr.run_ctx().deck.last().unwrap().id,id);}
    }
}

#[test]
fn solo_character_starts_use_original_character_profiles_and_floor_provenance() {
    use super::super::mp::Character;
    for (character,hp,count,relic,orbs) in [
        (Character::Ironclad,80,10,RelicId::BurningBlood,0),
        (Character::Silent,70,12,RelicId::RingOfTheSnake,0),
        (Character::Defect,75,10,RelicId::CrackedCore,3),
        (Character::Regent,75,10,RelicId::DivineRight,0),
        (Character::Necrobinder,66,10,RelicId::BoundPhylactery,0),
    ] {
        let mgr=RunManager::new_for_character(20261008010000,
            RunOptions {ascension:0,..Default::default()},character);
        let run=mgr.run_ctx();
        assert_eq!(mgr.character,character);
        assert_eq!((run.hp,run.max_hp),(hp,hp));
        assert_eq!(run.deck.len(),count);
        assert!(run.deck.iter().all(|card|card.floor_added_to_deck==Some(1)));
        assert_eq!(run.relics[0].id,relic);
        assert_eq!(run.relics[0].floor_added_to_deck,1);
        assert_eq!(run.base_orb_slots,orbs);
    }
}

#[test]
fn upgrade_template_allocates_one_warm_or_two_cold_factory_ids() {
    let mut mgr = manager();
    for expected in [2, 1] {
        let mut card = Card::from_factory(CardId::Inflame, false, 100).unwrap();
        let before = mgr.run_ctx().next_card_id;
        shop::upgrade_template(mgr.run_ctx_mut(), &mut card);
        assert!(card.upgraded);
        assert_eq!(card.instance_id, 100);
        assert_eq!(mgr.run_ctx().next_card_id, before + expected);
    }
}

#[test]
fn smith_and_reward_upgrade_share_the_unupgraded_reference_key_in_both_orders() {
    fn upgrade_reward(mgr: &mut RunManager, reward: &mut Card, relic: RelicId) {
        if relic == RelicId::SilverCrucible {
            let mut options = ArrayVec::new();
            options.push(reward.clone());
            mgr.apply_card_reward_relic_hooks(&mut options, true);
            *reward = options.remove(0);
        } else {
            mgr.modify_card_for_relic(reward, relic).unwrap();
        }
    }
    for relic in [RelicId::FrozenEgg, RelicId::SilverCrucible] {
    for reward_first in [false, true] {
        let mut mgr = manager();
        mgr.run_ctx_mut().relics.push(Relic::new(relic));
        let id = CardId::Inflame;
        mgr.run_ctx_mut().deck.push(Card::from_factory(id, false, 100).unwrap());
        let index = mgr.run_ctx().deck.len() - 1;
        let before = mgr.run_ctx().next_card_id;
        let mut reward = Card::from_factory(id, false, 101).unwrap();
        if reward_first {
            upgrade_reward(&mut mgr, &mut reward, relic);
            assert_eq!(mgr.run_ctx().next_card_id - before, 2);
            mgr.run_ctx_mut().upgrade_deck_card(index);
        } else {
            mgr.run_ctx_mut().upgrade_deck_card(index);
            assert_eq!(mgr.run_ctx().next_card_id - before, 2);
            upgrade_reward(&mut mgr, &mut reward, relic);
        }
        assert!(reward.upgraded && mgr.run_ctx().deck[index].upgraded);
        assert_eq!(mgr.run_ctx().next_card_id - before, 3);
    }
    }
}

#[test]
fn silver_crucible_attempts_nonupgradable_options_without_changing_gameplay() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::SilverCrucible));
    let mut options = ArrayVec::new();
    options.push(Card::from_factory(CardId::Guilty, false, 100).unwrap());
    for expected in [2, 1] {
        let before = mgr.run_ctx().next_card_id;
        mgr.apply_card_reward_relic_hooks(&mut options, true);
        assert_eq!(mgr.run_ctx().next_card_id - before, expected);
        assert!(!options[0].upgraded);
        assert_eq!(options[0].instance_id, 100);
    }
}

#[test]
fn extended_combat_without_multiplayer_run_roster_fails_closed() {
    let mut mgr = manager();
    let mut combat = Combat::new(mgr.take_run_from_owner(), 1);
    combat.add_player(80, 80, 3, 0, &[]);
    mgr.combat = Some(combat);
    mgr.after_combat_action().unwrap();
    assert_eq!(mgr.unported, Some("multiplayer run construction is unported"));
}

#[test]
fn shop_uses_shop_rare_odds_at_both_ascensions() {
    let mut mgr = manager();
    for (ascension, rare) in [(0, 0.09), (7, 0.045)] {
        mgr.run_ctx_mut().ascension = ascension;
        for seed in 0..256 {
            let mut oracle = Rng::new(seed);
            let roll = oracle.next_float(1.0);
            mgr.run_ctx_mut().rngs.rewards = Rng::new(seed);
            let expected = if roll < rare { CardRarity::Rare }
                else if roll < rare + 0.37 { CardRarity::Uncommon }
                else { CardRarity::Common };
            assert_eq!(shop::shop_card_rarity(mgr.run_ctx_mut(), 0.0), expected);
        }
    }
}

#[test]
fn silver_crucible_upgrades_only_the_first_three_card_rewards() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::SilverCrucible));
    for reward in 0..5 {
        let mut cards = ArrayVec::new();
        cards.push(Card::from_factory(CardId::StrikeIronclad, false, 100).unwrap());
        cards.push(Card::from_factory(CardId::Inflame, true, 101).unwrap());
        mgr.apply_card_reward_relic_hooks(&mut cards, false);
        assert!(!cards[0].upgraded);
        mgr.apply_card_reward_relic_hooks(&mut cards, true);
        assert_eq!(cards[0].upgraded, reward < 3);
        assert!(cards[1].upgraded);
        assert_eq!(cards[0].instance_id, 100);
        assert!(mgr.unported.is_none());
        assert_eq!(mgr.run_ctx().relics.last().unwrap().counter, (reward + 1).min(3));
    }
}

#[test]
fn wing_charm_enchants_one_reward_option_on_shared_niche_rng() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::WingCharm));
    let mut cards = ArrayVec::new();
    cards.push(Card::from_factory(CardId::StrikeIronclad, false, 100).unwrap());
    let mut expected=mgr.run_ctx().rngs.niche.clone();expected.choice_index(1);
    mgr.apply_card_reward_relic_hooks(&mut cards, true);
    assert_eq!(mgr.unported,None);
    assert_eq!(cards[0].enchantments.as_slice(),&[(crate::identity::EnchantmentId::Swift,1)]);
    assert_eq!(format!("{:?}",mgr.run_ctx().rngs.niche),format!("{expected:?}"));
}

#[test]
fn silken_tress_does_not_modify_ancient_card_grids() {
    let mut mgr = manager();
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::SilkenTress));
    let mut cards = ArrayVec::new();
    cards.push(Card::from_factory(CardId::StrikeIronclad, false, 100).unwrap());
    mgr.apply_card_reward_relic_hooks(&mut cards, false);
    assert!(cards[0].enchantments.is_empty());
    assert!(mgr.run_ctx().relics.iter().find(|r| r.id == RelicId::SilkenTress).unwrap().enabled);
    let next_id = mgr.run_ctx().next_card_id;
    mgr.apply_card_reward_relic_hooks(&mut cards, true);
    assert!(cards[0].enchantments.contains(&(crate::identity::EnchantmentId::Glam, 1)));
    assert_eq!(cards[0].instance_id, next_id + 1);
    assert_eq!(mgr.run_ctx().next_card_id, next_id + 1);
    assert!(!mgr.run_ctx().relics.iter().find(|r| r.id == RelicId::SilkenTress).unwrap().enabled);
}

#[test]
fn reward_modifiers_follow_relic_inventory_order() {
    for lens_first in [false, true] {
        let mut mgr = manager();
        let (first, second) = if lens_first {
            (RelicId::FresnelLens, RelicId::SilkenTress)
        } else {
            (RelicId::SilkenTress, RelicId::FresnelLens)
        };
        mgr.run_ctx_mut().relics.extend([Relic::new(first), Relic::new(second)]);
        let mut cards = ArrayVec::new();
        cards.push(Card::from_factory(CardId::DefendIronclad, false, 100).unwrap());
        mgr.apply_card_reward_relic_hooks(&mut cards, true);
        let expected = if lens_first { (EnchantmentId::Nimble, 2) } else { (EnchantmentId::Glam, 1) };
        assert_eq!(cards[0].enchantments.as_slice(), &[expected]);
    }
}

#[test]
fn rare_noncombat_rewards_consume_the_roll_but_get_no_act_upgrade_scaling() {
    for seed in 0..32 {
        let mut mgr = manager();
        mgr.current_act = 2;
        mgr.run_ctx_mut().rngs.rewards = Rng::new(seed);
        let count = super::super::card_pools::cards(CardPool::Ironclad, false).unwrap().into_iter()
            .filter(|id| card_info(*id, false).unwrap().rarity == CardRarity::Rare).count();
        let mut oracle = Rng::new(seed);
        oracle.choice_index(count);
        let upgraded = oracle.next_float(1.0) <= 0.0;
        let card = mgr.generate_noncombat_card(CardPool::Ironclad, CardRarity::Rare, &[], true).unwrap();
        assert_eq!(card.upgraded, upgraded);
        assert_eq!(mgr.run_ctx().rngs.rewards.counter, oracle.counter);
    }
}

#[test]
fn reward_clicks_advance_and_do_not_reopen_closed_card_screens() {
    for potion in [true, false] {
        for modal in [true, false] {
            let mut mgr = manager();
            mgr.phase = RunPhase::CardReward;
            let relic = RewardItemKind::Relic(RelicId::Strawberry);
            let flask = RewardItemKind::Potion(PotionId::Firepotion);
            let (first, second) = if potion { (flask, relic) } else { (relic, flask) };
            mgr.reward.items.push(item(first));
            mgr.reward.items.push(item(second));
            mgr.reward.active = modal.then_some(0);
            let displayed = mgr.ordered_remaining_reward_indices().iter()
                .position(|index| *index == 0).unwrap();
            let index = (!modal).then_some(displayed);
            if potion { mgr.pick_potion(index).unwrap(); } else { mgr.pick_relic_reward(index).unwrap(); }
            assert!(mgr.reward.items[0].consumed);
            assert!(!mgr.reward.items[1].consumed);
            assert_eq!(mgr.reward.active, Some(1));
            mgr.reward.items[1] = item(RewardItemKind::Card {
                cards: ArrayVec::new(), closed: true, reroll: None,
                progress: Default::default(),
            });
            mgr.reward.active = None;
            mgr.advance_rewards();
            assert_eq!(mgr.reward.active, None);
            assert_eq!(mgr.phase(), RunPhase::CardReward);
        }
    }
}

#[test]
fn unrest_kill_obtains_directly_and_obtained_relics_leave_all_bags() {
    let mut mgr = manager();
    for bag in mgr.relic_bags.player[0].iter_mut().chain(mgr.relic_bags.shared.iter_mut()) {
        *bag = vec![RelicId::Strawberry];
    }
    let before = mgr.run_ctx().max_hp;
    enter_event(&mut mgr, EventId::UnrestSite);
    mgr.take_event_choice("kill").unwrap();
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
    assert!(mgr.reward.items.is_empty());
    assert_eq!(mgr.run_ctx().max_hp, before - 8 + 7);
    assert!(mgr.run_ctx().relics.iter().any(|r| r.id == RelicId::Strawberry));
    assert!(mgr.relic_bags.player[0].iter().chain(mgr.relic_bags.shared.iter())
        .all(|bag| !bag.contains(&RelicId::Strawberry)));
}

#[test]
fn rest_dig_obtains_relic_and_its_effect_before_finishing_the_option() {
    // DigRestSiteOption.OnSelect awaits RelicCmd.Obtain, not a skippable reward.
    for tent in [false, true] {
        let mut mgr = manager();
        mgr.run_ctx_mut().total_floor = 8;
        mgr.phase = RunPhase::RestSite;
        mgr.run_ctx_mut().relics.push(Relic::new(RelicId::Shovel));
        if tent {
            mgr.run_ctx_mut().relics.push(Relic::new(RelicId::MiniatureTent));
        }
        for bag in mgr.relic_bags.player[0].iter_mut().chain(mgr.relic_bags.shared.iter_mut()) {
            *bag = vec![RelicId::Strawberry];
        }
        let mut rest = rest::RestState::default();
        rest::generate(&mut rest, mgr.run_ctx_mut());
        mgr.rest = rest;
        let index = rest::option_index(&mgr.rest, rest::RestOptionKind::Dig).unwrap();
        let before = mgr.run_ctx().max_hp;
        mgr.take_action(mgr.native_rest_action(index).unwrap()).unwrap();
        assert!(mgr.run_ctx().relics.iter().any(|r| r.id == RelicId::Strawberry));
        assert_eq!(mgr.run_ctx().max_hp, before + 7);
        assert_eq!(mgr.phase(), if tent { RunPhase::RestSite } else { RunPhase::MapChoice });
    }
}

#[test]
fn fatal_event_damage_stops_pickups_upgrades_and_random_draws() {
    let mut mgr = manager();
    mgr.run_ctx_mut().hp = 14;
    let relics = mgr.run_ctx().relics.len();
    enter_event(&mut mgr, EventId::RoomFullOfCheese);
    mgr.take_event_choice("search").unwrap();
    assert_eq!(mgr.phase(), RunPhase::RunOver);
    assert_eq!(mgr.run_ctx().relics.len(), relics);
    let mut mgr = manager();
    mgr.run_ctx_mut().hp = 8;
    let relics = mgr.run_ctx().relics.len();
    enter_event(&mut mgr, EventId::TrashHeap);
    mgr.take_event_choice("dive_in").unwrap();
    assert_eq!(mgr.phase(), RunPhase::RunOver);
    assert_eq!(mgr.run_ctx().relics.len(), relics);
    let mut mgr = manager();
    mgr.run_ctx_mut().hp = 15;
    let before = mgr.run_ctx().rngs.niche.counter;
    let identity = mgr.run_ctx().next_card_id;
    enter_event(&mut mgr, EventId::HungryForMushrooms);
    mgr.take_event_choice("fragrant").unwrap();
    assert_eq!(mgr.phase(), RunPhase::RunOver);
    assert_eq!(mgr.run_ctx().rngs.niche.counter, before);
    assert_eq!(mgr.run_ctx().next_card_id, identity);
    assert!(mgr.run_ctx().deck.iter().all(|card| !card.upgraded));
}

#[test]
fn event_damage_applies_tungsten_rod_before_fatal_guard() {
    let mut mgr = manager();
    mgr.run_ctx_mut().hp = 14;
    mgr.run_ctx_mut().relics.push(Relic::new(RelicId::TungstenRod));
    enter_event(&mut mgr, EventId::RoomFullOfCheese);
    mgr.take_event_choice("search").unwrap();
    assert_eq!(mgr.run_ctx().hp, 1);
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
    assert!(mgr.run_ctx().relics.iter().any(|r| r.id == RelicId::ChosenCheese));
    // The shared CreatureCmd.Damage path also covers ordinary event effects.
    enter_event(&mut mgr, EventId::ThisOrThat);
    mgr.run_ctx_mut().hp = 6;
    mgr.take_event_choice("plain").unwrap();
    assert_eq!(mgr.run_ctx().hp, 1);
}

#[test]
fn mushroom_pickup_hooks_keep_their_nonfatal_effects() {
    let mut mgr = manager();
    let before = mgr.run_ctx().max_hp;
    enter_event(&mut mgr, EventId::HungryForMushrooms);
    mgr.take_event_choice("big").unwrap();
    assert_eq!(mgr.run_ctx().max_hp, before + 20);
    let mut mgr = manager();
    let hp = mgr.run_ctx().hp;
    enter_event(&mut mgr, EventId::HungryForMushrooms);
    mgr.take_event_choice("fragrant").unwrap();
    assert_eq!(mgr.run_ctx().hp, hp - 15);
    assert_eq!(mgr.run_ctx().deck.iter().filter(|card| card.upgraded).count(), 2);
}

#[test]
fn precarious_shears_with_no_candidates_can_end_the_run() {
    let mut mgr = manager();
    mgr.run_ctx_mut().deck.clear();
    mgr.run_ctx_mut().hp = 16;
    mgr.neow[0].relic = RelicId::PrecariousShears;
    mgr.resolve_neow_choice(mgr.neow[0].option_id).unwrap();
    assert_eq!(mgr.phase(), RunPhase::RunOver);
    assert_eq!(mgr.run_ctx().hp, 0);
}

#[test]
fn boss_card_reward_resets_future_rarity_odds() {
    // C# Odds/CardRarityOdds.cs Roll resets after every Rare, including Boss.
    for ascension in [0, 7, 10] {
        let mut mgr = manager();
        mgr.options.ascension = ascension;
        mgr.card_rarity_offset = 0.40;
        let before = mgr.run_ctx().rngs.rewards.counter;
        for _ in 0..3 {
            assert_eq!(mgr.roll_card_rarity(RoomType::Boss), CardRarity::Rare);
            assert_eq!(mgr.card_rarity_offset, -0.05);
        }
        assert_eq!(mgr.run_ctx().rngs.rewards.counter, before + 3);
        let mut fresh = manager();
        fresh.options.ascension = ascension;
        fresh.run_ctx_mut().rngs.rewards = mgr.run_ctx().rngs.rewards.clone();
        assert_eq!(mgr.roll_card_rarity(RoomType::Monster), fresh.roll_card_rarity(RoomType::Monster));
        assert_eq!(mgr.card_rarity_offset, fresh.card_rarity_offset);
    }
}

#[test]
fn large_capsule_clears_the_neow_marker_before_its_pickup_chain() {
    let mut mgr = RunManager::new(4100115, RunOptions::default());
    assert_eq!(mgr.unported, Some("EVENT:Neow"));
    mgr.neow[0].relic = RelicId::LargeCapsule;
    for bag in mgr.relic_bags.player[0].iter_mut().chain(mgr.relic_bags.shared.iter_mut()) {
        *bag = vec![RelicId::Strawberry];
    }
    let deck = mgr.run_ctx().deck.len();
    mgr.resolve_neow_choice(mgr.neow[0].option_id).unwrap();
    assert_eq!(mgr.unported, None);
    assert_eq!(mgr.phase(), RunPhase::MapChoice);
    assert_eq!(mgr.run_ctx().deck.len(), deck + 2);
    assert!(mgr.run_ctx().relics.iter().any(|r| r.id == RelicId::Strawberry));
    assert!(mgr.run_ctx().relics.iter().any(|r| r.id == RelicId::Circlet));
}

#[test]
fn event_random_upgrades_use_dotnet_tie_order_before_shuffling() {
    let mut mgr = manager();
    mgr.run_ctx_mut().deck = (0..20).map(|i|
        Card::from_factory(CardId::StrikeIronclad, false, 100 + i).unwrap()).collect();
    let mut expected: Vec<_> = (0..20).map(|i| i as crate::card::KRef).collect();
    crate::flow::dotnet_sort_cards(&mut expected, &mgr.run_ctx().deck);
    Rng::new(42).shuffle(&mut expected);
    let mut selected: Vec<_> = expected[..2].iter().map(|i| mgr.run_ctx().deck[*i as usize].instance_id).collect();
    selected.sort_unstable();
    mgr.upgrade_random_event_cards(&mut Rng::new(42), 2);
    let mut upgraded: Vec<_> = mgr.run_ctx().deck.iter().filter(|c| c.upgraded).map(|c| c.instance_id).collect();
    upgraded.sort_unstable();
    assert_eq!(upgraded, selected);
}
