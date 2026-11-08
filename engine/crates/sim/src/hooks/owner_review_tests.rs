//! Owner-only extra turns, block clearing, retain resources and death refusals.
use super::*;
use super::engine_tests::combat;
use crate::rng::Rng;

#[test]
fn demon_tongue_heals_only_first_owner_damage_on_owner_side() {
    for owner in 0..4 {
        let mut c = combat();
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        let mut tongue = crate::relic::Relic::new(RelicId::DemonTongue);
        tongue.owner = owner; c.run.relics.push(tongue);
        let enemy = c.enemies[0]; let other = (owner + 1) % 4;
        c.side = Side::Player;
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        c.deal_damage(Some(enemy), other, 3, Props::MOVE);
        assert_eq!(c.cr(other).hp, 47);
        assert!(!c.run.relics[0].flag1);
        c.cr_mut(owner).block = 5;
        c.deal_damage(Some(enemy), owner, 3, Props::MOVE);
        assert!(!c.run.relics[0].flag1, "blocked damage cannot spend the heal");
        c.deal_damage(Some(enemy), owner, 7, Props::MOVE);
        assert_eq!(c.cr(owner).hp, 50);
        assert!(c.run.relics[0].flag1);
        c.deal_damage(None, owner, 3, Props::UNPOWERED.or(Props::UNBLOCKABLE));
        assert_eq!(c.cr(owner).hp, 47);
        c.side = Side::Enemy;
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Enemy));
        assert!(c.run.relics[0].flag1, "enemy side cannot reset Owner's latch");
        c.deal_damage(Some(enemy), owner, 2, Props::MOVE);
        assert_eq!(c.cr(owner).hp, 45);
        c.side = Side::Player;
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        c.deal_damage(None, owner, 2, Props::UNPOWERED.or(Props::UNBLOCKABLE));
        assert_eq!(c.cr(owner).hp, 45);
        for seat in 0..4 {
            if seat != owner && seat != other { assert_eq!(c.cr(seat).hp, 50); }
        }
        assert!(c.fault.is_none(), "owner {owner}: {:?}", c.fault);
    }
}

#[test]
fn reviewer_extra_turn_and_owner_resource_guards() {
    for owner in 0..4 {
        let mut c = combat();
        let mut eye = crate::relic::Relic::new(RelicId::PaelsEye);
        eye.owner = owner;
        c.run.relics.push(eye);
        assert!(c.should_take_extra_turn_now());
        assert_eq!(c.extra_turn_players, 1 << owner);
        assert_eq!(c.fault, None);
        assert!(c.players.iter().all(|p| p.turn_number == 0));
    }
    for owner in 1..4 {
        let mut c = combat();
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        let mut tail = crate::relic::Relic::new(RelicId::LizardTail);
        tail.owner = owner;
        c.run.relics.push(tail);
        c.start_player_turn();
        assert!(c.fault.is_none());
        assert_eq!(c.fault_relic, None);
        assert!(!c.run.relics[0].flag1);
    }
}

#[test]
fn lizard_tail_revives_each_dying_seat_only_once_with_minimum_one_hp() {
    for owner in 0..4 {
        for max_hp in [1, 51] {
            let mut c = combat();
            c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
            for seat in 1..4 { c.players[seat].potions = Some(vec![]); }
            let mut tail = crate::relic::Relic::new(RelicId::LizardTail);
            tail.owner = owner; c.run.relics.push(tail);
            c.cr_mut(owner).max_hp = max_hp; c.cr_mut(owner).hp = max_hp;
            c.deal_damage(None, owner, 999, Props::UNPOWERED.or(Props::UNBLOCKABLE));
            assert_eq!(c.cr(owner).hp, (max_hp / 2).max(1), "seat {owner}");
            assert!(c.run.relics[0].flag1);
            assert!(c.players[owner].hooks_active);
            assert!(!c.cr(owner).death_processed);
            for seat in 0..4 { if seat != owner { assert_eq!(c.cr(seat).hp, 50); } }
            assert!(c.fault.is_none(), "seat {owner}: {:?}", c.fault);
            c.deal_damage(None, owner, 999, Props::UNPOWERED.or(Props::UNBLOCKABLE));
            assert_eq!(c.cr(owner).hp, 0);
            assert!(c.fault.is_none());
        }
    }
}

