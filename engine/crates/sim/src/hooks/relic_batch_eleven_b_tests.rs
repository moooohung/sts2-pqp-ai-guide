use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat();
    let mut relic = Relic::new(id);
    relic.owner = owner;
    c.run.relics.push(relic);
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    c
}

#[test]
fn b11_flesh_changes_owner_maximum_not_side_start_energy() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::PaelsFlesh);
        for (round, turn, expected) in [(9, 1, 3), (9, 2, 3), (1, 3, 4), (1, 4, 4)] {
            c.round = round;
            for seat in 0..4 { c.players[seat].turn_number = 8; c.players[seat].energy = 0; }
            c.players[owner].turn_number = turn;
            for seat in 0..4 {
                assert_eq!(c.max_energy(seat), if seat == owner { expected } else { 3 });
            }
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
            for seat in 0..4 { assert_eq!(c.players[seat].energy, 0); }
        }
        c.players[owner].turn_number = 2;
        c.extra_turn_players = 1 << owner;
        c.start_player_turn();
        assert_eq!(c.players[owner].turn_number, 3);
        assert_eq!(c.players[owner].energy, 4);
        for seat in 0..4 {
            if seat != owner { assert_eq!(c.players[seat].energy, 0); }
        }
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn b11_poison_uses_owner_first_turn_instead_of_shared_round() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::RoyalPoison);
        c.round = 8;
        for seat in 0..4 { c.players[seat].turn_number = 1; }
        for seat in 0..4 { c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(seat)); }
        for seat in 0..4 { assert_eq!(c.cr(seat).hp, if seat == owner { 46 } else { 50 }); }
        c.round = 1;
        c.players[owner].turn_number = 2;
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
        assert_eq!(c.cr(owner).hp, 46);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn b11_noncombat_models_preserve_imported_owner_and_teammate_state() {
    for owner in 0..4 {
        for id in [RelicId::PrayerWheel, RelicId::Orrery, RelicId::Pear,
            RelicId::PreciseScissors, RelicId::PaelsHorn, RelicId::PreservedFog] {
            let mut c = setup(owner, id);
            for seat in 0..4 {
                c.cr_mut(seat).hp = 25;
                let k = c.new_card(seat, CardId::StrikeIronclad, false).unwrap();
                let card = c.cards[k as usize].clone();
                c.relic_owner_deck_mut(seat).push(card);
            }
            let niche = c.run.rngs.niche.counter;
            for hook in [Hook::BeforeCombatStart, Hook::AfterCombatVictory, Hook::AfterCombatEnd] {
                c.fire_relics(hook, Ev::None);
            }
            for seat in 0..4 {
                assert_eq!((c.cr(seat).hp, c.cr(seat).max_hp), (25, 50));
                assert_eq!(c.relic_owner_deck(seat).len(), 1);
                assert_eq!(c.relic_owner_deck(seat)[0].id, CardId::StrikeIronclad);
                assert!(!c.relic_owner_deck(seat)[0].upgraded);
            }
            assert_eq!(c.cards.len(), 4);
            assert_eq!(c.run.rngs.niche.counter, niche);
            assert!(c.fault.is_none(), "{id:?}: {:?}", c.fault);
        }
    }
}
