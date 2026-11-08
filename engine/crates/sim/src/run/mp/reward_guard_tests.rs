use crate::run::mp::{Character, MpPhase, MpRun, PlayerSpec};
use crate::{card::Card, combat::EndReward, tables::{CardId, RelicId, RoomType}};

fn victorious_run() -> MpRun {
    let specs: Vec<_> = (0..4).map(|seat| PlayerSpec {
        net_id: 100 + seat, character: Character::Ironclad,
    }).collect();
    let mut run = MpRun::new("73001", &specs, 0).unwrap();
    let mut c = run.roster.enter_combat(run.world.run.clone(), 1,
        "SLUDGE_SPINNER_WEAK", RoomType::Monster).unwrap();
    let enemy = c.enemies[0];
    c.cr_mut(enemy).hp = 1;
    let card = c.new_card(0, CardId::StrikeIronclad, false).unwrap();
    c.players[0].hand.push(card);
    let hand = c.players[0].hand.len() - 1;
    assert!(c.play_card(0, hand, Some(0)));
    assert!(c.is_over && c.won);
    // A carryback would visibly change the roster; refusal must not do it.
    c.run.gold -= 11;
    run.combat = Some(c);
    run.phase = MpPhase::Rewards;
    run
}

#[test]
fn owner_bound_card_removal_rewards_allow_cancel_then_remove_only_the_owner_card() {
    for owner in 0..4 {
        let mut run = victorious_run();
        run.combat.as_mut().unwrap().end_rewards.push(EndReward::CardRemoval { owner });
        run.offer_rewards().unwrap();
        let before=format!("{run:?}");
        run.take_card_removal_reward(100+owner as u64,owner as usize,0,None).unwrap();
        assert_eq!(format!("{run:?}"),before);
        assert!(run.take_card_removal_reward(999,owner as usize,0,Some(0)).is_err());
        let count=run.roster.player(owner as usize).unwrap().deck.len();
        run.take_card_removal_reward(100+owner as u64,owner as usize,0,Some(0)).unwrap();
        assert_eq!(run.roster.player(owner as usize).unwrap().deck.len(),count-1);
        assert!(run.rewards.as_ref().unwrap()[owner as usize].card_removals[0]);
        assert!(run.take_card_removal_reward(100+owner as u64,owner as usize,0,Some(0)).is_err());
    }
}

#[test]
fn special_card_rewards_keep_the_retained_owner_instance_until_selection() {
    for owner in 0..4usize {
        let mut run=victorious_run();
        let card=run.combat.as_mut().unwrap().player_deck_mut(owner as u8).remove(0);
        let before=run.combat.as_ref().unwrap().player_deck(owner as u8).len();
        run.combat.as_mut().unwrap().end_rewards.push(EndReward::SpecialCard{
            owner:owner as u8,card:card.clone()});
        run.offer_rewards().unwrap();
        assert_eq!(run.roster.player(owner).unwrap().deck.len(),before);
        let reward=&run.rewards.as_ref().unwrap()[owner];
        assert_eq!(reward.special_cards[0].as_ref().unwrap().instance_id,card.instance_id);
        let index=reward.special_card_reward_index(0).unwrap();
        assert_eq!(index,reward.gold.len()+usize::from(reward.potion.is_some())
            +reward.extra_relics.len()+reward.extra_potions.len());
        let rngs=format!("{:?}",run.roster.player(owner).unwrap().rngs);
        let snapshot=format!("{run:?}");
        assert!(run.take_special_card_reward(100+owner as u64,(owner+1)%4,index).is_err());
        assert_eq!(format!("{run:?}"),snapshot);
        run.take_special_card_reward(100+owner as u64,owner,index).unwrap();
        let received=run.roster.player(owner).unwrap().deck.last().unwrap();
        assert_eq!((received.id,received.instance_id,received.upgraded),
            (card.id,card.instance_id,card.upgraded));
        assert_eq!(received.enchantments,card.enchantments);
        assert_eq!(format!("{:?}",run.roster.player(owner).unwrap().rngs),rngs);
        assert_eq!(run.roster.player(owner).unwrap().deck.len(),before+1);
        let snapshot=format!("{run:?}");
        assert!(run.take_special_card_reward(100+owner as u64,owner,index).is_err());
        assert_eq!(format!("{run:?}"),snapshot);
    }
}

