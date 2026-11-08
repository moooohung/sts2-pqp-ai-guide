use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng, orbs::OrbKind};

fn setup(seats: usize, owner: CId, id: RelicId) -> Combat {
    let mut c = Combat::new(combat().run, 1);
    for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
    let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    c
}

#[test]
fn b3_pollinous_counts_only_owners_hand_draw_and_resets_after_bonus() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        let mut c = setup(seats, owner, RelicId::PollinousCore);
        let other = (owner + 1) % seats as CId;
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(other));
        assert_eq!(c.run.relics[0].counter, 0);
        c.run.relics[0].counter = 3;
        c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(owner));
        assert_eq!(c.modify_hand_draw(5, other), 5);
        assert_eq!(c.run.relics[0].counter, 4);
        assert_eq!(c.modify_hand_draw(5, owner), 7);
        assert_eq!(c.run.relics[0].counter, 0);
        c.run.relics[0].counter = 5;
        assert_eq!(c.modify_hand_draw(5, owner), 7);
        assert!(c.fault.is_none());
    }}
}

#[test]
fn b3_choker_limits_only_owner_and_resets_on_combat_entry_and_turn() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        let mut c = setup(seats, owner, RelicId::VelvetChoker);
        let other = (owner + 1) % seats as CId;
        let own = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        let foreign = c.new_card(other, CardId::DefendIronclad, false).unwrap();
        for _ in 0..7 { c.fire_relics(Hook::AfterCardPlayed, Ev::Card(foreign)); }
        assert_eq!(c.run.relics[0].counter, 0);
        c.players[owner].hand.push(own); c.players[other].hand.push(foreign);
        c.players[owner].energy = 3; c.players[other].energy = 3;
        for _ in 0..6 { c.fire_relics(Hook::AfterCardPlayed, Ev::Card(own)); }
        assert!(!c.can_play_card(own)); assert!(c.can_play_card(foreign));
        assert_eq!(c.max_energy(owner), 4); assert_eq!(c.max_energy(other), 3);
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        assert!(c.can_play_card(own));
        c.run.relics[0].counter = 6;
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.run.relics[0].counter, 0);
        assert!(c.fault.is_none());
    }}
}

#[test]
fn b3_funnel_and_virus_read_owner_turn_number_not_round() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        for id in [RelicId::TwistedFunnel, RelicId::SymbioticVirus] {
            let mut c = setup(seats, owner, id);
            c.round = 9; c.players[owner].turn_number = 1;
            c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Enemy));
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Enemy));
            for &e in &c.enemies { assert_eq!(c.power_amount(e, PowerId::Poison), 0); }
            assert_eq!(c.count_orbs(owner, OrbKind::Dark), 0);
            c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
            for &e in &c.enemies { assert_eq!(c.power_amount(e, PowerId::Poison), if id == RelicId::TwistedFunnel { 4 } else { 0 }); }
            for seat in 0..seats as CId {
                assert_eq!(c.count_orbs(seat, OrbKind::Dark), i32::from(id == RelicId::SymbioticVirus && seat == owner));
            }
            c.round = 1; c.players[owner].turn_number = 2;
            c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
            for &e in &c.enemies { assert_eq!(c.power_amount(e, PowerId::Poison), if id == RelicId::TwistedFunnel { 4 } else { 0 }); }
            assert!(c.fault.is_none());
        }
    }}
}

#[test]
fn b3_vitruvian_uses_owned_minion_source_for_damage_and_ally_block() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        let mut c = setup(seats, owner, RelicId::VitruvianMinion);
        let other = (owner + 1) % seats as CId; let enemy = c.enemies[0];
        let own = c.new_card(owner, CardId::MinionStrike, false).unwrap();
        let foreign = c.new_card(other, CardId::MinionStrike, false).unwrap();
        let normal = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        for (k, expected) in [(foreign, 6), (normal, 6), (own, 12)] {
            c.active_card = Some(k);
            assert_eq!(c.calculate_damage(6, Some(owner), enemy, Props::MOVE), expected);
            assert_eq!(c.calculate_block(6, other, Props::UNPOWERED, Some(k)), expected);
        }
        assert!(c.fault.is_none());
    }}
}

#[test]
fn b3_sigil_halves_only_incoming_powered_attacks_from_doomed_dealer() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        let mut c = setup(seats, owner, RelicId::UndyingSigil);
        let other = (owner + 1) % seats as CId; let enemy = c.enemies[0];
        c.cr_mut(enemy).hp = 20;
        c.apply_power_to(enemy, PowerId::Doom, 20, Some(owner));
        assert_eq!(c.calculate_damage(10, Some(enemy), owner, Props::MOVE), 5);
        assert_eq!(c.calculate_damage(10, Some(enemy), other, Props::MOVE), 10);
        assert_eq!(c.calculate_damage(10, Some(enemy), owner, Props::UNPOWERED), 10);
        c.cr_mut(enemy).hp = 21;
        assert_eq!(c.calculate_damage(10, Some(enemy), owner, Props::MOVE), 10);
        assert!(c.fault.is_none());
    }}
}

#[test]
fn b3_discard_relics_filter_owner_and_side_and_use_combat_target_rng() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        for id in [RelicId::Tingsha, RelicId::ToughBandages] {
            let mut c = setup(seats, owner, id); let other = (owner + 1) % seats as CId;
            for &e in &c.enemies.clone() { c.cr_mut(e).hp = 200; }
            let foreign = c.new_card(other, CardId::DefendIronclad, false).unwrap();
            let own = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            c.players[other].hand.push(foreign);
            c.players[owner].hand.push(own);
            c.discard_cards(other, &[foreign], 0);
            assert_eq!(c.cr(owner).block, 0);
            for &e in &c.enemies { assert_eq!(c.cr(e).hp, 200); }
            let mut rng = c.run.rngs.combat_targets.clone();
            let enemies = c.hittable_enemies();
            let target = enemies[rng.choice_index(enemies.len())];
            c.discard_cards(owner, &[own], 0);
            assert_eq!(c.cr(owner).block, if id == RelicId::ToughBandages { 3 } else { 0 });
            for &e in &c.enemies { assert_eq!(c.cr(e).hp, if id == RelicId::Tingsha && e == target { 197 } else { 200 }); }
            if id == RelicId::Tingsha { assert_eq!(c.run.rngs.combat_targets.counter, rng.counter); }
            let before = c.run.rngs.combat_targets.counter;
            c.side = Side::Enemy;
            c.players[owner].hand.push(own);
            c.discard_cards(owner, &[own], 0);
            assert_eq!(c.run.rngs.combat_targets.counter, before);
            assert_eq!(c.cr(owner).block, if id == RelicId::ToughBandages { 3 } else { 0 });
            assert!(c.fault.is_none());
        }
    }}
}

#[test]
fn b3_hand_flush_is_not_a_cardcmd_discard() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        let mut c = setup(seats, owner, RelicId::Tingsha);
        c.start_combat();
        let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        c.players[owner].hand.push(k);
        let before: Vec<_> = c.enemies.iter().map(|&e| c.cr(e).hp).collect();
        c.end_player_turn();
        let after: Vec<_> = c.enemies.iter().map(|&e| c.cr(e).hp).collect();
        assert_eq!(after, before);
        assert!(c.fault.is_none());
    }}
}
