//! Models/Relics/{Girya,Shuriken,UnsettlingLamp,BronzeScales,OddlySmoothStone}.cs.
use super::*;
use super::engine_tests::combat;
use crate::rng::Rng;

fn with_relic(owner: CId, id: RelicId) -> Combat {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let mut relic = crate::relic::Relic::new(id);
    relic.owner = owner;
    c.run.relics.push(relic);
    c
}

#[test]
fn girya_saved_lifts_apply_only_to_each_owner() {
    for owner in 0..4 {
        for lifts in [0, 1, 3] {
            let mut c = with_relic(owner, RelicId::Girya);
            c.run.relics[0].counter = lifts;
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            for seat in 0..4 {
                assert_eq!(c.power_amount(seat, PowerId::Strength), if seat == owner { lifts } else { 0 });
            }
            assert!(c.fault.is_none());
        }
    }
}

#[test]
fn scales_and_smooth_stone_use_owner_and_normal_damage_dispatch() {
    for owner in 0..4 {
        for (id, power, amount) in [
            (RelicId::BronzeScales, PowerId::Thorns, 3),
            (RelicId::OddlySmoothStone, PowerId::Dexterity, 1),
        ] {
            let mut c = with_relic(owner, id);
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            for seat in 0..4 {
                assert_eq!(c.power_amount(seat, power), if seat == owner { amount } else { 0 });
            }
            if id == RelicId::BronzeScales {
                let enemy = c.enemies[0];
                c.cr_mut(enemy).hp = 100;
                c.deal_damage(Some(enemy), owner, 2, Props::MOVE);
                assert_eq!(c.cr(enemy).hp, 97);
                assert_eq!(c.cr(owner).hp, 48);
                for seat in 0..4 {
                    if seat != owner { assert_eq!(c.cr(seat).hp, 50); }
                }
            }
            c.players[owner].hooks_active = false;
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            assert_eq!(c.power_amount(owner, power), amount);
            assert!(c.fault.is_none(), "{id:?} owner {owner}: {:?}", c.fault);
        }
    }
}

#[test]
fn shuriken_counts_only_owner_attacks_in_progress_and_resets_on_owner_side() {
    for owner in 0..4 {
        let mut c = with_relic(owner, RelicId::Shuriken);
        for seat in 0..4 {
            if seat != owner {
                let card = c.new_card(seat, CardId::StrikeIronclad, false).unwrap();
                for _ in 0..3 { c.fire_relics(Hook::AfterCardPlayed, Ev::Card(card)); }
            }
        }
        assert_eq!(c.run.relics[0].counter, 0);
        let skill = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(skill));
        assert_eq!(c.run.relics[0].counter, 0);
        let card = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        for _ in 0..3 { c.fire_relics(Hook::AfterCardPlayed, Ev::Card(card)); }
        assert_eq!(c.power_amount(owner, PowerId::Strength), 1);
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Enemy));
        assert_eq!(c.run.relics[0].counter, 3);
        c.is_over = true;
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(card));
        assert_eq!(c.run.relics[0].counter, 3);
        c.is_over = false;
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.run.relics[0].counter, 0);
        for seat in 0..4 {
            if seat != owner { assert_eq!(c.power_amount(seat, PowerId::Strength), 0); }
        }
        assert!(c.fault.is_none());
    }
}

#[test]
fn lamp_selects_by_applier_not_card_owner_skips_artifact_and_finishes_once() {
    for owner in 0..4 {
        let mut c = with_relic(owner, RelicId::UnsettlingLamp);
        let enemy = c.enemies[0];
        let other = (owner + 1) % 4;
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        let card = c.new_card(other, CardId::Bash, false).unwrap();
        c.active_card = Some(card);
        c.apply_power_to(enemy, PowerId::Vulnerable, 1, Some(other));
        assert_eq!(c.power_amount(enemy, PowerId::Vulnerable), 1);
        assert_eq!(c.run.relics[0].card, None);
        c.active_card = None;
        c.apply_power_to(enemy, PowerId::Artifact, 1, Some(enemy));
        c.active_card = Some(card);
        // ArtifactPower.GetScaledAmountForMultiplayer: 1 + player count - 1.
        let artifacts = c.players.len() as i32;
        assert_eq!(c.power_amount(enemy, PowerId::Artifact), artifacts);
        for used in 1..=artifacts {
            c.apply_power_to(enemy, PowerId::Weak, 1, Some(owner));
            assert_eq!(c.power_amount(enemy, PowerId::Artifact), artifacts - used);
            assert_eq!(c.run.relics[0].card, None);
        }
        assert!(!c.has_power(enemy, PowerId::Artifact));
        assert_eq!(c.run.relics[0].card, None);
        c.apply_power_to(enemy, PowerId::Weak, 1, Some(owner));
        assert_eq!(c.power_amount(enemy, PowerId::Weak), 2);
        assert_eq!(c.run.relics[0].card, Some(card));
        c.apply_power_to(other, PowerId::Weak, 1, Some(owner));
        assert_eq!(c.power_amount(other, PowerId::Weak), 2);
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(card));
        assert!(c.run.relics[0].flag1);
        let second = c.new_card(owner, CardId::Bash, false).unwrap();
        c.active_card = Some(second);
        c.apply_power_to(enemy, PowerId::Vulnerable, 1, Some(owner));
        assert_eq!(c.power_amount(enemy, PowerId::Vulnerable), 2);
        c.active_card = None;
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert_eq!(c.run.relics[0].card, None);
        assert!(!c.run.relics[0].flag1);
        assert!(c.fault.is_none(), "owner {owner}: {:?}", c.fault);
    }
}

