//! Models/Relics/{JossPaper,CentennialPuzzle,Bellows,MummifiedHand,SturdyClamp}.cs.
use super::*;
use super::engine_tests::combat;
use crate::rng::Rng;
fn setup(owner: CId, id: RelicId) -> Combat {
    let mut c = combat(); c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
    let mut r = crate::relic::Relic::new(id); r.owner = owner; c.run.relics.push(r); c
}
fn draw_pile(c: &mut Combat, owner: CId) {
    for _ in 0..5 { let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap(); c.players[owner].draw.push(k); }
}
#[test]
fn group_six_joss_normal_and_ethereal_use_owner_and_preserve_remainder() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::JossPaper); draw_pile(&mut c, owner);
        let k = c.new_card(owner, CardId::Anger, false).unwrap();
        let foreign = c.new_card((owner + 1) % 4, CardId::Anger, false).unwrap();
        c.fire_after_card_exhausted(foreign);
        assert_eq!(c.run.relics[0].counter, 0);
        for _ in 0..4 { c.fire_after_card_exhausted(k); }
        c.fire_power_relic_after_card_exhausted(k, true);
        assert_eq!(c.players[owner].hand.len(), 0);
        c.fire_relics(Hook::AfterTurnEnd, Ev::Side(Side::Enemy));
        assert_eq!(c.run.relics[0].counter2, 1);
        c.fire_relics(Hook::AfterTurnEnd, Ev::Side(Side::Player));
        assert_eq!(c.players[owner].hand.len(), 1); assert_eq!(c.run.relics[0].counter, 0);
        for _ in 0..6 { c.fire_after_card_exhausted(k); }
        assert_eq!(c.players[owner].hand.len(), 2);
        c.fire_relics(Hook::AfterCombatEnd, Ev::None);
        assert_eq!(c.run.relics[0].counter, 1); assert!(c.fault.is_none());
    }
}
#[test]
fn group_six_puzzle_first_owner_damage_only_and_clamp_retains_at_most_ten() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::CentennialPuzzle); draw_pile(&mut c, owner);
        let enemy = c.enemies[0]; let other = (owner + 1) % 4;
        c.deal_damage(Some(enemy), other, 1, Props::MOVE);
        assert_eq!(c.players[owner].hand.len(), 0);
        c.deal_damage(Some(enemy), owner, 1, Props::MOVE);
        assert_eq!(c.players[owner].hand.len(), 3);
        c.deal_damage(Some(enemy), owner, 1, Props::MOVE);
        assert_eq!(c.players[owner].hand.len(), 3); assert!(c.fault.is_none());
        for block in [0, 5, 20] {
            let mut c = setup(owner, RelicId::SturdyClamp); c.cr_mut(owner).block = block;
            c.cr_mut(other).block = 20; c.clear_block(other); c.clear_block(owner);
            assert_eq!(c.cr(other).block, 0); assert_eq!(c.cr(owner).block, block.min(10));
            assert!(c.fault.is_none());
        }
    }
}
#[test]
fn group_six_bellows_uses_owner_first_turn_not_round() {
    for owner in 0..4 {
        let mut c = setup(owner, RelicId::Bellows); let other = (owner + 1) % 4;
        let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap(); c.players[owner].hand.push(k);
        c.round = 8; c.players[owner].turn_number = 1;
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(other));
        assert!(!c.cards[k as usize].upgraded);
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
        assert!(c.cards[k as usize].upgraded);
        let k = c.new_card(owner, CardId::StrikeIronclad, false).unwrap(); c.players[owner].hand.push(k);
        c.round = 1; c.players[owner].turn_number = 2;
        c.fire_relics(Hook::AfterPlayerTurnStart, Ev::Creature(owner));
        assert!(!c.cards[k as usize].upgraded); assert!(c.fault.is_none());
    }
}
#[test]
fn group_six_mummified_hand_exercises_all_four_priority_pools() {
    for owner in 0..4 {
        for tier in 1..=4 {
            let mut c = setup(owner, RelicId::MummifiedHand);
            let other = (owner + 1) % 4;
            let power = c.new_card(owner, CardId::Inflame, false).unwrap();
            let strike = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
            let anger = c.new_card(owner, CardId::Anger, false).unwrap();
            if tier != 4 { c.players[owner].hand.push(strike); }
            c.players[owner].hand.push(anger);
            if tier >= 2 { c.cards[strike as usize].set_energy_cost(0, crate::card::CostExpiry::Turn, false); }
            if tier == 2 { c.cards[anger as usize].set_energy_cost(1, crate::card::CostExpiry::Combat, false); }
            let foreign = c.new_card(other, CardId::Inflame, false).unwrap();
            let before = c.run.rngs.combat_card_selection.counter;
            c.fire_relics(Hook::AfterCardPlayed, Ev::Card(foreign));
            assert_eq!(c.run.rngs.combat_card_selection.counter, before);
            c.fire_relics(Hook::AfterCardPlayed, Ev::Card(power));
            let chosen = if tier == 2 || tier == 4 { anger } else { strike };
            assert_eq!(c.cards[chosen as usize].local_energy_cost(), 0);
            assert_eq!(c.cards[chosen as usize].cv.turn_star_cost_override, Some(0));
            c.cards[chosen as usize].after_card_played_cost_cleanup();
            if tier <= 2 { assert_eq!(c.cards[chosen as usize].local_energy_cost(), 1); }
            assert!(c.fault.is_none(), "tier {tier} owner {owner}: {:?}", c.fault);
        }
    }
}
