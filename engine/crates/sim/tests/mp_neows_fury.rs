//! Original NeowsFury.OnPlay: optional owner discard selection, no shuffle.
use sts2_sim::{
    combat::{Combat, MAX_HAND_SIZE},
    rng::Rng,
    run::{RunManager, RunOptions},
    tables::CardId,
};

fn combat(seats: u8, owner: u8, upgraded: bool, hand_cards: usize) -> (Combat, u16, Vec<u16>) {
    let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    run.deck.clear();
    let mut c = Combat::new(run, 1);
    for _ in 1..seats { c.add_player(50, 50, 3, 0, &[]); }
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    c.start_combat();
    let enemy = c.enemies[0];
    c.cr_mut(enemy).hp = 200;
    c.cr_mut(enemy).max_hp = 200;
    let fury = c.new_card(owner, CardId::NeowsFury, upgraded).unwrap();
    c.players[owner].hand.push(fury);
    for _ in 0..hand_cards {
        let card = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        c.players[owner].hand.push(card);
    }
    let discarded: Vec<_> = [CardId::StrikeIronclad, CardId::Bash, CardId::DefendIronclad]
        .into_iter().map(|id| c.new_card(owner, id, false).unwrap()).collect();
    c.players[owner].discard.extend_from_slice(&discarded);
    (c, fury, discarded)
}

#[test]
fn original_fury_selection_is_optional_owner_bound_and_does_not_draw_rng() {
    for seats in [1, 4] { for owner in 0..seats { for upgraded in [false, true] {
        for accept in [false, true] {
            let (mut c, fury, discarded) = combat(seats, owner, upgraded, 0);
            let rng = c.run.rngs.combat_card_selection.clone();
            assert!(c.play_card(owner, 0, Some(0)));
            assert_eq!(c.cr(c.enemies[0]).hp, 200 - if upgraded { 14 } else { 10 });
            let choice = c.pending_choice_for_player(owner).unwrap();
            assert_eq!((choice.player, choice.min, choice.max), (owner, 0, if upgraded { 3 } else { 2 }));
            assert!(choice.is_multi() && choice.allow_skip);
            assert_eq!(c.choice_options_for_player(owner).unwrap(), discarded.as_slice());
            assert!(c.players[owner].play.contains(&fury));
            if accept { assert!(c.resolve_pending_choice_for_player(owner, Some(1))); }
            assert!(c.resolve_pending_choice_for_player(owner, None));
            assert_eq!(c.players[owner].hand.contains(&discarded[1]), accept);
            assert_eq!(c.players[owner].discard.contains(&discarded[1]), !accept);
            assert!(c.players[owner].exhaust.contains(&fury));
            assert!(c.pending_choice_for_player(owner).is_none());
            assert_eq!(c.run.rngs.combat_card_selection.counter, rng.counter);
            assert_eq!(c.run.rngs.combat_card_selection.inner.s, rng.inner.s);
            for other in 0..seats { if other != owner { assert!(c.players[other].hand.is_empty()); } }
            assert_eq!(c.fault, None);
        }
    }}}
}

#[test]
fn original_fury_caps_selection_at_hand_space_and_skips_empty_or_ending_combat() {
    for owner in 0..4 {
        let (mut c, _, _) = combat(4, owner, true, MAX_HAND_SIZE - 1);
        assert!(c.play_card(owner, 0, Some(0)));
        assert_eq!(c.pending_choice_for_player(owner).unwrap().max, 1);
        let (mut full, fury, _) = combat(4, owner, true, MAX_HAND_SIZE - 1);
        full.players[owner].hand.remove(0);
        let extra = full.new_card(owner, CardId::DefendIronclad, false).unwrap();
        full.players[owner].hand.push(extra);
        full.players[owner].draw.push(fury);
        assert!(full.auto_play_card(fury, Some(full.enemies[0]), false));
        assert!(full.pending_choice_for_player(owner).is_none());
        let (mut empty, _, _) = combat(4, owner, false, 0);
        empty.players[owner].discard.clear();
        assert!(empty.play_card(owner, 0, Some(0)));
        assert!(empty.pending_choice_for_player(owner).is_none());
        let (mut lethal, _, _) = combat(4, owner, false, 0);
        let enemy = lethal.enemies[0];
        lethal.cr_mut(enemy).hp = 1;
        assert!(lethal.play_card(owner, 0, Some(0)));
        assert!(lethal.pending_choice_for_player(owner).is_none());
        assert_eq!(lethal.fault, None);
    }
}

#[test]
fn original_fury_preserves_returned_selection_order_for_each_owner() {
    for owner in 0..4 { for upgraded in [false, true] {
        let (mut c, fury, discarded) = combat(4, owner, upgraded, 0);
        let rng = c.run.rngs.combat_card_selection.clone();
        assert!(c.play_card(owner, 0, Some(0)));
        assert!(c.resolve_pending_choice_for_player(owner, Some(2)));
        assert!(c.resolve_pending_choice_for_player(owner, Some(0)));
        assert!(c.resolve_pending_choice_for_player(owner, None));
        assert_eq!(c.players[owner].hand.as_slice(), &[discarded[2], discarded[0]]);
        assert_eq!(c.players[owner].discard.as_slice(), &[discarded[1]]);
        assert!(c.players[owner].exhaust.contains(&fury));
        assert_eq!(c.run.rngs.combat_card_selection, rng);
        assert_eq!(c.fault, None);
    }}
}
