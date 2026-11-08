use crate::character::Character;
use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

#[test]
fn sozu_energy_and_procurement_are_owner_only() {
    for owner in 0..4 {
        let mut c = combat();
        for seat in 0..4 { c.players[seat].character = Some(Character::Ironclad); }
        c.run.max_potion_slots = 3;
        c.run.potions = vec![None; 3];
        for seat in 1..4 { c.players[seat].potions = Some(vec![None; 3]); }
        for id in [RelicId::Sozu, RelicId::DelicateFrond] {
            let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
        }
        for seat in 0..4 { assert_eq!(c.max_energy(seat), if seat == owner { 4 } else { 3 }); }
        c.run.all_cards_unlocked = Some(true);
        // DelicateFrond generates one potion before Sozu vetoes procurement:
        // PotionFactory rarity roll, then selection within that rarity.
        let mut rng = c.run.rngs.combat_potion_generation.clone();
        let roll = rng.next_float(1.0);
        let rarity = if roll <= 0.1 { PotionRarity::Rare }
            else if roll <= 0.35 { PotionRarity::Uncommon } else { PotionRarity::Common };
        let pool = crate::run::potion_pools::cards(Character::Ironclad).unwrap();
        rng.choice_index(pool.iter().filter(|id| POTION_INFO[id.index()].rarity == rarity).count());
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.run.rngs.combat_potion_generation.counter, rng.counter);
        assert!(c.run.potions.iter().all(Option::is_none));
        for seat in 1..4 { assert!(c.players[seat].potions.as_ref().unwrap().iter().all(Option::is_none)); }
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn screaming_flagon_checks_only_owners_hand_and_deals_unpowered_owner_damage() {
    for owner in 0..4 {
        let mut c = combat(); c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
        for enemy in c.enemies.clone() { c.cr_mut(enemy).max_hp = 200; c.cr_mut(enemy).hp = 200; }
        let mut r = Relic::new(RelicId::ScreamingFlagon); r.owner = owner; c.run.relics.push(r);
        let held = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        c.players[owner].hand.push(held);
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Player));
        for enemy in &c.enemies { assert_eq!(c.cr(*enemy).hp, 200); }
        c.players[owner].hand.clear();
        let other = (owner + 1) % 4;
        let card = c.new_card(other, CardId::StrikeIronclad, false).unwrap();
        c.players[other].hand.push(card);
        c.active_card = Some(card);
        c.apply_power_to(owner, PowerId::Strength, 50, Some(owner));
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Enemy));
        for enemy in &c.enemies { assert_eq!(c.cr(*enemy).hp, 200); }
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Player));
        for enemy in &c.enemies { assert_eq!(c.cr(*enemy).hp, 180); }
        assert!(c.hist.damage.iter().all(|d| d.dealer == Some(owner)));
        assert_eq!(c.active_card, Some(card));
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn sz02_equipped_run_only_relics_preserve_the_supplied_entry() {
    for owner in 0..4 {
        let mut c = combat(); c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        for id in [RelicId::WingedBoots, RelicId::WhiteStar, RelicId::SandCastle, RelicId::Storybook] {
            let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
        }
        c.start_combat();
        assert!(c.relic_owner_deck(owner).is_empty());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
