use crate::character::Character;
use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng, orbs::{Orb, OrbKind}};

fn setup(owner: CId, ids: &[RelicId]) -> Combat {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    for &id in ids {
        let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
    }
    c
}

#[test]
fn wide03_infused_and_disk_use_owner_turn_and_lightning_values_after_focus() {
    for owner in 0..4 {
        let mut c = setup(owner, &[RelicId::InfusedCore, RelicId::DataDisk]);
        c.players[owner].orbs.open_base_slots(3);
        c.round = 99; c.players[owner].turn_number = 1;
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].orbs.orbs.len(), 3);
        for seat in 0..4 {
            let orb = Orb::new(OrbKind::Lightning);
            assert_eq!(c.orb_passive_value(seat, orb), if seat == owner { 5 } else { 3 });
            assert_eq!(c.orb_evoke_value(seat, orb), if seat == owner { 10 } else { 8 });
        }
        c.apply_power_from(owner, PowerId::Focus, -100, None, None);
        assert_eq!(c.orb_passive_value(owner, Orb::new(OrbKind::Lightning)), 1);
        c.players[owner].orbs.orbs.clear(); c.players[owner].turn_number = 2;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        assert!(c.players[owner].orbs.orbs.is_empty());
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide03_cables_repeat_only_front_owner_passives_including_explicit_triggers() {
    for owner in 0..4 {
        let mut c = setup(owner, &[RelicId::GoldPlatedCables]);
        for seat in 0..4 {
            c.players[seat].orbs.open_base_slots(3);
            c.channel_orb(seat, OrbKind::Frost); c.channel_orb(seat, OrbKind::Frost);
            c.trigger_orb_passives(seat, false);
            assert_eq!(c.cr(seat).block, if seat == owner { 6 } else { 4 });
        }
        c.orb_passive_at(owner, 0);
        assert_eq!(c.cr(owner).block, 10);
        c.orb_passive_at(owner, 1);
        assert_eq!(c.cr(owner).block, 12);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide03_fur_coat_marks_existing_and_later_enemies_without_act_guard() {
    for owner in 0..4 {
        let mut c = setup(owner, &[RelicId::FurCoat]);
        c.run.relics[0].set_fur_coat_coords(&[(2, 3)]).unwrap();
        c.run.coord = Some((1, 3));
        let enemy = c.enemies[0]; let hp = c.cr(enemy).hp;
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.cr(enemy).hp, hp);
        c.run.coord = Some((2, 3)); c.run.act_index = 2;
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.cr(enemy).hp, 1);
        c.cr_mut(enemy).hp = 20;
        c.fire_relics(Hook::AfterCreatureAddedToCombat, Ev::Creature(enemy));
        assert_eq!(c.cr(enemy).hp, 1);
        c.fire_relics(Hook::AfterCreatureAddedToCombat, Ev::Creature(owner));
        assert_eq!(c.cr(owner).hp, 50);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide03_fencing_and_scarf_follow_owner_not_shared_round_or_teammate() {
    for owner in 0..4 {
        let mut c = setup(owner, &[RelicId::FencingManual, RelicId::BrilliantScarf]);
        c.players[owner].turn_number = 1; c.round = 99;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        let blade = c.players[owner].hand[0];
        assert_eq!(c.cards[blade as usize].id, CardId::SovereignBlade);
        assert_eq!(c.cards[blade as usize].damage, Some(20));
        let foreign = c.new_card((owner + 1) % 4, CardId::DefendIronclad, false).unwrap();
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(foreign));
        assert_eq!(c.run.relics[1].counter, 0);
        let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        c.players[owner].hand.push(k);
        for _ in 0..4 { c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k)); }
        assert_eq!(c.relic_modify_card_cost(1, owner, k, 1, true), 0);
        assert_eq!(c.relic_modify_star_cost(1, owner, k, 2), 0);
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        assert_eq!(c.relic_modify_card_cost(1, owner, k, 1, true), 1);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide03_bookmark_only_discounts_retained_positive_local_cost_until_played() {
    for owner in 0..4 {
        let mut c = setup(owner, &[RelicId::Bookmark]);
        let own = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        let zero = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        c.cards[zero as usize].set_energy_cost(0, crate::card::CostExpiry::Combat, false);
        let foreign = c.new_card((owner + 1) % 4, CardId::DefendIronclad, false).unwrap();
        c.relic_after_flush((owner + 1) % 4, &[foreign]);
        assert_eq!(c.cards[foreign as usize].local_energy_cost(), 1);
        c.relic_after_flush(owner, &[own, zero]);
        assert_eq!(c.cards[own as usize].local_energy_cost(), 0);
        assert_eq!(c.cards[zero as usize].local_energy_cost(), 0);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn wide03_big_hat_only_generates_distinct_canonical_ethereal_owner_cards() {
    for owner in 0..4 {
        let mut c = setup(owner, &[RelicId::BigHat]);
        c.run.all_cards_unlocked = Some(true);
        c.players[owner].character = Some(Character::Necrobinder);
        c.players[owner].turn_number = 1; c.round = 99;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        let hand = &c.players[owner].hand;
        assert_eq!(hand.len(), 2);
        assert_ne!(c.cards[hand[0] as usize].id, c.cards[hand[1] as usize].id);
        for &k in hand {
            assert_eq!(c.cards[k as usize].owner, owner);
            assert!(c.cards[k as usize].keywords & KW_ETHEREAL != 0);
        }
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
