use sts2_sim::{
    relic::Relic,
    run::mp::{Character, MpPhase, MpRun, PlayerRewards, PlayerSpec},
    tables::{PotionId, RelicId},
};

fn offered(seats: usize) -> MpRun {
    let specs: Vec<_> = (0..seats).map(|i| PlayerSpec {
        net_id: 100 + i as u64, character: Character::Ironclad,
    }).collect();
    let mut run = MpRun::new("73001", &specs, 0).unwrap();
    run.phase = MpPhase::Rewards;
    run.rewards = Some((0..seats).map(|_| PlayerRewards {
        gold: vec![10], potion: Some(PotionId::Firepotion), cards: Vec::new(),
        extra_cards: Vec::new(), card_removals: Vec::new(),
        extra_relics: Vec::new(), extra_potions: Vec::new(), special_cards: Vec::new(),
        gold_taken: false, potion_taken: false, card_taken: false, completed: false,
    }).collect());
    run
}

#[test]
fn potion_reward_is_actor_bound_and_uses_only_owners_first_empty_slot() {
    for seats in 2..=4 {
        for owner in 0..seats {
            let mut run = offered(seats);
            run.roster.player_mut(owner).unwrap().potions[0] = Some(PotionId::Blockpotion);
            assert!(run.take_potion_reward(999, owner).is_err());
            assert!(!run.rewards.as_ref().unwrap()[owner].potion_taken);
            assert!(run.take_potion_reward(100 + owner as u64, owner).unwrap());
            for (seat, player) in run.roster.players().iter().enumerate() {
                assert_eq!(player.potions, if seat == owner {
                    vec![Some(PotionId::Blockpotion), Some(PotionId::Firepotion), None]
                } else { vec![None; 3] });
                assert_eq!(player.gold, 99);
            }
            assert!(run.take_potion_reward(100 + owner as u64, owner).is_err());
            assert_eq!(run.phase, MpPhase::Rewards);
        }
    }
}

#[test]
fn full_inventory_keeps_offer_retryable_and_sozu_denial_consumes_only_owners_offer() {
    let mut run = offered(2);
    run.roster.player_mut(1).unwrap().potions.fill(Some(PotionId::Blockpotion));
    assert!(!run.take_potion_reward(101, 1).unwrap());
    assert!(!run.rewards.as_ref().unwrap()[1].potion_taken);
    run.roster.player_mut(1).unwrap().potions[2] = None;
    assert!(run.take_potion_reward(101, 1).unwrap());
    assert_eq!(run.roster.player(1).unwrap().potions[2], Some(PotionId::Firepotion));

    let mut run = offered(2);
    let mut sozu = Relic::new(RelicId::Sozu);
    sozu.owner = 1;
    run.roster.player_mut(1).unwrap().relics.push(sozu);
    assert!(run.take_potion_reward(101, 1).unwrap());
    assert_eq!(run.roster.player(1).unwrap().potions, vec![None; 3]);
    assert!(run.rewards.as_ref().unwrap()[1].potion_taken);
    assert!(run.take_potion_reward(100, 0).unwrap());
    assert_eq!(run.roster.player(0).unwrap().potions[0], Some(PotionId::Firepotion));
}

#[test]
fn potion_reward_rejects_wrong_phase_absent_offer_and_completed_seat() {
    let mut run = offered(2);
    run.phase = MpPhase::Map;
    assert!(run.take_potion_reward(100, 0).is_err());
    run.phase = MpPhase::Rewards;
    run.rewards.as_mut().unwrap()[0].potion = None;
    assert!(run.take_potion_reward(100, 0).is_err());
    run.rewards.as_mut().unwrap()[0].potion = Some(PotionId::Firepotion);
    run.take_gold_skip_remaining_rewards(100, 0).unwrap();
    assert!(run.take_potion_reward(100, 0).is_err());
    assert_eq!(run.roster.player(0).unwrap().potions, vec![None; 3]);
}

