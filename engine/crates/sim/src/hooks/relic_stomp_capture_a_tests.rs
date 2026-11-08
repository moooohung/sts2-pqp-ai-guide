//! The IvoryTile DLL capture exposed Stomp listening to teammates' attacks.
use super::*;
use super::engine_tests::combat;
use crate::rng::Rng;

#[test]
fn stomp_discounts_only_owner_attacks_on_play_and_late_card_entry() {
    for owner in 0..4 {
        let mut c = combat();
        c.setup_encounter("TUNNELER_WEAK", &mut Rng::new(1));
        c.start_combat();
        let stomp = c.new_card(owner, CardId::Stomp, false).unwrap();
        c.add_generated_card_by(stomp, PileKind::Hand, PilePos::Bottom, Some(owner));
        let other = (owner + 1) % 4;
        let foreign = c.new_card(other, CardId::StrikeIronclad, false).unwrap();
        c.players[other].hand.push(foreign);
        assert!(c.play_card(other, 0, Some(0)));
        assert_eq!(c.cards[stomp as usize].local_energy_cost(), 3);
        let late = c.new_card(owner, CardId::Stomp, false).unwrap();
        c.add_generated_card_by(late, PileKind::Hand, PilePos::Bottom, Some(owner));
        assert_eq!(c.cards[late as usize].local_energy_cost(), 3);
        let own = c.new_card(owner, CardId::StrikeIronclad, false).unwrap();
        c.players[owner].hand.push(own);
        let index = c.players[owner].hand.iter().position(|&k| k == own).unwrap();
        assert!(c.play_card(owner, index, Some(0)));
        assert_eq!(c.cards[stomp as usize].local_energy_cost(), 2);
        let late = c.new_card(owner, CardId::Stomp, false).unwrap();
        c.add_generated_card_by(late, PileKind::Hand, PilePos::Bottom, Some(owner));
        assert_eq!(c.cards[late as usize].local_energy_cost(), 2);
        assert!(c.fault.is_none(), "{:?}", c.fault);
    }
}
