use super::*;
use super::engine_tests::combat;
use crate::{relic::Relic, rng::Rng};

fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat();
    let mut relic = Relic::new(id);
    relic.owner = owner;
    c.run.relics.push(relic);
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
    c
}

#[test]
fn b10_tears_latches_owner_energy_and_filters_participants() {
    for owner in 0..4 {
        let other = (owner + 1) % 4;
        let mut c = setup(owner, RelicId::PaelsTears);
        for leftover in [true, false] {
            for seat in 0..4 { c.players[seat].energy = if leftover { 0 } else { 1 }; }
            c.players[owner].energy = i32::from(leftover);
            c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Player));
            assert_eq!(c.run.relics[0].flag1, leftover);
            for seat in 0..4 { c.players[seat].energy = 3; }
            c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
            for seat in 0..4 {
                assert_eq!(c.players[seat].energy, if seat == owner && leftover { 5 } else { 3 });
            }
        }
        c.run.relics[0].flag1 = true;
        c.extra_turn_players = 1 << other;
        c.players[owner].energy = 0;
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Player));
        assert!(c.run.relics[0].flag1, "another player's extra turn cannot overwrite the latch");
        c.fire_relics(Hook::AfterSideTurnStart, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].energy, 0);
        c.fire_relics(Hook::BeforeTurnEnd, Ev::Side(Side::Enemy));
        assert!(c.run.relics[0].flag1);
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert!(!c.run.relics[0].flag1);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn b10_blood_draws_one_extra_card_for_each_owner_through_turn_start() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::PaelsBlood);
        for seat in 0..4 {
            for _ in 0..10 {
                let k = c.new_card(seat, CardId::StrikeIronclad, false).unwrap();
                c.players[seat].draw.push(k);
            }
        }
        c.start_player_turn();
        for seat in 0..4 {
            assert_eq!(c.players[seat].hand.len(), if seat == owner { 6 } else { 5 });
        }
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn b10_pyramid_keeps_only_owners_hand_in_order_through_real_turn_end() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::RunicPyramid);
        for seat in 0..4 {
            for _ in 0..10 {
                let k = c.new_card(seat, CardId::StrikeIronclad, false).unwrap();
                c.players[seat].draw.push(k);
            }
        }
        c.start_player_turn();
        let hands: Vec<Vec<_>> = (0..4).map(|s| c.players[s].hand.to_vec()).collect();
        c.end_player_turn();
        for seat in 0..4 {
            if seat == owner {
                assert!(c.players[seat].hand.starts_with(&hands[seat as usize]));
            } else {
                for k in &hands[seat as usize] {
                    assert!(!c.players[seat].hand.contains(k));
                    assert!(c.players[seat].discard.contains(k));
                }
            }
        }
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}

#[test]
fn b10_noncombat_models_do_not_replay_pickups_rewards_or_room_healing() {
    for owner in 0..4 {
        for id in [RelicId::PhialHolster, RelicId::RoyalStamp, RelicId::PaelsWing,
            RelicId::Planisphere, RelicId::OldCoin] {
            let mut c = setup(owner, id);
            for seat in 0..4 { c.cr_mut(seat).hp = 25; }
            let gold = c.run.gold;
            let slots = c.run.max_potion_slots;
            let niche = c.run.rngs.niche.counter;
            for hook in [Hook::BeforeCombatStart, Hook::AfterCombatVictory, Hook::AfterCombatEnd] {
                c.fire_relics(hook, Ev::None);
            }
            for seat in 0..4 {
                assert_eq!(c.cr(seat).hp, 25);
                let k = c.new_card(seat, CardId::StrikeIronclad, false).unwrap();
                c.add_generated_card(k, PileKind::Hand, PilePos::Bottom, true);
                assert!(c.cards[k as usize].enchantments.is_empty());
            }
            assert_eq!(c.run.gold, gold);
            assert_eq!(c.run.max_potion_slots, slots);
            assert_eq!(c.run.rngs.niche.counter, niche);
            assert!(c.fault.is_none(), "{id:?}: {:?}", c.fault);
        }
    }
}