#[test]
fn card_acceptance_preserves_offer_identity_owner_and_all_personal_rngs() {
    use sts2_sim::{card::Card, tables::CardId};
    for seats in 2..=4 {
        for owner in 0..seats {
            let mut run = offered(seats);
            run.roster.next_card_id += 1;
            let mut card = Card::from_factory(CardId::PommelStrike, false, run.roster.next_card_id).unwrap();
            card.owner = owner as u8;
            let iid = card.instance_id;
            run.rewards.as_mut().unwrap()[owner].cards.push(card);
            let rngs: Vec<_> = run.roster.players().iter().map(|p| p.rngs.clone()).collect();
            assert!(run.take_card_reward(999, owner, 0).is_err());
            assert!(run.take_card_reward(100 + owner as u64, owner, 1).is_err());
            run.take_card_reward(100 + owner as u64, owner, 0).unwrap();
            assert!(run.take_card_reward(100 + owner as u64, owner, 0).is_err());
            for (seat, p) in run.roster.players().iter().enumerate() {
                assert_eq!(p.deck.len(), 10 + usize::from(seat == owner));
                assert_eq!(p.rngs, rngs[seat]);
                assert_eq!(p.deck.iter().any(|c| c.instance_id == iid), seat == owner);
            }
            assert_eq!(run.phase, MpPhase::Rewards);
        }
    }
}

#[test]
fn owner_egg_upgrades_accepted_card_without_leaking_to_teammates() {
    use sts2_sim::{card::Card, tables::CardId};
    let mut run = offered(2);
    run.world.run.total_floor = 7;
    let mut egg = Relic::new(RelicId::MoltenEgg);
    egg.owner = 1;
    run.roster.player_mut(1).unwrap().relics.push(egg);
    let shared_inventory = format!("{:?}", run.world.run.relics);
    for seat in 0..2 {
        run.roster.next_card_id += 1;
        let mut card = Card::from_factory(CardId::PommelStrike, false, run.roster.next_card_id).unwrap();
        card.owner = seat as u8;
        run.rewards.as_mut().unwrap()[seat].cards.push(card);
        run.take_card_reward(100 + seat as u64, seat, 0).unwrap();
        assert_eq!(format!("{:?}", run.world.run.relics), shared_inventory,
            "the owner view must not replace the shared inventory projection");
    }
    assert!(!run.roster.player(0).unwrap().deck.last().unwrap().upgraded);
    assert!(run.roster.player(1).unwrap().deck.last().unwrap().upgraded);
    for player in run.roster.players() {
        assert_eq!(player.deck.last().unwrap().floor_added_to_deck, Some(7));
        assert_eq!(player.deck[0].floor_added_to_deck, Some(1));
    }
}

#[test]
fn remote_owners_can_discard_then_retry_a_retained_full_inventory_potion_offer() {
    for owner in 1..4 {
        let mut run = offered(4);
        run.roster.player_mut(owner).unwrap().potions.fill(Some(PotionId::Blockpotion));
        let rngs: Vec<_> = run.roster.players().iter().map(|p| p.rngs.clone()).collect();
        assert!(!run.take_potion_reward(100 + owner as u64, owner).unwrap());
        let before = format!("{run:?}");
        assert!(run.discard_potion(100, owner, 2).is_err());
        assert!(run.discard_potion(100 + owner as u64, owner, 99).is_err());
        assert_eq!(format!("{run:?}"), before);
        run.discard_potion(100 + owner as u64, owner, 2).unwrap();
        assert!(run.discard_potion(100 + owner as u64, owner, 2).is_err());
        assert!(run.take_potion_reward(100 + owner as u64, owner).unwrap());
        for (seat, player) in run.roster.players().iter().enumerate() {
            assert_eq!(player.rngs, rngs[seat]);
            assert_eq!(player.potions, if seat == owner {
                vec![Some(PotionId::Blockpotion), Some(PotionId::Blockpotion), Some(PotionId::Firepotion)]
            } else { vec![None; 3] });
        }
        assert!(run.rewards.as_ref().unwrap()[owner].potion_taken);
    }
}
