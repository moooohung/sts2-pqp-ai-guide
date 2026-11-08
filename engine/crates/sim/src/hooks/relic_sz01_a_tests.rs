use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

#[test]
fn sword_of_stone_counts_only_elite_victories_and_keeps_owner_when_replaced() {
    for owner in 0..4 {
        let mut c = combat();
        let mut r = Relic::new(RelicId::SwordOfStone); r.owner = owner; r.counter = 4;
        c.run.relics.push(r);
        for room in [RoomType::Monster, RoomType::Boss] {
            c.run.room_type = room;
            c.fire_relics(Hook::AfterCombatVictory, Ev::None);
            assert_eq!(c.run.relics[0].counter, 4);
        }
        c.run.room_type = RoomType::Elite;
        c.fire_relics(Hook::AfterCombatVictory, Ev::None);
        assert_eq!(c.run.relics[0].id, RelicId::SwordOfJade);
        assert_eq!(c.run.relics[0].owner, owner);
        assert_eq!(c.run.relics[0].counter, 0);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn seal_of_gold_spends_only_owners_supplied_gold_and_refuses_missing_state() {
    for owner in 0..4 {
        for gold in [0, 2, 3, 6] {
            let mut c = combat();
            let mut r = Relic::new(RelicId::SealOfGold); r.owner = owner;
            c.run.relics.push(r); c.run.gold = if owner == 0 { gold } else { 99 };
            for seat in 0..4 { c.players[seat].energy = 0; c.players[seat].gold = Some(gold); }
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Enemy));
            assert_eq!(c.players[owner].energy, 0);
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
            for seat in 0..4 {
                assert_eq!(c.players[seat].energy, i32::from(seat == owner && gold >= 3));
            }
            assert_eq!(if owner == 0 { c.run.gold } else { c.players[owner].gold.unwrap() },
                gold - if gold >= 3 { 3 } else { 0 });
            if owner != 0 { assert_eq!(c.run.gold, 99); }
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
    let mut c = combat(); c.run.gold = 99;
    let mut r = Relic::new(RelicId::SealOfGold); r.owner = 1;
    c.run.relics.push(r); c.players[1].gold = None;
    c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
    assert_eq!(c.fault, Some("relic owner initial gold not supplied"));
    assert_eq!(c.run.gold, 99);
}

#[test]
fn sz01_equipped_rest_reward_and_pickup_relics_never_replay_on_combat_entry() {
    for owner in 0..4 {
        let mut c = combat(); c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        for id in [RelicId::TinyMailbox, RelicId::SilkenTress, RelicId::ScrollBoxes,
            RelicId::WarPaint, RelicId::YummyCookie, RelicId::StoneHumidifier] {
            let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
        }
        let gold = c.run.gold;
        let max_hp = c.cr(owner).max_hp;
        c.start_combat();
        assert_eq!(c.run.gold, gold);
        assert_eq!(c.cr(owner).max_hp, max_hp);
        assert!(c.relic_owner_deck(owner).is_empty());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
