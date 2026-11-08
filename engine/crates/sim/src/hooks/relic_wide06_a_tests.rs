//! Source-audited equipped snapshots: pickup/map/reward effects are not replayed.
use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

const PASSIVE: &[RelicId] = &[RelicId::ArchaicTooth, RelicId::FresnelLens,
    RelicId::EmptyCage, RelicId::GoldenCompass, RelicId::DarkstonePeriapt,
    RelicId::DistinguishedCape, RelicId::FakeMango];

#[test]
fn equipped_only_relics_leave_owner_combat_state_unchanged_in_all_seats() {
    for owner in 0..4 {
        let mut c = combat();
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        for &id in PASSIVE {
            assert!(crate::relic::is_ported(id));
            let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
        }
        c.start_combat();
        for seat in 0..4 {
            assert_eq!(c.cr(seat).max_hp, 50); // FakeMango pickup not synthesized.
            assert!(c.players[seat].hand.is_empty());
            assert!(c.relic_owner_deck(seat).is_empty());
            assert_eq!(c.max_energy(seat), 3);
        }
        let curse = c.new_card(owner, CardId::Doubt, false).unwrap();
        c.add_generated_card_by(curse, PileKind::Hand, PilePos::Bottom, Some(owner));
        assert_eq!(c.cr(owner).max_hp, 50); // Darkstone only reacts to Deck additions.
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn black_blood_heals_living_owner_only_and_caps_twelve_at_max_hp() {
    for owner in 0..4 {
        for hp in [0, 20, 45] {
            let mut c = combat();
            let mut r = Relic::new(RelicId::BlackBlood); r.owner = owner; c.run.relics.push(r);
            c.cr_mut(owner).hp = hp;
            c.fire_relics(Hook::AfterCombatVictory, Ev::None);
            assert_eq!(c.cr(owner).hp, if hp == 0 { 0 } else { (hp + 12).min(50) });
            for seat in 0..4 {
                if seat != owner { assert_eq!(c.cr(seat).hp, 50); }
            }
            assert!(c.fault.is_none(), "{:?}", c.fault);
        }
    }
}
