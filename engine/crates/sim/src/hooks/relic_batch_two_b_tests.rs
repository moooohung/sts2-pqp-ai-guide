use super::*;
use super::engine_tests::combat;
use crate::{combat::RunCtx, relic::Relic, rng::Rng};

fn setup(seats: usize, owner: CId, id: RelicId) -> Combat {
    let mut run: RunCtx = combat().run;
    run.all_cards_unlocked = Some(true);
    let mut c = Combat::new(run, 1);
    for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
    let mut r = Relic::new(id);
    r.owner = owner;
    c.run.relics.push(r);
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    c
}

#[test]
fn b2_phylactery_summons_and_grows_only_each_owners_osty() {
    for seats in 2..=4 {
        for owner in 0..seats as CId {
            let mut c = setup(seats, owner, RelicId::PhylacteryUnbound);
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            let pet = c.osty(owner).unwrap();
            assert_eq!(c.cr(pet).hp, 5);
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Enemy));
            assert_eq!(c.cr(pet).hp, 5);
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
            assert_eq!((c.cr(pet).hp, c.cr(pet).max_hp), (7, 7));
            assert!(c.has_power(pet, PowerId::DieForYou));
            for seat in 0..seats as CId {
                assert_eq!(c.osty(seat).is_some(), seat == owner);
            }
            assert!(c.fault.is_none());
        }
    }
}

#[test]
fn b2_first_owner_turn_generates_colorless_cards_and_opens_slots() {
    for seats in 2..=4 {
        for owner in 0..seats as CId {
            for id in [RelicId::OrangeDough, RelicId::RunicCapacitor] {
                let mut c = setup(seats, owner, id);
                // Deliberately disagree with round: C# reads Owner.TurnNumber.
                c.round = 9;
                c.players[owner].turn_number = 1;
                c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Enemy));
                assert!(c.players[owner].hand.is_empty());
                assert_eq!(c.players[owner].orbs.capacity, 0);
                c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
                for seat in 0..seats as CId {
                    assert_eq!(c.players[seat].hand.len(),
                        if id == RelicId::OrangeDough && seat == owner { 2 } else { 0 });
                    assert_eq!(c.players[seat].orbs.capacity,
                        if id == RelicId::RunicCapacitor && seat == owner { 3 } else { 0 });
                }
                if id == RelicId::OrangeDough {
                    let hand = &c.players[owner].hand;
                    assert_ne!(c.cards[hand[0] as usize].id, c.cards[hand[1] as usize].id);
                    for &k in hand { assert_eq!(c.cards[k as usize].owner, owner); }
                }
                c.round = 1;
                c.players[owner].turn_number = 2;
                let cards = c.cards.len();
                let capacity = c.players[owner].orbs.capacity;
                c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
                assert_eq!(c.cards.len(), cards);
                assert_eq!(c.players[owner].orbs.capacity, capacity);
                assert!(c.fault.is_none());
            }
        }
    }
}

#[test]
fn b2_snecko_confuses_and_draws_only_for_owner() {
    for seats in 2..=4 {
        for owner in 0..seats as CId {
            let mut c = setup(seats, owner, RelicId::SneckoEye);
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            for seat in 0..seats as CId {
                assert_eq!(c.power_amount(seat, PowerId::Confused), i32::from(seat == owner));
                assert_eq!(c.modify_hand_draw(5, seat), if seat == owner { 7 } else { 5 });
            }
            assert!(c.fault.is_none());
        }
    }
}

#[test]
fn b2_legion_pet_and_block_follow_card_owner_including_ally_targets() {
    for seats in 2..=4 {
        for owner in 0..seats as CId {
            let mut c = setup(seats, owner, RelicId::PaelsLegion);
            let other = (owner + 1) % seats as CId;
            c.fire_relics(Hook::BeforeCombatStart, Ev::None);
            let pet = c.players[owner].pets[0];
            assert_eq!(c.cr(pet).pet_owner, Some(owner));
            assert_eq!(c.cr(pet).hp, 9999);
            assert!(c.players[other].pets.is_empty());
            let foreign = c.new_card(other, CardId::DefendIronclad, false).unwrap();
            let own = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
            assert_eq!(c.calculate_block(5, owner, Props::MOVE, Some(foreign)), 5);
            assert_eq!(c.calculate_block(5, other, Props::MOVE, Some(own)), 10);
            assert!(c.run.relics[0].card.is_none(), "no CardPlay, no cooldown");
            c.active_card = Some(own);
            assert_eq!(c.calculate_block(5, other, Props::MOVE, Some(own)), 10);
            c.fire_relics(Hook::AfterCardPlayed, Ev::Card(own));
            assert_eq!(c.run.relics[0].counter, 2);
            assert_eq!(c.calculate_block(5, other, Props::MOVE, Some(own)), 5);
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Enemy));
            assert_eq!(c.run.relics[0].counter, 2);
            for expected in [1, 0] {
                c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
                assert_eq!(c.run.relics[0].counter, expected);
            }
            assert_eq!(c.calculate_block(5, other, Props::MOVE, Some(own)), 10);
            c.fire_relics(Hook::AfterCombatEnd, Ev::None);
            assert_eq!(c.run.relics[0].counter, 0);
            assert!(c.run.relics[0].card.is_none());
            assert!(c.fault.is_none());
        }
    }
}
