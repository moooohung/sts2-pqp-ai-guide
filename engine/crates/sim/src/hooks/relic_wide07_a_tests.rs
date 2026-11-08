use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat(); c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r); c
}

#[test]
fn wide07_daughter_real_attacks_block_only_for_their_owner() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::DaughterOfTheWind);
        let other = (owner + 1) % 4;
        for seat in [other, owner] {
            let k = c.new_card(seat, CardId::StrikeIronclad, false).unwrap();
            c.players[seat].hand.push(k); c.players[seat].energy = 3;
            assert!(c.play_card(seat, 0, Some(0)));
            assert_eq!(c.cr(owner).block, i32::from(seat == owner));
            assert_eq!(c.cr(other).block, 0);
        }
        let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        assert_eq!(c.cr(owner).block, 1);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide07_fake_dummy_accepts_owner_dealer_or_card_but_never_foreign_or_unpowered() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::FakeStrikeDummy);
        let other = (owner + 1) % 4; let target = c.enemies[0];
        let own = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        let foreign = c.new_card(other, CardId::StrikeIronclad, false).unwrap();
        let bash = c.new_card(owner, CardId::Bash, false).unwrap();
        assert_eq!(c.relic_damage_additive(0, owner, Some(owner), target, Props::MOVE, Some(foreign)), 1);
        assert_eq!(c.relic_damage_additive(0, owner, Some(other), target, Props::MOVE, Some(own)), 1);
        assert_eq!(c.relic_damage_additive(0, owner, Some(other), target, Props::MOVE, Some(foreign)), 0);
        assert_eq!(c.relic_damage_additive(0, owner, Some(owner), target, Props::UNPOWERED, Some(own)), 0);
        assert_eq!(c.relic_damage_additive(0, owner, Some(owner), target, Props::MOVE, Some(bash)), 0);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide07_vial_and_mushroom_read_owner_turn_not_round_and_ignore_teammates() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::FakeBloodVial);
        c.cr_mut(owner).hp = 40; c.players[owner].turn_number = 1; c.round = 99;
        c.fire_relics(Hook::AfterPlayerTurnStartLate, Ev::Creature((owner + 1) % 4));
        assert_eq!(c.cr(owner).hp, 40);
        c.fire_relics(Hook::AfterPlayerTurnStartLate, Ev::Creature(owner));
        assert_eq!(c.cr(owner).hp, 41);
        c.players[owner].turn_number = 2; c.round = 1;
        c.fire_relics(Hook::AfterPlayerTurnStartLate, Ev::Creature(owner));
        assert_eq!(c.cr(owner).hp, 41);
        let mut c = setup(owner, RelicId::BigMushroom);
        c.players[owner].turn_number = 1; c.round = 99;
        assert_eq!(c.relic_modify_hand_draw(0, owner, 5, false), 3);
        c.players[owner].turn_number = 2; c.round = 1;
        assert_eq!(c.relic_modify_hand_draw(0, owner, 5, false), 5);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide07_equipped_pickup_only_models_do_not_modify_the_supplied_deck() {
    for owner in 0..4 {
        let mut c = combat(); c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        for id in [RelicId::GnarledHammer, RelicId::FakeMerchantsRug,
            RelicId::BeautifulBracelet, RelicId::Claws] {
            let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
        }
        c.start_combat();
        assert!(c.relic_owner_deck(owner).is_empty());
        assert!(c.players[owner].hand.is_empty());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
