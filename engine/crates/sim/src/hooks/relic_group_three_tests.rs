//! C# Models/Relics/{RainbowRing,Vajra,PetrifiedToad,RedMask,Vambrace}.cs.
use super::*;
use super::engine_tests::combat;
use crate::rng::Rng;

fn with_relic(owner: CId, id: RelicId) -> Combat {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let mut relic = crate::relic::Relic::new(id);
    relic.owner = owner;
    c.run.relics.push(relic);
    c
}

#[test]
fn group_three_vajra_and_toad_only_touch_owner_and_respect_hooks() {
    for owner in 0..4 {
        let mut c = with_relic(owner, RelicId::Vajra);
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        for seat in 0..4 {
            assert_eq!(c.power_amount(seat, PowerId::Strength), i32::from(seat == owner));
        }
        c.players[owner].hooks_active = false;
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.power_amount(owner, PowerId::Strength), 1);
        assert!(c.fault.is_none());

        let mut c = with_relic(owner, RelicId::PetrifiedToad);
        c.run.potions = vec![None; 2];
        c.run.max_potion_slots = 2;
        for seat in 1..4 { c.players[seat].potions = Some(vec![None; 2]); }
        c.fire_relics(Hook::BeforeCombatStartLate, Ev::None);
        for seat in 0..4 {
            let slots = c.relic_owner_potions(seat).unwrap();
            assert_eq!(slots[0], if seat == owner { Some(PotionId::Potionshapedrock) } else { None });
        }
        c.fire_relics(Hook::BeforeCombatStartLate, Ev::None);
        c.fire_relics(Hook::BeforeCombatStartLate, Ev::None);
        assert_eq!(c.relic_owner_potions(owner).unwrap().len(), 2);
        assert!(c.fault.is_none());
    }
}

#[test]
fn group_three_red_mask_uses_owner_turn_not_party_round() {
    for owner in 0..4 {
        let mut c = with_relic(owner, RelicId::RedMask);
        c.round = 5;
        c.players[owner].turn_number = 1;
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Enemy));
        let enemy = c.enemies[0];
        assert_eq!(c.power_amount(enemy, PowerId::Weak), 0);
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.power_amount(enemy, PowerId::Weak), 1);
        c.round = 1;
        c.players[owner].turn_number = 2;
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.power_amount(enemy, PowerId::Weak), 1);
        assert!(c.fault.is_none());
    }
}

#[test]
fn group_three_rainbow_counts_only_owner_once_per_turn_in_progress() {
    for owner in 0..4 {
        let mut c = with_relic(owner, RelicId::RainbowRing);
        let cards = [CardId::StrikeIronclad, CardId::DefendIronclad, CardId::Inflame];
        for id in cards {
            let k = c.new_card((owner + 1) % 4, id, false).unwrap();
            c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        }
        assert_eq!(c.run.relics[0].counter, 0);
        for _ in 0..2 {
            for id in cards {
                let k = c.new_card(owner, id, false).unwrap();
                c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
            }
        }
        assert_eq!(c.power_amount(owner, PowerId::Strength), 1);
        assert_eq!(c.power_amount(owner, PowerId::Dexterity), 1);
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        c.is_over = true;
        for id in cards {
            let k = c.new_card(owner, id, false).unwrap();
            c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        }
        assert_eq!(c.run.relics[0].counter, 0);
        for seat in 0..4 {
            if seat != owner { assert_eq!(c.power_amount(seat, PowerId::Strength), 0); }
        }
        assert!(c.fault.is_none());
    }
}

#[test]
fn group_three_vambrace_doubles_owner_card_even_when_recipient_is_an_ally() {
    for owner in 0..4 {
        let mut c = with_relic(owner, RelicId::Vambrace);
        let other = (owner + 1) % 4;
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        let foreign = c.new_card(other, CardId::DefendIronclad, false).unwrap();
        assert_eq!(c.calculate_block(5, owner, Props::MOVE, Some(foreign)), 5);
        assert_eq!(c.calculate_block(5, other, Props::MOVE, Some(k)), 10);
        assert_eq!(c.calculate_block(5, other, Props::MOVE, Some(k)), 10);
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(foreign));
        assert!(!c.run.relics[0].flag1);
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        assert!(c.run.relics[0].flag1);
        assert_eq!(c.calculate_block(5, other, Props::MOVE, Some(k)), 5);
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert!(!c.run.relics[0].flag1);
        assert_eq!(c.run.relics[0].card, None);
        assert!(c.fault.is_none());
    }
}
