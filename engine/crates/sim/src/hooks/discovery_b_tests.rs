use crate::character::Character;
use super::super::*;
use super::combat;
use crate::rng::Rng;

#[test]
fn discovery_uses_silent_pool_for_every_seat_including_seat_one() {
    let pool = crate::cardgen::character_pool(crate::character::CardPool::Silent, false, None, None).unwrap();
    for seats in 2..=4 {
        for owner in 0..seats as CId {
            let mut run = combat().run;
            run.all_cards_unlocked = Some(true);
            let mut c = Combat::new(run, 1);
            for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
            c.players[owner].character = Some(Character::Silent);
            c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(1));
            let k = c.new_card(owner, CardId::Discovery, false).unwrap();
            c.players[owner].hand.push(k);
            c.execute_card_play(k, None, false, false);
            assert!(c.pending_choice.is_some());
            assert_eq!(c.choice_options.len(), 3);
            let mut ids = Vec::new();
            for &option in &c.choice_options {
                let card = &c.cards[option as usize];
                assert_eq!(card.owner, owner);
                assert!(pool.contains(&card.id), "{:?} is not a Silent card", card.id);
                assert!(!ids.contains(&card.id), "Discovery options must be distinct");
                ids.push(card.id);
            }
            assert!(c.resolve_pending_choice(Some(0)));
            assert!(c.players[owner].hand.iter().any(|&q| c.cards[q as usize].id == ids[0]));
            for seat in 0..seats as CId {
                if seat != owner { assert!(c.players[seat].hand.is_empty()); }
            }
            assert!(c.fault.is_none());
        }
    }
}
