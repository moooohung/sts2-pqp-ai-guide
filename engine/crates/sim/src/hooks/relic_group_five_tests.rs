//! Models/Relics/{StrikeDummy,ParryingShield,TuningFork,PenNib,CaptainsWheel}.cs.
use super::*;
use super::engine_tests::combat;
use crate::rng::Rng;
fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat();
    c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let mut r = crate::relic::Relic::new(id); r.owner = owner;
    c.run.relics.push(r); c
}
#[test]
fn group_five_strike_source_and_dealer_ownership() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::StrikeDummy);
        let enemy = c.enemies[0]; let other = (owner + 1) % 4;
        let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        c.active_card = Some(k);
        c.deal_damage(Some(other), enemy, 6, Props::MOVE);
        assert_eq!(c.cr(enemy).max_hp - c.cr(enemy).hp, 9);
        c.cards[k as usize].card_type = CardType::Skill;
        assert_eq!(c.relic_damage_additive(0, owner, Some(other), enemy, Props::MOVE, Some(k)), 3);
        assert_eq!(c.relic_damage_additive(0, owner, Some(other), enemy, Props::UNPOWERED, Some(k)), 0);
        c.players[owner].hooks_active = false;
        assert_eq!(c.relic_damage_additive(0, owner, Some(other), enemy, Props::MOVE, Some(k)), 0);
        assert!(c.fault.is_none());
    }
}
#[test]
fn group_five_fork_persists_and_ignores_other_seats() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::TuningFork); c.run.relics[0].counter = 9;
        let foreign = c.new_card((owner + 1) % 4, CardId::DefendIronclad, false).unwrap();
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(foreign));
        assert_eq!(c.run.relics[0].counter, 9);
        let k = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        assert_eq!(c.cr(owner).block, 7); assert_eq!(c.run.relics[0].counter, 0);
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert_eq!(c.run.relics[0].counter, 1); assert!(c.fault.is_none());
    }
}
#[test]
fn group_five_nib_tracks_exact_attack_and_preview_pile() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::PenNib); c.run.relics[0].counter = 9;
        let enemy = c.enemies[0];
        let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        let foreign = c.new_card((owner + 1) % 4, CardId::StrikeIronclad, false).unwrap();
        assert_eq!(c.relic_damage_multiplicative(0, owner, Some(owner), Some(enemy), Props::MOVE, Some(k)), 2.0);
        c.players[owner].play.push(k);
        assert_eq!(c.relic_damage_multiplicative(0, owner, Some(owner), Some(enemy), Props::MOVE, Some(k)), 1.0);
        c.fire_relics(Hook::BeforeCardPlayed, Ev::Card(foreign));
        assert_eq!(c.run.relics[0].counter, 9);
        c.fire_relics(Hook::BeforeCardPlayed, Ev::Card(k));
        assert_eq!(c.run.relics[0].counter, 0);
        c.active_card = Some(k); c.deal_damage(Some(owner), enemy, 6, Props::MOVE);
        assert_eq!(c.cr(enemy).max_hp - c.cr(enemy).hp, 12);
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(foreign));
        assert_eq!(c.run.relics[0].card, Some(k));
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        assert_eq!(c.run.relics[0].card, None); assert!(c.fault.is_none());
    }
}
#[test]
fn group_five_wheel_and_parry_use_only_owner_state() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::CaptainsWheel); c.round = 7;
        c.players[owner].turn_number = 3;
        c.fire_relics(Hook::AfterBlockCleared, Ev::Creature((owner + 1) % 4));
        assert_eq!(c.cr(owner).block, 0);
        c.fire_relics(Hook::AfterBlockCleared, Ev::Creature(owner));
        assert_eq!(c.cr(owner).block, 18); assert!(c.fault.is_none());
        let mut c = setup(owner, RelicId::ParryingShield); c.cr_mut(owner).block = 10;
        let e = c.enemies[0]; let hp = c.cr(e).hp;
        c.fire_relics(Hook::AfterTurnEnd, Ev::Side(Side::Enemy));
        assert_eq!(c.cr(e).hp, hp);
        c.fire_relics(Hook::AfterTurnEnd, Ev::Side(Side::Player));
        assert_eq!(c.cr(e).hp, hp - 6); assert!(c.fault.is_none());
    }
}
