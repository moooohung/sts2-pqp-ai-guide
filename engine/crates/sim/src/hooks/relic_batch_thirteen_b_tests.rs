use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

#[test]
fn b13_tooth_returns_upgraded_cards_only_to_living_owner_using_owner_rng() {
    for owner in 0..4 {
        let mut c = combat();
        c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
        let mut r = Relic::new(RelicId::PaelsTooth); r.owner = owner;
        c.run.relics.push(r);
        for seat in 0..4 {
            c.players[seat].rewards_rng = Some(Rng::new(100 + seat as u64));
            c.players[seat].gold = Some(7);
            let mut card = crate::card::Card::from_factory(CardId::StrikeIronclad, false, 1000 + seat as u32).unwrap();
            card.owner = seat;
            c.run.paels_tooth_cards.push(card);
        }
        let mut fysh = Relic::new(RelicId::LuckyFysh); fysh.owner = owner;
        c.run.relics.push(fysh);
        let local_gold = c.run.gold;
        let local_rng = c.run.rngs.rewards.clone();
        let mut expected = if owner == 0 { local_rng.clone() } else { c.players[owner].rewards_rng.clone().unwrap() };
        expected.choice_index(1);
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        for seat in 0..4 {
            let deck = c.relic_owner_deck(seat);
            assert_eq!(deck.len(), usize::from(seat == owner));
            if seat == owner {
                assert!(deck[0].upgraded);
                assert_eq!(deck[0].owner, owner);
            }
        }
        assert_eq!(c.run.paels_tooth_cards.len(), 3);
        assert!(c.run.paels_tooth_cards.iter().all(|card| card.owner != owner));
        if owner == 0 {
            assert_eq!(c.run.rngs.rewards, expected);
            assert_eq!(c.run.gold, local_gold + 15);
        } else {
            assert_eq!(c.players[owner].rewards_rng.as_ref(), Some(&expected));
            assert_eq!(c.run.rngs.rewards, local_rng);
            assert_eq!(c.run.gold, local_gold);
            assert_eq!(c.players[owner].gold, Some(22));
        }
        assert_eq!(c.run.relics[0].counter, 0);
        assert!(c.fault.is_none(), "{:?}", c.fault);
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert_eq!(c.run.paels_tooth_cards.len(), 3, "other owners' cards cannot be returned");
    }
}

#[test]
fn b13_tooth_skips_dead_owner_and_empty_storage_without_touching_rng() {
    for owner in 0..4 {
        for dead in [false, true] {
            let mut c = combat();
            c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
            let mut r = Relic::new(RelicId::PaelsTooth); r.owner = owner;
            c.run.relics.push(r);
            if dead {
                let mut card = crate::card::Card::from_factory(CardId::Bash, false, 1000).unwrap();
                card.owner = owner;
                c.run.paels_tooth_cards.push(card);
                c.cr_mut(owner).hp = 0;
            }
            let rng = c.run.rngs.rewards.clone();
            c.fire_relics(Hook::AfterCombatEnd, Ev::None);
            assert!(c.relic_owner_deck(owner).is_empty());
            assert_eq!(c.run.paels_tooth_cards.len(), usize::from(dead));
            assert_eq!(c.run.rngs.rewards, rng);
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}
