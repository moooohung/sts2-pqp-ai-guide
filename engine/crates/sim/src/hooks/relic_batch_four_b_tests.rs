use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(seats: usize, owner: CId, id: RelicId) -> Combat {
    let mut c = Combat::new(combat().run, 1);
    for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
    let mut r = Relic::new(id); r.owner = owner; c.run.relics.push(r);
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    c
}

#[test]
fn b4_snake_and_scroll_use_owners_first_hand_draw() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        for id in [RelicId::RingOfTheSnake, RelicId::NinjaScroll] {
            let mut c = setup(seats, owner, id); let other = (owner + 1) % seats as CId;
            c.round = 9; c.players[owner].turn_number = 1;
            c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(other));
            assert!(c.players[owner].hand.is_empty());
            assert_eq!(c.modify_hand_draw(5, other), 5);
            c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(owner));
            if id == RelicId::RingOfTheSnake { assert_eq!(c.modify_hand_draw(5, owner), 7); }
            else {
                assert_eq!(c.players[owner].hand.len(), 3);
                for &k in &c.players[owner].hand {
                    assert_eq!(c.cards[k as usize].id, CardId::Shiv);
                    assert_eq!(c.cards[k as usize].owner, owner);
                }
                assert_eq!(c.hist.generated_by_player_combat, 3);
            }
            c.round = 1; c.players[owner].turn_number = 2;
            let count = c.cards.len();
            c.fire_relics(Hook::BeforeHandDraw, Ev::Creature(owner));
            assert_eq!(c.cards.len(), count); assert_eq!(c.modify_hand_draw(5, owner), 5);
            assert!(c.fault.is_none());
        }
    }}
}

#[test]
fn b4_skull_uses_giver_not_source_and_krane_uses_damage_recipient() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        let other = (owner + 1) % seats as CId;
        let mut c = setup(seats, owner, RelicId::SneckoSkull); let e = c.enemies[0];
        c.apply_power_to(e, PowerId::Poison, 3, Some(other));
        assert_eq!(c.power_amount(e, PowerId::Poison), 3);
        c.apply_power_to(e, PowerId::Poison, 3, Some(owner));
        assert_eq!(c.power_amount(e, PowerId::Poison), 7);
        c.apply_power_to(e, PowerId::Weak, 3, Some(owner));
        assert_eq!(c.power_amount(e, PowerId::Weak), 3);
        assert!(c.fault.is_none());
        let mut c = setup(seats, owner, RelicId::PaperKrane); let e = c.enemies[0];
        c.apply_power_to(e, PowerId::Weak, 2, Some(owner));
        assert_eq!(c.calculate_damage(20, Some(e), owner, Props::MOVE), 12);
        assert_eq!(c.calculate_damage(20, Some(e), other, Props::MOVE), 15);
        assert_eq!(c.calculate_damage(20, Some(e), owner, Props::UNPOWERED), 20);
        assert!(c.fault.is_none());
    }}
}

#[test]
fn b4_regalite_tracks_creator_even_when_card_belongs_to_teammate() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        let mut c = setup(seats, owner, RelicId::Regalite); let other = (owner + 1) % seats as CId;
        let k = c.new_card(owner, CardId::Shiv, false).unwrap();
        c.add_generated_card_by(k, PileKind::Hand, PilePos::Bottom, Some(other));
        assert_eq!(c.cr(owner).block, 0);
        let k = c.new_card(other, CardId::Shiv, false).unwrap();
        c.add_generated_card_by(k, PileKind::Hand, PilePos::Bottom, Some(owner));
        assert_eq!(c.cr(owner).block, 4); assert_eq!(c.cr(other).block, 0);
        let k = c.new_card(owner, CardId::Shiv, false).unwrap();
        c.add_generated_card_by(k, PileKind::Hand, PilePos::Bottom, Some(owner));
        assert_eq!(c.cr(owner).block, 4);
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        let k = c.new_card(owner, CardId::Shiv, false).unwrap();
        c.add_generated_card_by(k, PileKind::Hand, PilePos::Bottom, None);
        assert_eq!(c.cr(owner).block, 4);
        let k = c.new_card(owner, CardId::Shiv, false).unwrap();
        c.add_generated_card_by(k, PileKind::Hand, PilePos::Bottom, Some(owner));
        assert_eq!(c.cr(owner).block, 8);
        assert!(c.fault.is_none());
    }}
}

#[test]
fn b4_power_cell_moves_only_two_locally_free_non_x_cards_without_drawing() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        let mut c = setup(seats, owner, RelicId::PowerCell); let other = (owner + 1) % seats as CId;
        for id in [CardId::Shiv, CardId::Deflect, CardId::Anger, CardId::Whirlwind] {
            let k = c.new_card(owner, id, false).unwrap(); c.players[owner].draw.push(k);
        }
        let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        c.cards[k as usize].set_energy_cost(0, crate::card::CostExpiry::Combat, false);
        c.players[owner].draw.push(k);
        c.round = 9; c.players[owner].turn_number = 1;
        let counter = c.run.rngs.combat_card_selection.counter;
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].hand.len(), 2);
        assert_eq!(c.players[owner].draw.len(), 3);
        assert!(c.players[other].hand.is_empty());
        assert_eq!(c.run.rngs.combat_card_selection.counter - counter, 3);
        for &k in &c.players[owner].hand {
            assert!(!c.cards[k as usize].x_cost);
            assert_eq!(c.cards[k as usize].local_energy_cost(), 0);
        }
        c.round = 1; c.players[owner].turn_number = 2;
        c.fire_relics(Hook::BeforeSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].hand.len(), 2);
        assert!(c.fault.is_none());
    }}
}

#[test]
fn b4_sai_blocks_only_owner_and_oyster_has_no_combat_heal_or_maxhp_effect() {
    for seats in 2..=4 { for owner in 0..seats as CId {
        let mut c = setup(seats, owner, RelicId::Sai);
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Enemy));
        assert_eq!(c.cr(owner).block, 0);
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        for seat in 0..seats as CId { assert_eq!(c.cr(seat).block, if seat == owner { 7 } else { 0 }); }
        assert!(c.fault.is_none());
        let mut c = setup(seats, owner, RelicId::NutritiousOyster);
        c.cr_mut(owner).hp = 25;
        for hook in [Hook::BeforeCombatStart, Hook::AfterCombatVictory, Hook::AfterCombatEnd] {
            c.fire_relics(hook, Ev::None);
        }
        assert_eq!((c.cr(owner).hp, c.cr(owner).max_hp), (25, 50));
        assert!(c.fault.is_none());
    }}
}