#[test]
fn lamp_doubles_temporary_debuff_but_not_its_internal_power_twice() {
    for owner in 0..4 {
        for (card_id, power, amount) in [
            (CardId::DarkShackles, PowerId::DarkShackles, 6),
            (CardId::PiercingWail, PowerId::PiercingWail, 6),
            (CardId::EnfeeblingTouch, PowerId::EnfeeblingTouch, 8),
        ] {
            let mut c = with_relic(owner, RelicId::UnsettlingLamp);
            let enemy = c.enemies[0];
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            let card = c.new_card(owner, card_id, false).unwrap();
            c.active_card = Some(card);
            c.apply_power_to(enemy, power, amount, Some(owner));
            // TemporaryStrengthPower.BeforeApplied/AfterPowerAmountChanged:
            // the doubled parent applies -2*amount Strength, never -4*amount.
            assert_eq!(c.power_amount(enemy, power), amount * 2);
            assert_eq!(c.power_amount(enemy, PowerId::Strength), -amount * 2);
            c.active_card = None;
            assert!(c.fault.is_none(), "{power:?} owner {owner}: {:?}", c.fault);
        }
    }
}

#[test]
fn lamp_retains_the_original_power_instance_after_removal_and_same_class_recreation() {
    let mut c = with_relic(0, RelicId::UnsettlingLamp);
    let enemy = c.enemies[0];
    assert!(c.run.relics[0].doubled_powers.is_none());
    c.fire_relics(Hook::BeforeCombatStart, Ev::None);
    assert_eq!(c.run.relics[0].doubled_powers.as_deref(), Some(&[][..]));
    let card = c.new_card(0, CardId::Bash, false).unwrap();
    let original = c.apply_power_from(enemy, PowerId::Weak, 2, Some(0), Some(card)).unwrap();
    assert_eq!(c.run.relics[0].doubled_powers.as_deref(), Some(&[original][..]));
    assert_eq!(c.powers[original as usize].amount, 4);
    c.remove_power(enemy, original);
    let replacement = c.apply_power_from(enemy, PowerId::Weak, 1, Some(0), None).unwrap();
    assert_ne!(original, replacement);
    assert_eq!(c.run.relics[0].doubled_powers.as_deref(), Some(&[original][..]));
    assert_eq!(c.powers[original as usize].amount, 4);
    assert_eq!(c.powers[replacement as usize].amount, 1);
    let mut cloned = c.clone();
    cloned.run.relics[0].doubled_powers.as_mut().unwrap().clear();
    assert_eq!(c.run.relics[0].doubled_powers.as_deref(), Some(&[original][..]));
    c.fire_relics(Hook::AfterCombatEnd, Ev::None);
    assert_eq!(c.run.relics[0].doubled_powers.as_deref(), Some(&[][..]));
}

#[test]
fn lamp_does_not_attribute_relic_or_power_debuffs_to_the_active_card() {
    for owner in 0..4 {
        let mut c = with_relic(owner, RelicId::UnsettlingLamp);
        let mut marbles = crate::relic::Relic::new(RelicId::BagOfMarbles);
        marbles.owner = owner;
        c.run.relics.push(marbles);
        c.players[owner].turn_number = 1;
        let enemy = c.enemies[0];
        let card = c.new_card(owner, CardId::Bash, false).unwrap();
        c.active_card = Some(card);
        // BagOfMarbles.BeforeSideTurnStart explicitly supplies null cardSource.
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.power_amount(enemy, PowerId::Vulnerable), 1);
        assert_eq!(c.run.relics[0].card, None);
        c.apply_power_from(owner, PowerId::Envenom, 1, Some(owner), None);
        c.deal_damage(Some(owner), enemy, 1, Props::MOVE);
        // EnvenomPower.cs:24 supplies null even during an owned attack.
        assert_eq!(c.power_amount(enemy, PowerId::Poison), 1);
        assert_eq!(c.run.relics[0].card, None);
        // The real card's command still selects/doubles its own source.
        c.apply_power_to(enemy, PowerId::Weak, 1, Some(owner));
        assert_eq!(c.power_amount(enemy, PowerId::Weak), 2);
        assert_eq!(c.run.relics[0].card, Some(card));
        assert!(c.fault.is_none(), "owner {owner}: {:?}", c.fault);
    }
}
