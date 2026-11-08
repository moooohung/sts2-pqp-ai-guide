use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

#[test]
fn wide09_owner_turn_and_elite_room_control_conch_and_destiny() {
    for owner in 0..4 {
        for elite in [false, true] {
            let mut c = combat();
            c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
            for id in [RelicId::DivineDestiny, RelicId::BoomingConch] {
                let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
            }
            c.run.room_type = if elite { RoomType::Elite } else { RoomType::Monster };
            c.round = 99; c.players[owner].turn_number = 1;
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
            for seat in 0..4 {
                assert_eq!(c.players[seat].stars, if seat == owner { 7 } else { 0 });
                assert_eq!(c.players[seat].energy, i32::from(elite && seat == owner));
            }
            assert_eq!(c.relic_modify_hand_draw(1, owner, 5, false), if elite { 7 } else { 5 });
            c.round = 1; c.players[owner].turn_number = 2;
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
            assert_eq!(c.players[owner].stars, 7);
            assert_eq!(c.relic_modify_hand_draw(1, owner, 5, false), 5);
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}

#[test]
fn wide09_equipped_pickup_reward_models_do_not_synthesize_acquisition() {
    for owner in 0..4 {
        let mut c = combat();
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        for id in [RelicId::ArcaneScroll, RelicId::DowsingRod, RelicId::AlchemicalCoffer,
            RelicId::DollysMirror, RelicId::Cauldron, RelicId::BlackStar] {
            let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
        }
        let slots = c.run.max_potion_slots;
        c.start_combat();
        assert!(c.relic_owner_deck(owner).is_empty());
        assert!(c.players[owner].hand.is_empty());
        assert_eq!(c.run.max_potion_slots, slots);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
