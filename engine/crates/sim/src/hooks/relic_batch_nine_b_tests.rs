use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};
#[test]
fn b9_pumpkin_energy_and_countdown_belong_to_each_seat() {
    for owner in 0..4 {
        let mut c = combat();
        let mut r = Relic::new(RelicId::PumpkinCandle); r.owner = owner; r.counter = 2;
        c.run.relics.push(r);
        c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
        for seat in 0..4 { assert_eq!(c.max_energy(seat), if seat == owner { 4 } else { 3 }); }
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert_eq!(c.run.relics[0].counter, 1);
        assert_eq!(c.max_energy(owner), 4);
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert_eq!(c.run.relics[0].counter, 0);
        assert!(!c.run.relics[0].enabled);
        assert_eq!(c.max_energy(owner), 3);
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert_eq!(c.run.relics[0].counter, 0);
        assert!(c.fault.is_none());
    }
}
#[test]
fn b9_equipped_pickup_relics_do_not_mutate_owner_or_teammates_during_combat() {
    for owner in 0..4 {
        for id in [RelicId::NeowsSacrifice, RelicId::Strawberry, RelicId::PunchDagger,
            RelicId::ToxicEgg, RelicId::SereTalon, RelicId::TanxsWhistle, RelicId::NeowsTorment] {
            let mut c = combat();
            let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
            c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
            for seat in 0..4 { c.cr_mut(seat).hp = 25; }
            let niche = c.run.rngs.niche.counter;
            for hook in [Hook::BeforeCombatStart, Hook::AfterCombatVictory, Hook::AfterCombatEnd] {
                c.fire_relics(hook, Ev::None);
            }
            let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.add_generated_card(k, PileKind::Hand, PilePos::Bottom, true);
            assert!(!c.cards[k as usize].upgraded, "ToxicEgg is not a combat-card upgrade");
            assert!(c.cards[k as usize].enchantments.is_empty());
            for seat in 0..4 { assert_eq!((c.cr(seat).hp, c.cr(seat).max_hp), (25,50)); }
            assert_eq!(c.run.rngs.niche.counter, niche);
            assert_eq!(c.cards.len(), 1);
            assert!(c.fault.is_none());
        }
    }
}
