use super::*;
use super::engine_tests::combat;
use crate::rng::Rng;
fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat(); c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    for e in c.enemies.clone() { c.cr_mut(e).hp = 200; c.cr_mut(e).max_hp = 200; }
    let mut r = crate::relic::Relic::new(id); r.owner = owner; c.run.relics.push(r); c
}
#[test]
fn group_nine_popper_and_ashes_damage_enemies_only_for_their_owners() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::FestivePopper); let other = (owner + 1) % 4;
        c.round = 9; c.players[owner].turn_number = 1;
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(other));
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 200); }
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 191); }
        c.round = 1; c.players[owner].turn_number = 2;
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 191); }
        let mut c = setup(owner, RelicId::CharonsAshes);
        let foreign = c.new_card(other, CardId::Dazed, false).unwrap();
        c.fire_after_card_exhausted(foreign);
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 200); }
        let own = c.new_card(owner, CardId::Dazed, false).unwrap();
        c.fire_after_card_exhausted(own);
        for &e in &c.enemies { assert_eq!(c.cr(e).hp, 197); }
        assert!(c.fault.is_none());
    }
}
#[test]
fn group_nine_skull_tracks_owner_hp_and_clay_only_owner_positive_damage() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::RedSkull); let other = (owner + 1) % 4;
        c.cr_mut(owner).hp = 25;
        c.fire_relics(Hook::BeforeCombatStart, Ev::None);
        assert_eq!(c.power_amount(owner, PowerId::Strength), 3);
        c.heal(owner, 1);
        assert_eq!(c.power_amount(owner, PowerId::Strength), 0);
        c.cr_mut(owner).hp = 25;
        c.heal(other, 1);
        c.fire_relics(Hook::AfterCurrentHpChanged, Ev::HpChanged { creature: other, delta: -1 });
        assert_eq!(c.power_amount(owner, PowerId::Strength), 3);
        assert!(c.fault.is_none());
        let mut c = setup(owner, RelicId::SelfFormingClay);
        c.deal_damage(Some(c.enemies[0]), other, 1, Props::MOVE);
        assert_eq!(c.power_amount(owner, PowerId::SelfFormingClay), 0);
        c.deal_damage(Some(c.enemies[0]), owner, 1, Props::MOVE);
        assert_eq!(c.power_amount(owner, PowerId::SelfFormingClay), 3);
        c.clear_block(owner);
        c.fire(Hook::AfterBlockCleared, Ev::Creature(owner), None);
        assert_eq!(c.cr(owner).block, 3);
        assert!(!c.has_power(owner, PowerId::SelfFormingClay));
        assert!(c.fault.is_none());
    }
}
#[test]
fn group_nine_phrog_is_the_attacking_players_relic_not_a_teammates() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::PaperPhrog); let enemy = c.enemies[0];
        c.apply_power_from(enemy, PowerId::Vulnerable, 2, None, None);
        let before = c.cr(enemy).hp;
        c.deal_damage(Some((owner + 1) % 4), enemy, 8, Props::MOVE);
        assert_eq!(before - c.cr(enemy).hp, 12);
        let before = c.cr(enemy).hp;
        c.deal_damage(Some(owner), enemy, 8, Props::MOVE);
        assert_eq!(before - c.cr(enemy).hp, 14);
        assert!(c.fault.is_none());
    }
}
