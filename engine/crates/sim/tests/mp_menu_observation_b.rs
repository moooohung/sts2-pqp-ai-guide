use sts2_sim::{obs::{*, mp_menu::*}, run::mp::{Character, MpPhase, MpRun, PlayerRewards, PlayerSpec},
    card::Card, tables::CardId};

fn with_buffers(f: impl FnOnce(&mut ObsBuffers<'_>)) {
    macro_rules! v { ($name:ident, $ty:ty, $len:expr) => { let mut $name = vec![<$ty>::default(); $len]; }; }
    v!(sk, u16, STATE_CAPACITY); v!(st, u16, STATE_CAPACITY * TOKEN_SLOTS);
    v!(sn, i32, STATE_CAPACITY * SCALAR_SLOTS); v!(ss, f32, STATE_CAPACITY * SCALAR_SLOTS);
    v!(sb, f32, STATE_CAPACITY * BOOLEAN_FEATURES); v!(sm, i8, STATE_CAPACITY);
    v!(ck, u16, CANDIDATE_CAPACITY); v!(ct, u16, CANDIDATE_CAPACITY * TOKEN_SLOTS);
    v!(cn, i32, CANDIDATE_CAPACITY * SCALAR_SLOTS); v!(cs, f32, CANDIDATE_CAPACITY * SCALAR_SLOTS);
    v!(cb, f32, CANDIDATE_CAPACITY * BOOLEAN_FEATURES); v!(cp, u16, CANDIDATE_CAPACITY);
    v!(mm, i8, CANDIDATE_CAPACITY); v!(sa, i8, CANDIDATE_CAPACITY); v!(ta, i8, CANDIDATE_CAPACITY);
    v!(sx, i8, CANDIDATE_CAPACITY); v!(tx, i8, CANDIDATE_CAPACITY);
    v!(sl, u8, SOURCE_LINK_BYTES); v!(tl, u8, SOURCE_LINK_BYTES);
    v!(legal, bool, CANDIDATE_CAPACITY); v!(boss, f32, BOSS_FEATURES);
    let mut b = ObsBuffers::new(&mut sk, &mut st, &mut sn, &mut ss, &mut sb, &mut sm,
        &mut ck, &mut ct, &mut cn, &mut cs, &mut cb, &mut cp, &mut mm, &mut sa, &mut ta,
        &mut sx, &mut tx, &mut sl, &mut tl, &mut legal, &mut boss).unwrap();
    f(&mut b);
}

#[test]
fn special_card_is_not_production_admitted_and_proof_retains_owner_identity() {
    let specs=[PlayerSpec{net_id:1,character:Character::Ironclad},
        PlayerSpec{net_id:2,character:Character::Silent}];
    let mut run=MpRun::new("73001",&specs,10).unwrap();
    run.phase=MpPhase::Rewards;
    let card=run.roster.player(1).unwrap().deck[0].clone();
    run.rewards=Some((0..2).map(|seat|PlayerRewards{
        gold:vec![12],potion:None,cards:Vec::new(),extra_cards:Vec::new(),card_removals:Vec::new(),extra_relics:Vec::new(),
        extra_potions:Vec::new(),special_cards:if seat==1 {vec![Some(card.clone())]}else{Vec::new()},
        gold_taken:false,potion_taken:false,card_taken:false,completed:false,
    }).collect());
    let context=CombatObservationContext{public_act1_boss:[0.;BOSS_FEATURES],choice_prompt:""};
    let mut bindings=Vec::new();
    let before=format!("{run:?}");
    with_buffers(|buffers|assert!(write_mp_menu_perspective(
        &run,1,&context,buffers,&mut bindings).is_err()));
    #[cfg(feature="selector-proof")]
    with_buffers(|buffers|{
        write_mp_special_reward_proof(&run,1,&context,buffers,&mut bindings).unwrap();
        let slot=bindings.iter().position(|b|b.action==MpMenuAction::PickSpecialCard(0)).unwrap();
        assert_eq!(bindings[slot].actor,1);
        assert_eq!(bindings[slot].net_id,2);
        assert_eq!(buffers.candidate_token_ids[slot*TOKEN_SLOTS],
            frozen_token_id("action_kind","pick_special_card").unwrap());
        assert_eq!(buffers.candidate_token_ids[slot*TOKEN_SLOTS+1],
            frozen_token_id("card_id",card.id.name()).unwrap());
        assert_eq!(buffers.model_mask[slot],1);
    });
    assert_eq!(format!("{run:?}"),before);
}

#[test]
fn cheese_grid_and_merchant_navigation_are_owned_read_only_and_driver_bound() {
    let context = CombatObservationContext { public_act1_boss: [0.0; BOSS_FEATURES], choice_prompt: "" };
    for seats in 2..=4 {
        let specs: Vec<_> = (0..seats).map(|seat| PlayerSpec {
            net_id: 100 + seat as u64, character: Character::Ironclad,
        }).collect();
        for owner in 0..seats {
            let mut run = MpRun::new("73001", &specs, 0).unwrap();
            run.phase = MpPhase::Event;
            run.world.current_room = Some("ROOM_FULL_OF_CHEESE");
            let mut bindings = Vec::new();
            with_buffers(|b| {
                write_mp_menu_perspective(&run, owner as u8, &context, b, &mut bindings).unwrap();
                assert_eq!(bindings[0].action, MpMenuAction::CheeseGorge);
            });
            assert!(run.choose_cheese(999, owner, 0).is_err());
            run.choose_cheese(specs[owner].net_id, owner, 0).unwrap();
            let snapshot = format!("{run:?}");
            for selected in [&[][..], &[0][..], &[0, 1][..]] {
                with_buffers(|b| {
                    write_mp_menu_with_selection(&run, owner as u8, selected, &context, b, &mut bindings).unwrap();
                    assert!(bindings.iter().all(|a| a.actor as usize == owner && a.net_id == specs[owner].net_id));
                    assert_eq!(b.legal_mask.iter().filter(|&&v| v).count(), match selected.len() { 0 => 8, 1 => 7, _ => 1 });
                    for (slot, action) in bindings.iter().enumerate() {
                        if let MpMenuAction::ChooseCheeseCard(index) = action.action {
                            assert_eq!(b.candidate_numeric_values[slot * SCALAR_SLOTS + 5], selected.len() as i32);
                            let links = &b.source_links_bits[slot * ROUTE_BYTES_PER_CANDIDATE..(slot + 1) * ROUTE_BYTES_PER_CANDIDATE];
                            let row = (0..b.state_rows()).find(|&r| links[r / 8] & (1 << (r % 8)) != 0).unwrap();
                            assert_eq!(b.state_token_ids[row * TOKEN_SLOTS],
                                frozen_token_id("card_id", run.cheese_cards[owner].as_ref().unwrap()[index].id.name()).unwrap());
                            assert_eq!(b.state_token_ids[row * TOKEN_SLOTS + 3], 0);
                        }
                    }
                });
            }
            assert_eq!(snapshot, format!("{run:?}"));
            assert_eq!(bindings.last().unwrap().action, MpMenuAction::ConfirmCheese([0, 1]));
            let before: Vec<_> = run.roster.players().iter().map(|p| p.deck.len()).collect();
            assert!(run.select_cheese_cards(999, owner, [0, 1]).is_err());
            run.select_cheese_cards(specs[owner].net_id, owner, [0, 1]).unwrap();
            for (seat, p) in run.roster.players().iter().enumerate() {
                assert_eq!(p.deck.len(), before[seat] + if seat == owner { 2 } else { 0 });
            }
            with_buffers(|b| assert!(write_mp_menu_perspective(&run, owner as u8, &context, b, &mut bindings).is_err()));
            run.phase = MpPhase::Shop;
            // A fabricated Shop phase without generated inventory must refuse.
            with_buffers(|b| assert!(write_mp_menu_perspective(&run, owner as u8, &context, b, &mut bindings).is_err()));
            assert!(run.leave_shop(999, owner).is_err());
            run.leave_shop(specs[owner].net_id, owner).unwrap();
            assert_eq!(run.phase, MpPhase::Map);
        }
    }
}

#[test]
fn every_seat_has_owned_neow_and_retained_card_reward_bindings() {
    let context = CombatObservationContext { public_act1_boss: [0.0; BOSS_FEATURES], choice_prompt: "" };
    for seats in 2..=4 {
        let specs: Vec<_> = (0..seats).map(|seat| PlayerSpec {
            net_id: 100 + seat as u64, character: Character::Ironclad,
        }).collect();
        let mut run = MpRun::new("73001", &specs, 0).unwrap();
        let mut bindings = Vec::new();
        for actor in 0..seats as u8 {
            with_buffers(|b| {
                write_mp_menu_perspective(&run, actor, &context, b, &mut bindings).unwrap();
                assert_eq!(bindings.len(), 3);
                assert!(bindings.iter().all(|a| a.actor == actor && a.net_id == 100 + actor as u64));
                assert_eq!(b.legal_mask.iter().filter(|&&x| x).count(), 3);
            });
        }
        run.phase = MpPhase::Rewards;
        run.rewards = Some((0..seats).map(|actor| {
            let mut card = Card::from_factory(CardId::StrikeIronclad, false, 900 + actor as u32).unwrap();
            card.owner = actor as u8;
            PlayerRewards { gold: vec![12], potion: None, cards: vec![card],
                extra_cards: Vec::new(), card_removals: Vec::new(),
                extra_relics: Vec::new(), extra_potions: Vec::new(), special_cards: Vec::new(),
                gold_taken: false, potion_taken: false, card_taken: false, completed: false }
        }).collect());
        for actor in 0..seats as u8 {
            run.roster.player_mut(actor as usize).unwrap().gold = 200 + actor as i32;
            let before = format!("{run:?}");
            with_buffers(|b| {
                write_mp_menu_perspective(&run, actor, &context, b, &mut bindings).unwrap();
                assert_eq!(b.state_numeric_values[6], 200 + actor as i32);
                assert_eq!(b.legal_mask.iter().filter(|&&x| x).count(), 2);
                assert_eq!(bindings[0].action, MpMenuAction::PickCard(0));
                assert_eq!(bindings[1].action, MpMenuAction::TakeGoldSkipRemaining);
            });
            assert_eq!(format!("{run:?}"), before, "observation is read-only");
            assert_eq!(mp_menu_decision_owner(&run, None), Some(actor));
            let card = bindings[0];
            let mut probe = run.clone();
            assert!(probe.take_card_reward(999, actor as usize, 0).is_err());
            probe.take_card_reward(card.net_id, card.actor as usize, 0).unwrap();
            assert!(probe.rewards.as_ref().unwrap()[actor as usize].card_taken);
            run.take_gold_skip_remaining_rewards(100 + actor as u64, actor as usize).unwrap();
        }
        assert_eq!(mp_menu_decision_owner(&run, None), None);
        with_buffers(|b| assert!(write_mp_menu_perspective(&run, 0, &context, b, &mut bindings).is_err()));
    }
}

#[test]
fn merchant_candidates_bind_only_the_selected_seats_inventory_and_prices() {
    use sts2_sim::{run::{Coord, mp::MerchantAction}, tables::MapPointType};
    let context = CombatObservationContext { public_act1_boss: [0.0; BOSS_FEATURES], choice_prompt: "" };
    for seats in 2..=4 {
        let specs: Vec<_> = (0..seats).map(|s| PlayerSpec { net_id: 100 + s as u64, character: Character::Ironclad }).collect();
        let mut run = MpRun::new("73001", &specs, 0).unwrap();
        run.phase = MpPhase::Map;
        let (col, row) = run.world.run.coord.unwrap();
        let index = run.world.map.point(Coord::new(col, row)).unwrap().children[0] as usize;
        // Exercise the public map-vote -> shop-entry dispatch in a controlled
        // world; initialization/random stock still comes from the real driver.
        run.world.map.points[index].point_type = MapPointType::Shop;
        let destination = run.world.map.points[index].coord;
        for seat in 0..seats {
            run.roster.player_mut(seat).unwrap().gold = 2000;
            run.vote(specs[seat].net_id, seat, destination).unwrap();
        }
        assert_eq!(run.phase, MpPhase::Shop);
        for owner in 0..seats {
            let offers = run.merchant_offers(specs[owner].net_id, owner).unwrap();
            let before = format!("{run:?}");
            let mut bindings = Vec::new();
            with_buffers(|b| {
                write_mp_menu_perspective(&run, owner as u8, &context, b, &mut bindings).unwrap();
                for (slot, binding) in bindings.iter().enumerate() {
                    assert_eq!(binding.net_id, specs[owner].net_id);
                    assert_eq!(binding.actor as usize, owner);
                    if let MpMenuAction::MerchantPurchase(action) = binding.action {
                        let offer = offers.iter().find(|o| o.action == action).unwrap();
                        assert_eq!(b.candidate_numeric_values[slot * SCALAR_SLOTS + 4], offer.price);
                        assert_eq!(b.legal_mask[slot], offer.enabled);
                    }
                }
            });
            with_buffers(|b| {
                write_mp_merchant_removal(&run, owner as u8, &context, b, &mut bindings).unwrap();
                assert_eq!(bindings.last().unwrap().action, MpMenuAction::CancelMerchantRemoval);
                assert!(bindings.iter().any(|a| matches!(a.action, MpMenuAction::MerchantPurchase(MerchantAction::RemoveCard(Some(_))))));
            });
            assert_eq!(format!("{run:?}"), before);
            let a = bindings[0];
            if let MpMenuAction::MerchantPurchase(action) = a.action {
                assert!(run.merchant_purchase(999, owner, action).is_err());
                assert!(run.merchant_purchase(a.net_id, a.actor as usize, action).unwrap());
            } else { panic!("expected removal binding"); }
        }
    }
}
