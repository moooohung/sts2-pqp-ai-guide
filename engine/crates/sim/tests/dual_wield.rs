use sts2_sim::{combat::Combat, rng::Rng, run::{RunManager, RunOptions}, tables::CardId};

#[test]
fn dual_wield_selects_only_owner_attacks_and_powers_and_generates_exact_copies() {
    for owner in 0..4 { for upgraded in [false, true] { for selected in 0..2 {
        let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
        run.deck.clear();
        let mut c = Combat::new(run, 1);
        for _ in 1..4 { c.add_player(50, 50, 3, 0, &[]); }
        c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
        c.start_combat();
        let play = c.new_card(owner, CardId::DualWield, upgraded).unwrap();
        let attack = c.new_card(owner, CardId::StrikeIronclad, true).unwrap();
        let power = c.new_card(owner, CardId::RuptureCard, false).unwrap();
        let skill = c.new_card(owner, CardId::DefendIronclad, false).unwrap();
        c.players[owner].hand.extend([play, attack, power, skill]);
        assert!(c.play_card(owner, 0, None));
        assert_eq!(c.choice_options_for_player(owner).unwrap(), &[attack, power]);
        assert!(c.resolve_pending_choice_for_player(owner, Some(selected)));
        let original = if selected == 0 { attack } else { power };
        let copies = if upgraded { 2 } else { 1 };
        assert_eq!(c.players[owner].hand.len(), 3 + copies);
        for &k in &c.players[owner].hand[3..] {
            assert_ne!(k, original);
            assert_eq!(c.cards[k as usize].id, c.cards[original as usize].id);
            assert_eq!(c.cards[k as usize].upgraded, c.cards[original as usize].upgraded);
        }
        for seat in 0..4 { if seat != owner { assert!(c.players[seat].hand.is_empty()); } }
        assert_eq!(c.hist.played_combat.len(), 1);
        assert_eq!(c.fault, None);
    }}}
}

#[test]
fn dual_wield_with_no_attack_or_power_finishes_without_a_selector() {
    let mut run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    run.deck.clear();
    let mut c = Combat::new(run, 1);
    c.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
    c.start_combat();
    let k = c.new_card(0, CardId::DualWield, false).unwrap();
    let skill = c.new_card(0, CardId::DefendIronclad, false).unwrap();
    c.players[0].hand.extend([k, skill]);
    assert!(c.play_card(0, 0, None));
    assert!(c.pending_choice.is_none());
    assert_eq!(c.players[0].hand.as_slice(), &[skill]);
    assert_eq!(c.fault, None);
}
