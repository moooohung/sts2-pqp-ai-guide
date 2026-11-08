//! Models/Relics/{GremlinHorn,BagOfPreparation,StoneCalendar,TungstenRod,RazorTooth}.cs.
use super::*;
use super::engine_tests::combat;
use crate::rng::Rng;
fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat(); c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    let mut r = crate::relic::Relic::new(id); r.owner = owner; c.run.relics.push(r); c
}
#[test]
fn group_seven_horn_enemy_death_rewards_only_its_owner_not_the_killer() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::GremlinHorn);
        let other = (owner + 1) % 4;
        let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        c.players[owner].draw.push(k);
        let enemy = c.enemies[0];
        c.deal_damage(Some(other), enemy, 999, Props::MOVE);
        assert_eq!(c.players[owner].energy, 1);
        assert_eq!(c.players[owner].hand.as_slice(), &[k]);
        for seat in 0..4 {
            if seat != owner { assert_eq!(c.players[seat].energy, 0); assert!(c.players[seat].hand.is_empty()); }
        }
        c.fire_relic_after_death(other);
        assert_eq!(c.players[owner].energy, 1);
        c.players[owner].hooks_active = false;
        c.fire_relic_after_death(enemy);
        assert_eq!(c.players[owner].energy, 1);
        assert!(c.fault.is_none());
    }
}
#[test]
fn group_seven_bag_and_calendar_use_owner_turn_not_round() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::BagOfPreparation);
        c.round = 9; c.players[owner].turn_number = 1;
        for seat in 0..4 { assert_eq!(c.modify_hand_draw(5, seat), if seat == owner { 7 } else { 5 }); }
        c.round = 1; c.players[owner].turn_number = 2;
        assert_eq!(c.modify_hand_draw(5, owner), 5);
        assert!(c.fault.is_none());
        let mut c = setup(owner, RelicId::StoneCalendar);
        c.round = 6; c.players[owner].turn_number = 7;
        for enemy in c.enemies.clone() { c.cr_mut(enemy).hp = 200; c.cr_mut(enemy).max_hp = 200; }
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Enemy));
        for &enemy in &c.enemies { assert_eq!(c.cr(enemy).hp, 200); }
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Player));
        for &enemy in &c.enemies { assert_eq!(c.cr(enemy).hp, 148); }
        c.round = 7; c.players[owner].turn_number = 8;
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Player));
        for &enemy in &c.enemies { assert_eq!(c.cr(enemy).hp, 148); }
        assert!(c.fault.is_none());
    }
}
#[test]
fn group_seven_rod_reduces_owner_hp_loss_after_block_including_self_damage() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::TungstenRod); let other = (owner + 1) % 4;
        let enemy = c.enemies[0];
        c.cr_mut(owner).block = 3;
        c.deal_damage(Some(enemy), owner, 10, Props::MOVE);
        assert_eq!(c.cr(owner).hp, 44);
        c.deal_damage(Some(enemy), other, 10, Props::MOVE);
        assert_eq!(c.cr(other).hp, 40);
        c.deal_damage(None, owner, 1, Props::UNPOWERED.or(Props::UNBLOCKABLE));
        assert_eq!(c.cr(owner).hp, 44);
        c.deal_damage(None, owner, 3, Props::UNPOWERED.or(Props::UNBLOCKABLE));
        assert_eq!(c.cr(owner).hp, 42);
        assert!(c.fault.is_none());
    }
}
#[test]
fn group_seven_razor_upgrades_only_owners_upgradable_attacks_and_skills() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::RazorTooth);
        for seat in 0..4 {
            for id in [CardId::StrikeIronclad, CardId::DefendIronclad, CardId::Inflame] {
                let k = c.new_card(seat, id, false).unwrap();
                c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
                assert_eq!(c.cards[k as usize].upgraded, seat == owner && id != CardId::Inflame);
            }
        }
        let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        c.is_over = true;
        c.fire_relics(Hook::AfterCardPlayed, Ev::Card(k));
        assert!(c.cards[k as usize].upgraded);
        assert!(c.fault.is_none());
    }
}