#[test]
fn wongo_ticket_populates_three_rewards_only_for_its_owner_after_five_combats() {
    for owner in 0..4 {
        for count in [4,5] {
            let mut run=victorious_run();
            let mut ticket=crate::relic::Relic::new(RelicId::WongosMysteryTicket);
            ticket.owner=owner;
            ticket.counter=count;
            run.combat.as_mut().unwrap().run.relics.push(ticket);
            run.offer_rewards().unwrap();
            for seat in 0..4 {
                assert_eq!(run.rewards.as_ref().unwrap()[seat].extra_relics.len(),
                    if seat==owner as usize&&count==5 {3}else{0});
            }
            let ticket=run.roster.player(owner as usize).unwrap().relics.iter()
                .find(|r|r.id==RelicId::WongosMysteryTicket).unwrap();
            assert_eq!(ticket.flag1,count==5);
            assert_eq!(ticket.enabled,count<5);
        }
    }
}

#[test]
fn multiple_special_card_ordering_stays_fail_closed_before_carryback() {
    let mut run=victorious_run();
    let card=run.combat.as_ref().unwrap().player_deck(0)[0].clone();
    for _ in 0..2 {
        run.combat.as_mut().unwrap().end_rewards.push(EndReward::SpecialCard{owner:0,card:card.clone()});
    }
    let before=format!("{run:?}");
    assert_eq!(run.offer_rewards(),Err("unported multiplayer multiple special-card reward ordering"));
    assert_eq!(format!("{run:?}"),before);
}

#[test]
fn stolen_back_gold_is_not_silently_dropped_for_any_owner() {
    for owner in 0..4 {
        let mut run = victorious_run();
        if owner == 0 {
            run.combat.as_mut().unwrap().run.extra_gold_reward = 23;
        } else {
            run.combat.as_mut().unwrap().players[owner].extra_gold_reward = 23;
        }
        run.offer_rewards().unwrap();
        assert_eq!(run.rewards.as_ref().unwrap()[owner as usize].gold.last(),Some(&23));
    }
}

#[test]
fn combat_extra_gold_and_card_rewards_are_retained_for_their_owner_without_double_gold() {
    for owner in 0..4 {
        let mut run=victorious_run();
        let c=run.combat.as_mut().unwrap();
        c.end_rewards.push(EndReward::Gold{owner,amount:19});
        c.end_rewards.push(EndReward::Card{owner,room:RoomType::Monster});
        if owner==0 {c.run.extra_gold_reward=19;}else{c.players[owner].extra_gold_reward=19;}
        run.offer_rewards().unwrap();
        let reward=&run.rewards.as_ref().unwrap()[owner as usize];
        assert_eq!(reward.gold.last(),Some(&19));
        assert_eq!(reward.gold.len(),2);
        assert_eq!(reward.extra_cards[0].as_ref().unwrap().len(),3);
        let before=run.roster.player(owner as usize).unwrap().deck.len();
        run.take_extra_card_reward(100+owner as u64,owner as usize,0,0).unwrap();
        assert_eq!(run.roster.player(owner as usize).unwrap().deck.len(),before+1);
        assert!(run.take_extra_card_reward(100+owner as u64,owner as usize,0,0).is_err());
    }
}

#[test]
fn fixed_combat_extra_gold_population_advances_only_recipients_rng_once() {
    let mut base=victorious_run();
    let mut extra=base.clone();
    extra.combat.as_mut().unwrap().end_rewards.push(EndReward::Gold{owner:2,amount:19});
    base.offer_rewards().unwrap();
    extra.offer_rewards().unwrap();
    for seat in 0..4 {
        assert_eq!(extra.roster.player(seat).unwrap().rngs.rewards.counter,
            base.roster.player(seat).unwrap().rngs.rewards.counter+u64::from(seat==2));
    }
}

#[test]
fn exhausted_elite_bags_offer_circlet_for_every_owner() {
    // RelicGrabBag.cs:185-207 and RelicFactory.cs:32-36: exhaustion falls
    // through to Circlet; it must not destroy combat or strand reward offering.
    let mut run = victorious_run();
    run.combat.as_mut().unwrap().run.room_type = RoomType::Elite;
    for bags in &mut run.world.player_bags { for bag in bags { bag.clear(); } }
    for fallback in &mut run.world.player_fallbacks { fallback.clear(); }
    run.offer_rewards().unwrap();
    assert!(run.combat().is_none());
    assert_eq!(run.rewards.as_ref().unwrap().len(), 4);
    for rewards in run.rewards.as_ref().unwrap() {
        assert_eq!(rewards.extra_relics, vec![Some(RelicId::Circlet)]);
    }
    let before = format!("{run:?}");
    run.offer_rewards().unwrap();
    assert_eq!(format!("{run:?}"), before, "offered rewards are idempotent");
}

