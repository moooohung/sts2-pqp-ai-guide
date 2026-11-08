//! Models/Relics/<Name>.cs, A-C census group c1.
use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat();
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    for e in c.enemies.clone() { c.cr_mut(e).hp = 200; c.cr_mut(e).max_hp = 200; }
    let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
    c
}

#[test]
fn c1_bone_tea_upgrades_only_drawn_owner_hand_once_on_owner_first_turn() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::BoneTea);
        let other = (owner + 1) % 4;
        let own = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        let foreign = c.new_card(other, CardId::StrikeIronclad, false).unwrap();
        c.players[owner].hand.push(own); c.players[other].hand.push(foreign);
        c.round = 9; c.players[owner].turn_number = 1;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(other));
        assert!(!c.cards[own as usize].upgraded);
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
        assert!(c.cards[own as usize].upgraded);
        assert!(!c.cards[foreign as usize].upgraded);
        assert_eq!(c.run.relics[0].counter, 1);
        let later = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        c.players[owner].hand.push(later);
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
        assert!(!c.cards[*c.players[owner].hand.last().unwrap() as usize].upgraded);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn c1_bread_and_brimstone_obey_owner_turn_participation() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::Bread);
        let other = (owner + 1) % 4;
        c.round = 9; c.players[owner].turn_number = 1;
        for p in 0..4 { c.players[p].energy = 3; }
        assert_eq!(c.max_energy(owner), 3);
        c.extra_turn_players = 1 << other;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].energy, 3);
        c.extra_turn_players = 1 << owner;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].energy, 1);
        assert_eq!(c.players[other].energy, 3);
        c.round = 1; c.players[owner].turn_number = 2;
        assert_eq!(c.max_energy(owner), 4);
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].energy, 1);
        assert!(c.fault.is_none());
        let mut c = setup(owner, RelicId::Brimstone);
        c.extra_turn_players = 1 << other;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.power_amount(owner, PowerId::Strength), 0);
        c.extra_turn_players = 1 << owner;
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.power_amount(owner, PowerId::Strength), 2);
        for p in 0..4 { if p != owner { assert_eq!(c.power_amount(p, PowerId::Strength), 0); } }
        for &e in &c.enemies {
            assert_eq!(c.power_amount(e, PowerId::Strength), 1);
            assert_eq!(c.powers[c.find_power(e, PowerId::Strength).unwrap() as usize].applier, None);
        }
        assert!(c.fault.is_none());
    }
}

#[test]
fn c1_cheese_and_shuffle_generation_only_affect_owner() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::ChosenCheese);
        c.cr_mut(owner).hp = 30;
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert_eq!((c.cr(owner).hp, c.cr(owner).max_hp), (31, 51));
        for p in 0..4 { if p != owner { assert_eq!(c.cr(p).max_hp, 50); } }
        let mut c = setup(owner, RelicId::BiiigHug);
        c.fire_relic_after_shuffle((owner + 1) % 4);
        assert!(c.players[owner].draw.is_empty());
        c.fire_relic_after_shuffle(owner);
        assert_eq!(c.players[owner].draw.len(), 1);
        let k = c.players[owner].draw[0];
        assert_eq!((c.cards[k as usize].id, c.cards[k as usize].owner), (CardId::Soot, owner));
        assert!(c.fault.is_none());
    }
}

#[test]
fn c1_knife_heals_owner_after_doom_list_excluding_minions() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::BookRepairKnife);
        c.cr_mut(owner).hp = 30;
        let first = c.enemies[0]; let minion = c.enemies[1];
        c.apply_power_to(minion, PowerId::Minion, 1, None);
        c.doom_kill(&[first, minion]);
        assert_eq!(c.cr(owner).hp, 33);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn c1_pickup_only_models_preserve_imported_decks_and_rng() {
    for owner in 0..4 { for id in [RelicId::Astrolabe, RelicId::CursedPearl] {
        let mut c = setup(owner, id);
        assert!(crate::relic::is_ported(id));
        assert!(c.relic_owner_has_enabled(owner, id));
        let before = (c.run.deck.len(), c.run.gold, c.run.rngs.niche.counter, c.run.next_card_id);
        for hook in [Hook::BeforeCombatStart, Hook::AfterCombatVictory, Hook::AfterCombatEnd] {
            c.fire_relics(hook, Ev::None);
        }
        assert_eq!(before, (c.run.deck.len(), c.run.gold, c.run.rngs.niche.counter, c.run.next_card_id));
        assert!(c.fault.is_none());
    }}
}