#[test]
fn lizard_tail_seat_two_prevents_party_defeat_during_a_batch_of_deaths() {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    c.players[2].potions = Some(vec![]);
    let mut tail = crate::relic::Relic::new(RelicId::LizardTail);
    tail.owner = 2; c.run.relics.push(tail);
    c.monster_attack_players(c.enemies[0], 999, 1);
    assert_eq!(c.cr(2).hp, 25);
    for seat in [0, 1, 3] { assert_eq!(c.cr(seat).hp, 0); }
    assert!(!c.is_over);
    assert!(c.players[2].hooks_active);
    assert!(c.fault.is_none(), "{:?}", c.fault);
}

#[test]
fn fairy_revives_only_its_owner_before_tail_and_fires_owner_potion_relics() {
    for owner in 0..4 {
        for max_hp in [1, 51] {
            let mut c = combat();
            c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
            for seat in 1..4 { c.players[seat].potions = Some(vec![None]); }
            c.run.potions = vec![None];
            c.relic_owner_potions(owner).unwrap()[0] = Some(PotionId::Fairyinabottle);
            for id in [RelicId::LizardTail, RelicId::ReptileTrinket, RelicId::BeltBuckle] {
                let mut r = crate::relic::Relic::new(id); r.owner = owner; c.run.relics.push(r);
            }
            c.cr_mut(owner).max_hp = max_hp; c.cr_mut(owner).hp = max_hp;
            c.deal_damage(None, owner, 999, Props::UNPOWERED.or(Props::UNBLOCKABLE));
            assert_eq!(c.cr(owner).hp, (max_hp * 30 / 100).max(1));
            assert!(!c.run.relics[0].flag1, "Fairy must precede Tail");
            assert!(c.relic_owner_potions(owner).unwrap()[0].is_none());
            assert_eq!(c.power_amount(owner, PowerId::Strength), 3);
            assert_eq!(c.power_amount(owner, PowerId::Dexterity), 2);
            for seat in 0..4 { if seat != owner { assert_eq!(c.cr(seat).hp, 50); assert_eq!(c.power_amount(seat, PowerId::Strength), 0); } }
            c.deal_damage(None, owner, 999, Props::UNPOWERED.or(Props::UNBLOCKABLE));
            assert_eq!(c.cr(owner).hp, (max_hp / 2).max(1));
            assert!(c.run.relics[0].flag1);
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}

#[test]
fn reviewer_extra_turn_clears_block_even_in_round_one() {
    let mut c = Combat::new(combat().run, 1);
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    c.round = 1;
    c.cr_mut(0).block = 13;
    c.start_player_turn();
    assert_eq!(c.players[0].turn_number, 1);
    assert_eq!(c.cr(0).block, 13);
    c.cr_mut(0).block = 17;
    c.start_player_turn();
    assert_eq!(c.players[0].turn_number, 2);
    assert_eq!(c.round, 1);
    assert_eq!(c.cr(0).block, 0);
}

#[test]
fn inactive_eye_owner_does_not_request_an_extra_turn() {
    for owner in 0..4 {
        let mut c = combat();
        let mut relic = crate::relic::Relic::new(RelicId::PaelsEye);
        relic.owner = owner;
        c.run.relics.push(relic);
        c.players[owner].hooks_active = false;
        assert_eq!(c.relic_should_take_extra_turn(0, owner), None);
        assert!(c.fault.is_none());
        assert!(!c.should_take_extra_turn_now());
        assert_eq!(c.fault, None);
    }
}

#[test]
fn dead_allies_advance_their_turn_number_and_clear_block() {
    for owner in 1..4 {
        let mut c = combat();
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        for p in c.players.0.iter_mut() { p.turn_number = 1; }
        c.cr_mut(owner).hp = 0;
        c.cr_mut(owner).block = 17;
        c.players[owner].hooks_active = false;
        c.round = 2;
        c.start_player_turn();
        assert_eq!(c.players[owner].turn_number, 2);
        assert_eq!(c.cr(owner).block, 0);
        assert!(!c.players[owner].hooks_active);
    }
}
