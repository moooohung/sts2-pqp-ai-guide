use super::*;

#[test]
fn capacity_large_deck_selector_offers_and_removes_the_last_card() {
    let mut run = RunManager::new(42, RunOptions::default());
    run.unported = None;
    let card = run.run_ctx().deck[0].clone();
    let mut cards = Vec::new();
    for _ in 0..300 {
        let mut card = card.clone();
        card.instance_id = run.run_ctx_mut().new_card_id();
        cards.push(card);
    }
    run.run_ctx_mut().deck = cards;
    let removed = run.run_ctx().deck[299].instance_id;
    let candidates: Vec<_> = (0..300).collect();
    run.begin_run_card_choice("relic.preserved_fog.pickup", "Choose a card to remove",
        &candidates, 1, 1, false, RunChoiceEffect::Remove { damage_after: 0 },
        ChoiceResume::MapChoice);
    let actions = run.available_actions();
    assert_eq!(actions.len(), 300);
    let action = actions.into_iter()
        .find(|a| a.kind == RunActionKind::Choose && a.index == Some(299)).unwrap();
    run.take_action(action).unwrap();
    assert_eq!(run.unported, None);
    assert_eq!(run.run_ctx().deck.len(), 299);
    assert!(!run.run_ctx().deck.iter().any(|c| c.instance_id == removed));
    assert_eq!(run.phase(), RunPhase::MapChoice);
}

#[test]
fn fixed_count_deck_selector_completes_on_last_card_without_confirmation() {
    let mut run = RunManager::new(42, RunOptions::default());
    run.unported = None;
    let before = run.run_ctx().deck.len();
    run.begin_run_card_choice("relic.preserved_fog.pickup", "Choose 3 cards to remove",
        &[0, 1, 2, 3], 3, 3, false, RunChoiceEffect::Remove { damage_after: 0 },
        ChoiceResume::MapChoice);
    for index in 0..3 {
        let action = run.available_actions().into_iter()
            .find(|a| a.kind == RunActionKind::Choose && a.index == Some(index)).unwrap();
        run.take_action(action).unwrap();
    }
    assert_eq!(run.run_ctx().deck.len(), before - 3);
    assert_eq!(run.phase(), RunPhase::MapChoice);
}

