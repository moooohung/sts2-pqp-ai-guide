use super::*;
use super::engine_tests::combat;
use crate::rng::Rng;

fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat();
    let mut r = crate::relic::Relic::new(id);
    r.owner = owner;
    c.run.relics.push(r);
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    c
}

#[test]
fn b1_strength_relics_target_only_owner() {
    for owner in 0..4 {
        for (id, strength) in [(RelicId::SlingOfCourage, 2), (RelicId::SwordOfJade, 3)] {
            let mut c = setup(owner, id);
            c.run.room_type = RoomType::Elite;
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            for seat in 0..4 {
                assert_eq!(c.power_amount(seat, PowerId::Strength), if seat == owner { strength } else { 0 });
            }
            assert!(c.fault.is_none());
        }
        let mut c = setup(owner, RelicId::SlingOfCourage);
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.power_amount(owner, PowerId::Strength), 0);
        assert!(c.fault.is_none());
    }
}

#[test]
fn b1_tea_inserts_two_owned_dazed_once() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::TeaOfDiscourtesy);
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        for seat in 0..4 {
            assert_eq!(c.players[seat].draw.len(), if seat == owner { 2 } else { 0 });
        }
        for &k in &c.players[owner].draw {
            assert_eq!(c.cards[k as usize].owner, owner);
            assert_eq!(c.cards[k as usize].id, CardId::Dazed);
        }
        assert!(!c.run.relics[0].enabled);
        assert!(c.fault.is_none());
    }
}

#[test]
fn b1_stone_buffs_opponents_and_only_owners_energy() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::PhilosophersStone);
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        for seat in 0..4 {
            assert_eq!(c.max_energy(seat), if seat == owner { 4 } else { 3 });
            c.fire_relics(Hook::AfterCreatureAddedToCombat, Ev::Creature(seat));
            assert_eq!(c.power_amount(seat, PowerId::Strength), 0);
        }
        let e = c.enemies[0];
        assert_eq!(c.power_amount(e, PowerId::Strength), 1);
        c.fire_relics(Hook::AfterCreatureAddedToCombat, Ev::Creature(e));
        assert_eq!(c.power_amount(e, PowerId::Strength), 2);
        assert!(c.fault.is_none());
    }
}

#[test]
fn b1_axe_duplicates_first_owner_card_not_teammates() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::ThrowingAxe);
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        let enemy = c.enemies[0];
        c.cr_mut(enemy).hp = 200;
        c.cr_mut(enemy).max_hp = 200;
        for (seat, expected) in [((owner + 1) % 4, 6), (owner, 12), (owner, 6)] {
            let k = c.new_card(seat, CardId::StrikeIronclad, false).unwrap();
            c.players[seat].hand.push(k);
            c.players[seat].energy = 3;
            let hp = c.cr(enemy).hp;
            assert!(c.play_card(seat, 0, Some(0)));
            assert_eq!(hp - c.cr(enemy).hp, expected);
        }
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert!(!c.run.relics[0].flag1);
        assert!(c.fault.is_none());
    }
}
