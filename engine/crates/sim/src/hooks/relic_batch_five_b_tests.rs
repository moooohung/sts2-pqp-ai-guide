use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(seats: usize, owner: CId, id: RelicId) -> Combat {
    let mut c = Combat::new(combat().run, 1);
    for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
    let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    for e in c.enemies.clone() { c.cr_mut(e).hp = 200; c.cr_mut(e).max_hp = 200; }
    c
}

#[test]
fn b5_acquisition_only_relics_do_not_repeat_pickup_effects_in_combat() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        for id in [RelicId::PrecariousShears, RelicId::PaelsGrowth, RelicId::Whetstone,
            RelicId::PandorasBox, RelicId::NeowsTalisman, RelicId::PotionBelt] {
            let mut c = setup(seats, owner, id);
            c.cr_mut(owner).hp = 25;
            let niche = c.run.rngs.niche.counter;
            let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
            c.players[owner].hand.push(k);
            for hook in [Hook::BeforeCombatStart, Hook::BeforeCombatStartLate,
                Hook::AfterCombatVictory, Hook::AfterCombatEnd] {
                c.fire_relics(hook, Ev::None);
            }
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
            assert_eq!((c.cr(owner).hp, c.cr(owner).max_hp), (25, 50));
            assert_eq!(c.run.rngs.niche.counter, niche);
            assert_eq!(c.players[owner].hand.as_slice(), &[k]);
            assert_eq!(c.cards[k as usize].id, CardId::StrikeIronclad);
            assert!(!c.cards[k as usize].upgraded);
            assert!(c.fault.is_none());
        }
    }}
}

#[test]
fn b5_drake_bonus_tracks_owner_turn_three_not_global_round() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        let mut c = setup(seats, owner, RelicId::RingOfTheDrake);
        c.round = 10; c.players[owner].turn_number = 3;
        for seat in 0..seats as CId {
            assert_eq!(c.modify_hand_draw(5, seat), if seat == owner { 7 } else { 5 });
        }
        c.round = 1; c.players[owner].turn_number = 4;
        assert_eq!(c.modify_hand_draw(5, owner), 5);
        assert!(c.fault.is_none());
    }}
}

#[test]
fn b5_earring_autoplays_once_at_owners_first_phase_and_spends_only_owner_energy() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        let mut c = setup(seats, owner, RelicId::WhisperingEarring);
        let other = (owner + 1) % seats as CId;
        c.round = 9; c.players[owner].turn_number = 1;
        let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        c.players[owner].hand.push(k); c.players[owner].energy = 3;
        c.players[other].energy = 3;
        c.fire_relics(Hook::BeforePlayPhaseStart, Ev::Creature(other));
        assert_eq!(c.players[owner].hand.len(), 1);
        let enemy = c.enemies[0]; let hp = c.cr(enemy).hp;
        c.fire_relics(Hook::BeforePlayPhaseStart, Ev::Creature(owner));
        assert_eq!(c.cr(enemy).hp, hp - 6);
        assert_eq!(c.players[owner].energy, 2); assert_eq!(c.players[other].energy, 3);
        assert_eq!(c.max_energy(owner), 4); assert_eq!(c.max_energy(other), 3);
        let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        c.players[owner].hand.push(k);
        c.round = 1; c.players[owner].turn_number = 2;
        c.fire_relics(Hook::BeforePlayPhaseStart, Ev::Creature(owner));
        assert_eq!(c.players[owner].hand.len(), 1);
        assert!(c.fault.is_none());
    }}
}
