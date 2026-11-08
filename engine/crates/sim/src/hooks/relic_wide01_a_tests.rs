//! Wide A-M batch 1; expected effects come from the audited C# models.
use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let mut r = Relic::new(id);
    r.owner = owner;
    c.run.relics.push(r);
    c
}

#[test]
fn wide01_divine_right_and_ember_tea_target_each_owner_and_expire() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::DivineRight);
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        for seat in 0..4 {
            assert_eq!(c.players[seat].stars, if seat == owner { 3 } else { 0 });
        }
        assert!(c.fault.is_none(), "{:?}", c.fault);
        let mut c = setup(owner, RelicId::EmberTea);
        for used in 1..=6 {
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            for seat in 0..4 {
                assert_eq!(c.power_amount(seat, PowerId::Strength),
                    if seat == owner { 2 * used.min(5) } else { 0 });
            }
            assert_eq!(c.run.relics[0].counter, used.min(5));
        }
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide01_burning_sticks_clones_only_first_owner_skill_with_overflow_and_reset() {
    for owner in 0..4 {
        for full in [false, true] {
            let mut c = setup(owner, RelicId::BurningSticks);
            let foreign = c.new_card((owner + 1) % 4, CardId::DefendIronclad, false).unwrap();
            let attack = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
            c.fire_after_card_exhausted(foreign);
            c.fire_after_card_exhausted(attack);
            assert!(!c.run.relics[0].flag1);
            let skill = c.new_card(owner, CardId::DefendIronclad, true).unwrap();
            if full {
                for _ in 0..MAX_HAND_SIZE {
                    let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
                    c.players[owner].hand.push(k);
                }
            }
            let before = c.cards.len();
            c.fire_after_card_exhausted(skill);
            assert_eq!(c.cards.len(), before + 1);
            let clone = before as KRef;
            assert_eq!(c.cards[before].owner, owner);
            assert!(c.cards[before].upgraded);
            assert_ne!(c.cards[before].instance_id, c.cards[skill as usize].instance_id);
            assert!(if full { c.players[owner].discard.contains(&clone) }
                else { c.players[owner].hand.contains(&clone) });
            c.fire_after_card_exhausted(skill);
            assert_eq!(c.cards.len(), before + 1);
            c.fire_relics(Hook::AfterCombatEnd, Ev::None);
            assert!(!c.run.relics[0].flag1);
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}

#[test]
fn wide01_byrdpip_registers_one_pet_per_owner_without_touching_teammates() {
    let mut c = combat();
    for owner in 0..4 {
        let mut r = Relic::new(RelicId::Byrdpip);
        r.owner = owner;
        c.run.relics.push(r);
    }
    for _ in 0..2 { c.fire_relics(Hook::BeforeCombatStart, Ev::None); }
    for owner in 0..4 {
        assert_eq!(c.players[owner].pets.len(), 1);
        let pet = c.players[owner].pets[0];
        assert_eq!(c.cr(pet).pet_owner, Some(owner));
        assert_eq!(c.cr(pet).monster_id, "BYRDPIP");
        assert_eq!((c.cr(pet).hp, c.cr(pet).max_hp), (9999, 9999));
    }
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn wide01_frond_fills_only_owner_slots_preserves_existing_and_full_consumes_no_rng() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::DelicateFrond);
        c.run.all_cards_unlocked = Some(true);
        c.run.max_potion_slots = 3;
        c.run.potions = vec![None; 3];
        for seat in 0..4 {
            c.players[seat].character = Some([crate::character::Character::Ironclad, crate::character::Character::Silent, crate::character::Character::Defect, crate::character::Character::Regent][seat as usize]);
            if seat != 0 { c.players[seat].potions = Some(vec![None; 3]); }
        }
        c.relic_owner_potions(owner).unwrap()[0] = Some(PotionId::Strengthpotion);
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        for seat in 0..4 {
            let slots = c.relic_owner_potions(seat).unwrap();
            assert_eq!(slots.iter().filter(|p| p.is_some()).count(), if seat == owner { 3 } else { 0 });
        }
        assert_eq!(c.relic_owner_potions(owner).unwrap()[0], Some(PotionId::Strengthpotion));
        let counter = c.run.rngs.combat_potion_generation.counter;
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.run.rngs.combat_potion_generation.counter, counter);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
