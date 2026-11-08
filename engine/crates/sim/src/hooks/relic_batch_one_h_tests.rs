//! Models/Relics/{Metronome,LunarPastry,HelicalDart,IronClub}.cs.
use super::*;
use super::engine_tests::combat;
use crate::{orbs::OrbKind, relic::Relic, rng::Rng};

fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat();
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    for e in c.enemies.clone() {
        c.cr_mut(e).hp = 200;
        c.cr_mut(e).max_hp = 200;
    }
    let mut r = Relic::new(id);
    r.owner = owner;
    c.run.relics.push(r);
    c
}

#[test]
fn h1_metronome_counts_owner_channels_once_per_combat_without_target_rng() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::Metronome);
        let other = (owner + 1) % 4;
        c.run.relics[0].counter = 6;
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.run.relics[0].counter, 0);
        c.channel_orb(other, OrbKind::Plasma);
        assert_eq!(c.run.relics[0].counter, 0);
        let rng = c.run.rngs.combat_targets.counter;
        for _ in 0..6 { c.channel_orb(owner, OrbKind::Plasma); }
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 200); }
        c.channel_orb(owner, OrbKind::Plasma);
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 170); }
        for _ in 0..7 { c.channel_orb(owner, OrbKind::Plasma); }
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 170); }
        assert_eq!(c.run.rngs.combat_targets.counter, rng);
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert_eq!(c.run.relics[0].counter, 0);
        c.run.relics[0].enabled = false;
        c.channel_orb(owner, OrbKind::Plasma);
        assert_eq!(c.run.relics[0].counter, 0);
        c.run.relics[0].enabled = true;
        c.players[owner].hooks_active = false;
        c.channel_orb(owner, OrbKind::Plasma);
        assert_eq!(c.run.relics[0].counter, 0);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn h1_helical_dart_uses_owner_shiv_tag_and_temporary_dexterity_lifecycle() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::HelicalDart);
        let other = (owner + 1) % 4;
        let foreign = c.new_card(other, CardId::Shiv, false).unwrap();
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(foreign));
        assert_eq!(c.power_amount(owner, PowerId::Dexterity), 0);
        let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        assert_eq!(c.power_amount(owner, PowerId::Dexterity), 0);
        c.cards[k as usize].tags |= 1 << CardTag::Shiv.index();
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        assert_eq!(c.power_amount(owner, PowerId::HelicalDart), 2);
        assert_eq!(c.power_amount(owner, PowerId::Dexterity), 2);
        for seat in 0..4 {
            if seat != owner { assert_eq!(c.power_amount(seat, PowerId::Dexterity), 0); }
        }
        c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Enemy), None);
        assert_eq!(c.power_amount(owner, PowerId::Dexterity), 2);
        c.fire(Hook::AfterTurnEnd, Ev::Side(Side::Player), None);
        assert!(!c.has_power(owner, PowerId::HelicalDart));
        assert_eq!(c.power_amount(owner, PowerId::Dexterity), 0);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn h1_lunar_pastry_requires_owner_participation_including_extra_turns() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::LunarPastry);
        c.fire_relics(Hook::AfterTurnEnd, Ev::Side(Side::Enemy));
        assert_eq!(c.players[owner].stars, 0);
        c.extra_turn_players = 1 << ((owner + 1) % 4);
        c.fire_relics(Hook::AfterTurnEnd, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].stars, 0);
        c.extra_turn_players = 1 << owner;
        c.round = 9;
        c.players[owner].turn_number = 2;
        c.fire_relics(Hook::AfterTurnEnd, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].stars, 1);
        for seat in 0..4 {
            if seat != owner { assert_eq!(c.players[seat].stars, 0); }
        }
        c.players[owner].hooks_active = false;
        c.fire_relics(Hook::AfterTurnEnd, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].stars, 1);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn h1_iron_club_preserves_saved_count_filters_owner_and_skips_terminal_draw() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::IronClub);
        let other = (owner + 1) % 4;
        let own = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        let foreign = c.new_card(other, CardId::DefendIronclad, false).unwrap();
        let draw = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        c.players[owner].draw.push(draw);
        c.run.relics[0].counter = 11;
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.run.relics[0].counter, 11);
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(foreign));
        assert_eq!(c.run.relics[0].counter, 11);
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(own));
        assert_eq!(c.run.relics[0].counter, 12);
        assert!(c.players[owner].hand.contains(&draw));
        assert!(c.players[other].hand.is_empty());
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert_eq!(c.run.relics[0].counter, 12);
        c.is_over = true;
        c.run.relics[0].counter = 15;
        let before = c.players[owner].hand.clone();
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(own));
        assert_eq!(c.run.relics[0].counter, 16);
        assert_eq!(c.players[owner].hand, before);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn h1_run_layer_relics_are_inert_during_combat_for_every_owner() {
    // C# AfterObtained / AfterRoomEntered / AfterItemPurchased are not combat
    // hooks. Entry snapshots already contain HP, deck, gold and saved state.
    for owner in 0..4 {
        for id in [RelicId::HeftyTablet, RelicId::LoomingFruit,
            RelicId::MealTicket, RelicId::MawBank]
        {
            assert!(crate::relic::is_ported(id));
            assert!(crate::relics::OWNER_AWARE_RELICS.contains(&id));
            let mut c = setup(owner, id);
            let hp = c.cr(owner).hp;
            let max_hp = c.cr(owner).max_hp;
            let gold = c.run.gold;
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
            c.fire_relics(Hook::AfterTurnEnd, Ev::Side(Side::Player));
            c.fire_relics(Hook::AfterCombatEnd, Ev::None);
            assert_eq!((c.cr(owner).hp, c.cr(owner).max_hp, c.run.gold), (hp, max_hp, gold));
            assert!(c.fault.is_none(), "{id:?}: {:?}", c.fault);
        }
    }
}

#[test]
fn h1_maw_bank_base_room_entry_uses_only_owner_gold_and_saved_purchase_latch() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::MawBank);
        c.run.gold = 99;
        for p in 1..4 { c.players[p].gold = Some(99); }
        c.relic_base_room_entered();
        assert_eq!(c.relic_owner_gold(owner), Some(111));
        for p in 0..4 {
            if p != owner { assert_eq!(c.relic_owner_gold(p), Some(99)); }
        }
        c.run.relics[0].flag1 = true;
        c.relic_base_room_entered();
        assert_eq!(c.relic_owner_gold(owner), Some(111));
        assert!(c.fault.is_none());
        c.run.relics[0].flag1 = false;
        let mut hat = Relic::new(RelicId::BowlerHat);
        hat.owner = owner;
        c.run.relics.push(hat);
        c.relic_base_room_entered();
        assert_eq!(c.relic_owner_gold(owner), Some(126));
        let mut fruit = Relic::new(RelicId::DragonFruit);
        fruit.owner = owner;
        c.run.relics.push(fruit);
        c.relic_base_room_entered();
        assert_eq!(c.relic_owner_gold(owner), Some(141));
        assert_eq!(c.cr(owner).max_hp, 51);
        assert!(c.fault.is_none());
    }
}
