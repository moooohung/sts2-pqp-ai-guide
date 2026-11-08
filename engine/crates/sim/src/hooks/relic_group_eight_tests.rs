use crate::character::Character;
use super::*;
use super::engine_tests::combat;
use crate::rng::Rng;
fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat(); c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let mut r = crate::relic::Relic::new(id); r.owner = owner; c.run.relics.push(r); c
}
#[test]
fn group_eight_puzzlebox_only_generates_for_owner_on_its_first_turn() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::VexingPuzzlebox);
        c.run.all_cards_unlocked = Some(true);
        c.players[owner].character = Some(Character::Ironclad);
        c.set_character_pool(owner, Character::Ironclad).unwrap();
        c.round = 9; c.players[owner].turn_number = 1;
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature((owner + 1) % 4));
        assert!(c.players[owner].hand.is_empty());
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
        assert_eq!(c.players[owner].hand.len(), 1);
        let k = c.players[owner].hand[0];
        assert_eq!(c.cards[k as usize].owner, owner);
        assert_eq!(c.modified_card_cost(owner, k), 0);
        c.round = 1; c.players[owner].turn_number = 2;
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
        assert_eq!(c.players[owner].hand.len(), 1);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
#[test]
fn group_eight_basin_and_helmet_only_consider_owner_plays_and_payments() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::RippleBasin); let other = (owner + 1) % 4;
        let k = c.new_card(other, CardId::StrikeIronclad, false).unwrap();
        c.hist.played_this_turn.push((other, k));
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Player));
        assert_eq!(c.cr(owner).block, 4);
        let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        c.hist.played_this_turn.push((owner, k));
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Player));
        assert_eq!(c.cr(owner).block, 4); assert!(c.fault.is_none());
        let mut c = setup(owner, RelicId::IntimidatingHelmet);
        for seat in 0..4 {
            let k = c.new_card(seat, CardId::Bash, false).unwrap();
            c.cards[k as usize].energy_spent = 1;
            c.fire_relics(Hook::BeforeCardPlayed, Ev::Card(k));
            c.cards[k as usize].energy_spent = 2;
            c.fire_relics(Hook::BeforeCardPlayed, Ev::Card(k));
        }
        assert_eq!(c.cr(owner).block, 4);
        for seat in 0..4 { if seat != owner { assert_eq!(c.cr(seat).block, 0); } }
        assert!(c.fault.is_none());
    }
}
#[test]
fn group_eight_ruined_helmet_doubles_only_first_positive_owner_strength() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::RuinedHelmet); let other = (owner + 1) % 4;
        c.apply_power_from(other, PowerId::Strength, 2, Some(other), None);
        assert_eq!(c.power_amount(other, PowerId::Strength), 2);
        c.apply_power_from(owner, PowerId::Strength, -1, Some(owner), None);
        assert!(!c.run.relics[0].flag1);
        c.apply_power_from(owner, PowerId::Strength, 2, Some(owner), None);
        assert_eq!(c.power_amount(owner, PowerId::Strength), 3);
        c.apply_power_from(owner, PowerId::Strength, 2, Some(owner), None);
        assert_eq!(c.power_amount(owner, PowerId::Strength), 5);
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        c.apply_power_from(owner, PowerId::Strength, 1, Some(owner), None);
        assert_eq!(c.power_amount(owner, PowerId::Strength), 7);
        assert!(c.fault.is_none());
    }
}

#[test]
fn group_eight_ruined_helmet_does_not_reset_after_an_earlier_entry_strength_hook() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::RuinedHelmet);
        let mut vajra = crate::relic::Relic::new(RelicId::Vajra);
        vajra.owner = owner; c.run.relics.insert(0, vajra);
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.power_amount(owner, PowerId::Strength), 2);
        c.apply_power_from(owner, PowerId::Strength, 1, Some(owner), None);
        assert_eq!(c.power_amount(owner, PowerId::Strength), 3);
        assert!(c.fault.is_none());
    }
}
