use crate::character::Character;
use super::*;
use super::engine_tests::combat;
use crate::relic::Relic;

#[test]
fn reviewer_frond_default_solo_without_explicit_character_or_card_unlock_flag() {
    let template = combat();
    let mut c = Combat::new(template.run, 1);
    c.run.all_cards_unlocked = Some(false);
    c.players[0].character = None;
    c.run.max_potion_slots = 3;
    c.run.potions = vec![None];
    c.run.relics.push(Relic::new(RelicId::DelicateFrond));
    c.fire_relics(Hook::BeforeCombatStart, Ev::None);
    assert_eq!(c.run.potions.len(), 3);
    assert!(c.run.potions.iter().all(Option::is_some));
    let pool = crate::run::potion_pools::cards(Character::Ironclad).unwrap();
    for potion in c.run.potions.iter().flatten() { assert!(pool.contains(potion)); }
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn reviewer_frond_ignores_empty_storage_beyond_solo_slot_capacity() {
    let template = combat();
    let mut c = Combat::new(template.run, 1);
    c.run.max_potion_slots = 1;
    c.run.potions = vec![Some(PotionId::Strengthpotion), None];
    c.run.relics.push(Relic::new(RelicId::DelicateFrond));
    let counter = c.run.rngs.combat_potion_generation.counter;
    c.fire_relics(Hook::BeforeCombatStart, Ev::None);
    assert_eq!(c.run.potions, vec![Some(PotionId::Strengthpotion), None]);
    assert_eq!(c.run.rngs.combat_potion_generation.counter, counter);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn reviewer_sozu_stops_frond_after_one_factory_attempt_for_every_owner() {
    for owner in 0..4 {
        let mut c = combat();
        c.run.all_cards_unlocked = Some(true);
        c.run.max_potion_slots = 3;
        c.run.potions = vec![None; 3];
        for seat in 0..4 {
            c.players[seat].character = Some(Character::Ironclad);
            if seat > 0 { c.players[seat].potions = Some(vec![None; 3]); }
        }
        for id in [RelicId::DelicateFrond, RelicId::Sozu] {
            let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
        }
        // Dispatch only the Frond: Sozu's unaudited unrelated nonzero-seat
        // callbacks remain fail-closed, but TryToProcure's owner veto is ported.
        c.relic_event(0, Hook::BeforeCombatStart, Ev::None);
        assert!(c.relic_owner_potions(owner).unwrap().iter().all(Option::is_none));
        // Independent factory invariant: one rarity roll and one selection roll.
        let mut expected = crate::rng::Rng::new(1);
        expected.next_float(1.0);
        let pool = crate::run::potion_pools::cards(Character::Ironclad).unwrap();
        let mut rarity_rng = crate::rng::Rng::new(1);
        let roll = rarity_rng.next_float(1.0);
        let rarity = if roll <= 0.1 { PotionRarity::Rare }
            else if roll <= 0.35 { PotionRarity::Uncommon } else { PotionRarity::Common };
        expected.choice_index(pool.iter().filter(|id| POTION_INFO[id.index()].rarity == rarity).count());
        assert_eq!(c.run.rngs.combat_potion_generation.counter, expected.counter);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn reviewer_ghost_seed_new_cards_are_owner_scoped_and_basic_tag_filtered() {
    for owner in 0..4 {
        let mut c = combat();
        let mut r = Relic::new(RelicId::GhostSeed); r.owner = owner; c.run.relics.push(r);
        for (seat, id, ethereal) in [
            (owner, CardId::StrikeIronclad, true),
            (owner, CardId::DefendIronclad, true),
            (owner, CardId::Bash, false),
            ((owner + 1) % 4, CardId::StrikeIronclad, false),
        ] {
            let k = c.new_card(seat, id, false).unwrap();
            c.fire_relics(Hook::AfterCardEnteredCombat, Ev::Card(k));
            assert_eq!(c.cards[k as usize].keywords & KW_ETHEREAL != 0, ethereal);
        }
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
