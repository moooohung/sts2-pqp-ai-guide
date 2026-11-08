use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

#[test]
fn b12_pearl_creates_for_starting_owner_only_on_their_first_turn() {
    for owner in 0..4 {
        let mut c = combat();
        let mut r = Relic::new(RelicId::RadiantPearl);
        r.owner = owner;
        c.run.relics.push(r);
        c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
        c.round = 9;
        for seat in 0..4 {
            c.players[seat].turn_number = 1;
            c.fire(Hook::BeforeHandDraw, Ev::Creature(seat), Some(seat));
        }
        for seat in 0..4 {
            assert_eq!(c.players[seat].hand.len(), usize::from(seat == owner));
        }
        assert_eq!(c.cards.len(), 1);
        assert_eq!(c.cards[0].id, CardId::Luminesce);
        assert_eq!(c.cards[0].owner, owner);
        c.round = 1;
        c.players[owner].turn_number = 2;
        c.fire(Hook::BeforeHandDraw, Ev::Creature(owner), Some(owner));
        assert_eq!(c.cards.len(), 1, "same-round extra turns do not generate another card");
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn b12_acquisition_models_do_not_replay_deck_or_reward_changes_in_combat() {
    for owner in 0..4 {
        for id in [RelicId::Pomander, RelicId::NutritiousSoup, RelicId::NeowsBones] {
            let mut c = combat();
            let mut r = Relic::new(id);
            r.owner = owner;
            c.run.relics.push(r);
            c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
            for seat in 0..4 {
                let k = c.new_card(seat, CardId::StrikeIronclad, false).unwrap();
                let card = c.cards[k as usize].clone();
                c.relic_owner_deck_mut(seat).push(card);
            }
            let niche = c.run.rngs.niche.counter;
            let rewards = c.run.rngs.rewards.counter;
            for hook in [Hook::BeforeCombatStart, Hook::AfterCombatVictory, Hook::AfterCombatEnd] {
                c.fire_relics(hook, Ev::None);
            }
            for seat in 0..4 {
                let deck = c.relic_owner_deck(seat);
                assert_eq!(deck.len(), 1);
                assert_eq!(deck[0].id, CardId::StrikeIronclad);
                assert!(!deck[0].upgraded);
                assert!(deck[0].enchantments.is_empty());
            }
            assert_eq!(c.run.relics.len(), 1);
            assert_eq!(c.run.rngs.niche.counter, niche);
            assert_eq!(c.run.rngs.rewards.counter, rewards);
            assert!(c.fault.is_none(), "{id:?}: {:?}", c.fault);
        }
    }
}
