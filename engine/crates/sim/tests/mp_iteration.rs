//! Original IterationPower.AfterCardDrawn counts Status draws by Actor this turn.
use sts2_sim::{
    combat::Combat,
    rng::Rng,
    run::{RunManager, RunOptions},
    tables::{CardId, PowerId},
};

fn combat(seats: u8) -> Combat {
    let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    run.deck.clear();
    let mut c = Combat::new(run, 1);
    for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    c.start_combat();
    c
}

fn queue(c: &mut Combat, owner: u8, ids: &[CardId]) -> Vec<u16> {
    ids.iter().map(|&id| {
        let card = c.new_card(owner, id, false).unwrap();
        c.players[owner].draw.push(card);
        card
    }).collect()
}

#[test]
fn iteration_first_status_is_owner_bound_for_hand_and_extra_draws() {
    let status = CardId::from_name("SLIMED").unwrap();
    for seats in [1, 4] { for owner in 0..seats { for hand_draw in [false, true] {
        let mut c = combat(seats);
        c.apply_power_to(owner, PowerId::Iteration, 2, Some(owner));
        for other in 0..seats { if other != owner {
            queue(&mut c, other, &[status]);
            c.draw_cards(other, 1, !hand_draw);
        } }
        let cards = queue(&mut c, owner, &[
            status, CardId::DefendIronclad, CardId::DefendIronclad,
            status, CardId::DefendIronclad,
        ]);
        c.draw_cards(owner, 1, hand_draw);
        assert_eq!(c.players[owner].hand.as_slice(), &cards[..3]);
        c.draw_cards(owner, 1, hand_draw);
        assert_eq!(c.players[owner].hand.as_slice(), &cards[..4]);
        assert_eq!(c.players[owner].draw.as_slice(), &cards[4..]);
        for other in 0..seats { if other != owner {
            assert_eq!(c.players[other].hand.len(), 1);
        } }
        assert_eq!(c.fault, None);
    } } }
}

#[test]
fn iteration_includes_draws_before_application_and_resets_each_side_turn() {
    let status = CardId::from_name("SLIMED").unwrap();
    for seats in [1, 4] { for owner in 0..seats {
        let mut c = combat(seats);
        let cards = queue(&mut c, owner, &[
            status, status, status, CardId::DefendIronclad, CardId::DefendIronclad,
        ]);
        c.draw_cards(owner, 1, true);
        c.apply_power_to(owner, PowerId::Iteration, 2, Some(owner));
        c.draw_cards(owner, 1, false);
        assert_eq!(c.players[owner].hand.as_slice(), &cards[..2]);
        c.hist.reset_side_turn();
        c.draw_cards(owner, 1, true);
        assert_eq!(c.players[owner].hand.as_slice(), cards.as_slice());
        assert!(c.players[owner].draw.is_empty());
        assert_eq!(c.fault, None);
    } }
}
