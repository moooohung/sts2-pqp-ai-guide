//! C# Models/Relics/{Orichalcum,BeatingRemnant,BagOfMarbles,Kunai,MeatOnTheBone}.cs.
use super::*;
use super::engine_tests::combat;
use crate::rng::Rng;

fn with_relic(owner: CId, id: RelicId) -> Combat {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let mut r = crate::relic::Relic::new(id);
    r.owner = owner;
    c.run.relics.push(r);
    c
}

#[test]
fn group_four_orichalcum_latches_owner_block_at_very_early_tier() {
    for owner in 0..4 {
        let mut c = with_relic(owner, RelicId::Orichalcum);
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        c.fire_relics(Hook::BeforeTurnEndVeryEarly, Ev::Side(Side::Player));
        c.cr_mut(owner).block = 4;
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Enemy));
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Player));
        assert_eq!(c.cr(owner).block, 10);
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Player));
        assert_eq!(c.cr(owner).block, 10);
        c.fire_relics(Hook::BeforeTurnEndVeryEarly, Ev::Side(Side::Player));
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Player));
        assert_eq!(c.cr(owner).block, 10);
        for seat in 0..4 { if seat != owner { assert_eq!(c.cr(seat).block, 0); } }
        assert!(c.fault.is_none());
    }
}

#[test]
fn group_four_remnant_caps_normal_damage_for_only_owner_and_resets_on_owner_side() {
    for owner in 0..4 {
        let mut c = with_relic(owner, RelicId::BeatingRemnant);
        let enemy = c.enemies[0];
        let other = (owner + 1) % 4;
        c.deal_damage(Some(enemy), owner, 15, Props::MOVE);
        c.deal_damage(Some(enemy), owner, 15, Props::MOVE);
        assert_eq!(c.cr(owner).hp, 30);
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Enemy));
        c.deal_damage(Some(enemy), owner, 15, Props::MOVE);
        assert_eq!(c.cr(owner).hp, 30);
        c.deal_damage(Some(enemy), other, 25, Props::MOVE);
        assert_eq!(c.cr(other).hp, 25);
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        c.deal_damage(Some(enemy), owner, 5, Props::MOVE);
        assert_eq!(c.cr(owner).hp, 25);
        assert!(c.fault.is_none());
    }
}

#[test]
fn group_four_marbles_uses_owner_turn_and_kunai_counts_only_owner_attacks() {
    for owner in 0..4 {
        let mut c = with_relic(owner, RelicId::BagOfMarbles);
        c.round = 8;
        c.players[owner].turn_number = 1;
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.power_amount(c.enemies[0], PowerId::Vulnerable), 1);
        c.players[owner].turn_number = 2;
        c.round = 1;
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.power_amount(c.enemies[0], PowerId::Vulnerable), 1);
        assert!(c.fault.is_none());
        let mut c = with_relic(owner, RelicId::Kunai);
        for seat in 0..4 {
            let k = c.new_card(seat, CardId::StrikeIronclad, false).unwrap();
            for _ in 0..3 { c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k)); }
        }
        assert_eq!(c.power_amount(owner, PowerId::Dexterity), 1);
        assert_eq!(c.run.relics[0].counter, 3);
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Enemy));
        assert_eq!(c.run.relics[0].counter, 3);
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        c.is_over = true;
        let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        assert_eq!(c.run.relics[0].counter, 0);
        assert!(c.fault.is_none());
    }
}

#[test]
fn group_four_meat_threshold_is_owner_hp_living_and_rounded_down() {
    for owner in 0..4 {
        for (hp, expected) in [(0, 0), (25, 37), (26, 26)] {
            let mut c = with_relic(owner, RelicId::MeatOnTheBone);
            c.cr_mut(owner).max_hp = 51;
            c.cr_mut(owner).hp = hp;
            c.fire_relics(Hook::AfterCombatVictoryEarly, Ev::None);
            assert_eq!(c.cr(owner).hp, expected);
            for seat in 0..4 { if seat != owner { assert_eq!(c.cr(seat).hp, 50); } }
            assert!(c.fault.is_none());
        }
        let mut c = with_relic(owner, RelicId::MeatOnTheBone);
        c.cr_mut(owner).hp = 20;
        c.players[owner].hooks_active = false;
        c.fire_relics(Hook::AfterCombatVictoryEarly, Ev::None);
        assert_eq!(c.cr(owner).hp, 20);
    }
}
