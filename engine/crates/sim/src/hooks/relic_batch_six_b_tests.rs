use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};
fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat();
    let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    c
}
#[test]
fn b6_energy_cost_and_cocoa_follow_owner_not_round() {
    for owner in 0..4 {
        for id in [RelicId::PrismaticGem, RelicId::SpikedGauntlets] {
            let mut c = setup(owner, id);
            for seat in 0..4 { assert_eq!(c.max_energy(seat), if seat == owner { 4 } else { 3 }); }
            let k = c.new_card(owner, CardId::Inflame, false).unwrap();
            c.players[owner].hand.push(k); c.players[owner].energy = 2;
            // Existing global cost API implements SpikedGauntlets; no extra
            // ShouldPlay gate may override a final absolute zero-cost modifier.
            assert_eq!(c.modified_card_cost(owner, k), if id == RelicId::SpikedGauntlets { 2 } else { 1 });
            assert!(c.can_play_card(k));
            assert!(c.fault.is_none());
        }
        let mut c = setup(owner, RelicId::VeryHotCocoa);
        c.round = 9; c.players[owner].turn_number = 1;
        let before = c.players[owner].energy;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].energy, before + 4);
        c.round = 1; c.players[owner].turn_number = 2;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].energy, before + 4);
        assert!(c.fault.is_none());
    }
}
#[test]
fn b6_pantograph_heals_only_living_owner_in_boss_room() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::Pantograph);
        for seat in 0..4 { c.cr_mut(seat).hp = 10; }
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.cr(owner).hp, 10);
        c.run.room_type = RoomType::Boss;
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        for seat in 0..4 { assert_eq!(c.cr(seat).hp, if seat == owner { 35 } else { 10 }); }
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.cr(owner).hp, 50);
        c.cr_mut(owner).hp = 0;
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.cr(owner).hp, 0);
        assert!(c.fault.is_none());
    }
}
#[test]
fn b6_run_only_relics_are_neutral_and_ticket_counts_each_combat_end() {
    for owner in 0..4 {
        for id in [RelicId::Shovel, RelicId::SignetRing, RelicId::SeaGlass] {
            let mut c = setup(owner, id); c.run.gold = 99;
            let cards = c.cards.len();
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            c.fire_relics(Hook::AfterCombatEnd, Ev::None);
            assert_eq!(c.run.gold, 99); assert_eq!(c.cards.len(), cards);
            assert!(c.fault.is_none());
        }
        let mut c = setup(owner, RelicId::WongosMysteryTicket);
        c.run.relics[0].counter = 4;
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert_eq!(c.run.relics[0].counter, 5);
        assert!(c.run.relics[0].enabled);
        assert!(c.fault.is_none());
    }
}
