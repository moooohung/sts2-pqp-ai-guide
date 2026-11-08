//! The rewards screen's card reward can be opened and closed without end, as in the
//! original game: C# CardReward.OnSelect returns false when the selection screen closes
//! without a card (choice index null), and NRewardButton.GetReward re-enables the button.
//! A greedy policy that prefers OpenReward on the container and Skip in the modal
//! therefore cycles forever (search_ab trace: every decision-capped A/B game).
use super::*;
use super::review_tests::manager;
use super::native_view::NativeRewardContent;

fn find(mgr: &mut RunManager, kind: RunActionKind) -> RunAction {
    *mgr.available_actions().iter().find(|a| a.kind == kind).unwrap_or_else(|| panic!("no {kind:?}"))
}
fn take(mgr: &mut RunManager, kind: RunActionKind) {
    let action = find(mgr, kind);
    mgr.take_action(action).unwrap();
}

#[test]
fn closing_a_card_reward_is_a_reopenable_no_op_and_only_proceeding_leaves() {
    let mut mgr = manager();
    mgr.phase = RunPhase::Combat;
    mgr.generate_combat_rewards(RoomType::Monster);
    assert_eq!((mgr.phase(), mgr.unported), (RunPhase::CardReward, None));
    // The card reward may open on its own after combat; close it to reach the container.
    if !mgr.available_actions().iter().any(|a| a.kind == RunActionKind::PickCard) {
        take(&mut mgr, RunActionKind::OpenReward);
    }
    take(&mut mgr, RunActionKind::Skip);
    let container = mgr.available_actions();
    let open = find(&mut mgr, RunActionKind::OpenReward);
    let deck = mgr.run_ctx().deck.len();
    let card_reward = |mgr: &RunManager| mgr.native_reward_indices().filter_map(|i| mgr.native_reward(i))
        .find_map(|r| match r.content { NativeRewardContent::Cards { closed, .. } => Some((closed, r.consumed)), _ => None });
    assert_eq!(card_reward(&mgr), Some((true, false)), "closed, not consumed");
    for _ in 0..3 {
        mgr.take_action(open).unwrap();
        assert!(mgr.available_actions().iter().any(|a| a.kind == RunActionKind::PickCard), "the same offer reopens");
        take(&mut mgr, RunActionKind::Skip);
        assert_eq!(mgr.available_actions(), container, "open + close returns to the identical container");
        assert_eq!((mgr.phase(), mgr.run_ctx().deck.len(), card_reward(&mgr)), (RunPhase::CardReward, deck, Some((true, false))));
    }
    take(&mut mgr, RunActionKind::SkipRewards);
    assert_ne!(mgr.phase(), RunPhase::CardReward, "proceeding is the only exit");
}
