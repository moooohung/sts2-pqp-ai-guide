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
fn b7_abacus_blocks_for_owners_real_shuffle_only() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::TheAbacus);
        let other = (owner + 1) % 4;
        for seat in [other, owner] {
            let k = c.new_card(seat, CardId::DefendIronclad, false).unwrap();
            c.players[seat].discard.push(k);
        }
        c.draw_cards(other, 1, false);
        assert_eq!(c.cr(owner).block, 0);
        c.draw_cards(owner, 1, false);
        assert_eq!(c.cr(owner).block, 6);
        assert_eq!(c.cr(other).block, 0);
        assert!(c.fault.is_none());
    }
}
#[test]
fn b7_triangle_uses_owner_turn_and_toybox_melts_only_owners_first_wax() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::RingingTriangle);
        c.round = 8; c.players[owner].turn_number = 1;
        assert!(!c.should_flush(owner));
        assert!(c.should_flush((owner + 1) % 4));
        c.round = 1; c.players[owner].turn_number = 2;
        assert!(c.should_flush(owner));
        assert!(c.fault.is_none());
        let mut c = setup(owner, RelicId::ToyBox);
        for seat in [(owner + 1) % 4, owner, owner] {
            let mut r = Relic::new(RelicId::Anchor);
            r.owner = seat; r.is_wax = true; c.run.relics.push(r);
        }
        c.run.relics[0].counter = 2;
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert!(!c.run.relics[1].is_melted);
        assert!(c.run.relics[2].is_melted);
        assert!(!c.run.relics[2].enabled);
        assert!(!c.run.relics[3].is_melted);
        c.run.relics[0].counter = 14;
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert!(c.run.relics[3].is_melted);
        assert!(!c.run.relics[0].enabled);
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert_eq!(c.run.relics[0].counter, 15);
        assert!(c.fault.is_none());
    }
}
#[test]
fn b7_war_hammer_upgrades_four_owner_deck_cards_only_on_elite_victory() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::WarHammer);
        let other = (owner + 1) % 4;
        for id in [CardId::StrikeIronclad, CardId::DefendIronclad, CardId::Bash,
            CardId::Anger, CardId::Inflame, CardId::Armaments] {
            let k = c.new_card(owner, id, false).unwrap();
            let card = c.cards[k as usize].clone();
            c.relic_owner_deck_mut(owner).push(card);
        }
        let k = c.new_card(other, CardId::StrikeIronclad, false).unwrap();
        let card = c.cards[k as usize].clone();
        c.relic_owner_deck_mut(other).push(card);
        let count = |c: &Combat| c.relic_owner_deck(owner).iter().filter(|c| c.upgraded).count();
        c.fire_relics(Hook::AfterCombatVictory, Ev::None);
        assert_eq!(count(&c), 0);
        c.run.room_type = RoomType::Elite;
        let counter = c.run.rngs.niche.counter;
        c.fire_relics(Hook::AfterCombatVictory, Ev::None);
        assert_eq!(count(&c), 4);
        assert_eq!(c.run.rngs.niche.counter - counter, 5);
        assert!(c.relic_owner_deck(other).iter().all(|c| !c.upgraded));
        assert!(c.fault.is_none());
    }
}
#[test]
fn b7_noncombat_relics_preserve_imported_deck_and_hp() {
    for owner in 0..4 {
        for id in [RelicId::RegalPillow, RelicId::WingCharm, RelicId::PaelsClaw, RelicId::TriBoomerang] {
            let mut c = setup(owner, id); c.cr_mut(owner).hp = 25;
            let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
            c.players[owner].hand.push(k);
            let card = c.cards[k as usize].clone();
            c.relic_owner_deck_mut(owner).push(card);
            let niche = c.run.rngs.niche.counter;
            for hook in [Hook::BeforeCombatStart, Hook::AfterCombatVictory, Hook::AfterCombatEnd] {
                c.fire_relics(hook, Ev::None);
            }
            assert_eq!(c.cr(owner).hp, 25);
            assert!(c.cards[k as usize].enchantments.is_empty());
            assert!(!c.relic_owner_deck(owner)[0].upgraded);
            assert_eq!(c.run.rngs.niche.counter, niche);
            assert!(c.fault.is_none());
        }
    }
}
