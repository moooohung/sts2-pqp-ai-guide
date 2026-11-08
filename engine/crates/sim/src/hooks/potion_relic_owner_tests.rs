//! BeltBuckle.cs inventories belong to each listener, not the potion event owner.
//! ReptileTrinket.cs alone guards potion.Owner == Owner.
use super::*;
use super::engine_tests::combat;
fn setup() -> Combat {
    let mut c = combat();
    c.run.potions = vec![None; 3];
    for owner in 0..4 {
        if owner > 0 { c.players[owner].potions = Some(vec![None; 3]); }
        for id in [RelicId::BeltBuckle, RelicId::ReptileTrinket] {
            let mut r = crate::relic::Relic::new(id); r.owner = owner; c.run.relics.push(r);
        }
    }
    c
}
#[test]
fn potion_relics_track_each_seats_inventory_and_only_the_users_strength() {
    for user in 0..4 {
        let mut c = setup();
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        for seat in 0..4 { assert_eq!(c.power_amount(seat, PowerId::Dexterity), 2); }
        c.relic_owner_potions(user).unwrap()[0] = Some(PotionId::Strengthpotion);
        c.fire_relic_after_potion_procured(user);
        for seat in 0..4 { assert_eq!(c.power_amount(seat, PowerId::Dexterity), if seat == user { 0 } else { 2 }); }
        c.relic_owner_potions(user).unwrap()[0] = None;
        c.fire_relic_after_potion_used(user, PotionId::Strengthpotion, None);
        for seat in 0..4 {
            assert_eq!(c.power_amount(seat, PowerId::Dexterity), 2);
            assert_eq!(c.power_amount(seat, PowerId::Strength), if seat == user { 3 } else { 0 });
        }
        assert!(c.fault.is_none(), "user {user}: {:?}", c.fault);
    }
}
#[test]
fn buckle_checks_its_own_inventory_even_for_another_players_potion_event() {
    for owner in 0..4 {
        let mut c = setup();
        let other = (owner + 1) % 4;
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        c.relic_owner_potions(owner).unwrap()[0] = Some(PotionId::Strengthpotion);
        c.fire_relic_after_potion_procured(other);
        assert_eq!(c.power_amount(owner, PowerId::Dexterity), 0);
        c.relic_owner_potions(owner).unwrap()[0] = None;
        c.fire_relic_after_potion_discarded(other);
        assert_eq!(c.power_amount(owner, PowerId::Dexterity), 2);
        c.fire_relic_after_potion_discarded(other);
        assert_eq!(c.power_amount(owner, PowerId::Dexterity), 2);
        c.players[owner].hooks_active = false;
        c.fire_relic_after_potion_used(owner, PotionId::Strengthpotion, None);
        assert_eq!(c.power_amount(owner, PowerId::Strength), 0);
        assert!(c.fault.is_none());
    }
}