#[test]
fn elite_relic_populates_before_gold_and_hunt_extras() {
    // RewardsSet.GenerateRewardsFor -> WithRewardsFromRoom -> Populate:
    // ordinary card, ordinary elite relic, fixed Gold, The Hunt card options.
    for owner in 0..4usize {
        let mut ordinary=victorious_run();
        ordinary.combat.as_mut().unwrap().run.room_type=RoomType::Elite;
        let mut extra=ordinary.clone();
        let c=extra.combat.as_mut().unwrap();
        c.end_rewards.push(EndReward::Gold{owner:owner as u8,amount:19});
        c.end_rewards.push(EndReward::Card{owner:owner as u8,room:RoomType::Elite});
        ordinary.offer_rewards().unwrap();
        let expected_relic=ordinary.rewards.as_ref().unwrap()[owner].extra_relics.clone();
        let mut expected_player=ordinary.roster.player(owner).unwrap().clone();
        expected_player.rngs.rewards.next_int(19,19);
        let mut next_id=ordinary.roster.next_card_id;
        let expected_cards=super::super::rewards::generate_card_reward(&mut expected_player,
            owner as u8,ordinary.world.run.ascension,ordinary.world.run.act_index,
            &mut next_id,3,RoomType::Elite,&mut ordinary.world.run.rngs.niche).unwrap();
        extra.offer_rewards().unwrap();
        let actual=&extra.rewards.as_ref().unwrap()[owner];
        assert_eq!(actual.extra_relics,expected_relic);
        let actual_cards=actual.extra_cards[0].as_ref().unwrap();
        assert_eq!(actual_cards.iter().map(|c|(c.id,c.upgraded)).collect::<Vec<_>>(),
            expected_cards.iter().map(|c|(c.id,c.upgraded)).collect::<Vec<_>>());
        assert_eq!(extra.roster.player(owner).unwrap().rngs.rewards.counter,
            expected_player.rngs.rewards.counter);
    }
}

#[test]
fn reward_generation_refusal_rolls_back_carryback_and_rng_and_is_retryable() {
    let mut run = victorious_run();
    let combat = run.combat.as_mut().unwrap();
    // This refusal happens in reward generation AFTER successful carryback,
    // which would otherwise publish the changed gold and Rewards RNG.
    combat.run.room_type = RoomType::Shop;
    combat.run.rngs.rewards.next_float(1.0);
    let before = format!("{run:?}");
    for _ in 0..2 {
        assert_eq!(run.offer_rewards(), Err("unsupported multiplayer reward room type"));
        assert_eq!(format!("{run:?}"), before,
            "refusal must retain combat, carryback, inventories, bags, rewards and RNGs");
    }
}

#[test]
fn relic_pull_prunes_every_queue_escalates_and_uses_fifo_fallback_before_circlet() {
    let mut run = victorious_run();
    run.world.run.total_floor = 38;
    run.world.player_bags[0] = [vec![RelicId::MealTicket], vec![RelicId::BowlerHat],
        vec![RelicId::OldCoin, RelicId::IceCream], Vec::new()];
    run.world.player_fallbacks[0] = vec![RelicId::MealTicket, RelicId::Vajra, RelicId::Anchor];
    let other_bags = run.world.player_bags[1..].to_vec();
    for expected in [RelicId::IceCream, RelicId::Vajra, RelicId::Anchor, RelicId::Circlet] {
        assert_eq!(run.world.pull_player_relic_from_front(&run.roster, 0, 0), expected);
        assert!(!run.world.shared_bags.iter().flatten().any(|id| *id == expected));
    }
    assert!(run.world.player_bags[0].iter().all(Vec::is_empty));
    assert!(run.world.player_fallbacks[0].is_empty());
    assert_eq!(run.world.player_bags[1..], other_bags);
}

#[test]
fn relic_pull_checks_party_unlocks_and_multiplayer_exclusions() {
    let mut run = victorious_run();
    run.roster.player_mut(3).unwrap().unlock_state.number_of_runs = 0;
    run.world.player_bags[0] = [Vec::new(), vec![RelicId::LastingCandy],
        vec![RelicId::WingedBoots, RelicId::SilverCrucible, RelicId::IceCream], Vec::new()];
    assert_eq!(run.world.pull_player_relic_from_front(&run.roster, 0, 1), RelicId::IceCream);
    assert!(run.world.player_bags[0].iter().all(Vec::is_empty));
}

#[test]
fn relic_pull_keeps_late_exclusions_before_multiplayer_floor_38() {
    let mut run = victorious_run();
    run.world.run.total_floor = 37;
    run.world.player_bags[0][0] = vec![RelicId::MealTicket];
    assert_eq!(run.world.pull_player_relic_from_front(&run.roster, 0, 0), RelicId::MealTicket);
}
