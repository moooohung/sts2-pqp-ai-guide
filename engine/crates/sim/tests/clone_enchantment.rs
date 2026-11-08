use sts2_sim::{
    card::{enchantment_name, PORTED_ENCHANTMENTS},
    combat::Combat, rng::Rng, run::{RunManager, RunOptions}, tables::CardId,
};

// C# Clone.cs overrides nothing. EnchantmentModel's inherited combat defaults:
// additive damage/block 0, multiplicative damage/block 1, unchanged play count,
// completed OnPlay, empty OnEnchant/RecalculateValues. No cloning occurs in combat.
#[test]
fn clone_marker_preserves_combat_behavior_and_does_not_mark_card_as_combat_clone() {
    assert_eq!(enchantment_name("Clone"), Some(sts2_sim::identity::EnchantmentId::Clone));
    assert!(PORTED_ENCHANTMENTS.contains(&"Clone"));
    for id in [CardId::StrikeNecrobinder, CardId::DefendNecrobinder] {
        for upgraded in [false, true] {
            let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
            let mut plain = Combat::new(run, 1);
            plain.setup_encounter("SLUDGE_SPINNER_WEAK", &mut Rng::new(1));
            plain.players[0].hand.clear();
            plain.players[0].draw.clear();
            plain.players[0].energy = 3;
            let k = plain.new_card(0, id, upgraded).unwrap();
            plain.players[0].hand.push(k);
            let mut marked = plain.clone();
            marked.cards[k as usize].enchantments.push((sts2_sim::identity::EnchantmentId::Clone, 4));
            let target = (id == CardId::StrikeNecrobinder).then_some(0);
            assert!(plain.play_card(0, 0, target));
            assert!(marked.play_card(0, 0, target));
            assert_eq!(marked.digest_json(), plain.digest_json());
            assert_eq!(marked.cards.len(), plain.cards.len());
            assert!(!marked.cards[k as usize].cv.is_clone);
            assert_eq!(marked.cards[k as usize].keywords, plain.cards[k as usize].keywords);
            assert_eq!(marked.cards[k as usize].enchantments.as_slice(), &[(sts2_sim::identity::EnchantmentId::Clone, 4)]);
            plain.end_player_turn();
            marked.end_player_turn();
            assert_eq!(marked.digest_json(), plain.digest_json());
            assert_eq!(marked.fault, None);
        }
    }
}

#[test]
fn clone_marker_survives_upgrade_without_adding_replays_or_keywords() {
    let run = RunManager::new(1, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    let k = c.new_card(0, CardId::StrikeNecrobinder, false).unwrap();
    c.cards[k as usize].enchantments.push((sts2_sim::identity::EnchantmentId::Clone, 4));
    c.upgrade_card(k);
    assert_eq!(c.cards[k as usize].damage, Some(9));
    assert_eq!(c.cards[k as usize].replay, 0);
    assert_eq!(c.cards[k as usize].enchantments.as_slice(), &[(sts2_sim::identity::EnchantmentId::Clone, 4)]);
    assert!(!c.cards[k as usize].cv.is_clone);
    assert_eq!(c.fault, None);
}
