//! Exact reset-to-stall traces captured on main 238f8d2, character IRONCLAD.
//! Run: cargo test -p sts2_sim --release --test event_stall_repro -- --nocapture
//! Both traces now assert nonempty masks and the completed event outcomes.
//! RunOptions::default(): Live scheduling, all cards unlocked, ascension 10,
//! no HP cheat, Underdocks discovered, number_of_runs=0.
//! The original policy was lane_memory's seeded xorshift64. These tests replay
//! frozen action bindings instead: no PRNG, oracle, fixture, or outcome injection.
//! Each array entry is one action; its zero-based position is the decision index.
use sts2_sim::{
    run::{RunAction, RunActionKind, RunManager, RunOptions, RunPhase},
    tables::CardId,
};
use CardId::*;
use RunActionKind::*;
use Recorded::*;

#[derive(Debug)]
enum Recorded {
    Event(&'static str),
    MoveTo(i32, i32),
    Play(usize, CardId, Option<usize>),
    Indexed(RunActionKind, usize, Option<CardId>),
    Slot(RunActionKind, usize),
    Plain(RunActionKind),
}

impl Recorded {
    fn matches(&self, a: &RunAction) -> bool {
        match *self {
            Self::Event(option) => a.kind == EventChoice && a.option_id == Some(option),
            Self::MoveTo(col, row) => a.kind == Move
                && a.coord.is_some_and(|c| c.col as i32 == col && c.row as i32 == row),
            Self::Play(hand, card, target) => a.kind == PlayCard
                && a.hand_index == Some(hand) && a.card_id == Some(card) && a.target_index == target,
            Self::Indexed(kind, index, card) =>
                a.kind == kind && a.index == Some(index) && a.card_id == card,
            Self::Slot(kind, slot) => a.kind == kind && a.slot_index == Some(slot),
            Self::Plain(kind) => a.kind == kind,
        }
    }
}

fn reproduce(seed: u64, event: &str, trace: &[Recorded]) -> RunManager {
    // RunManager::new constructs Ironclad; do not infer character from card colors.
    let mut run = RunManager::new(seed, RunOptions::default());
    for (decision, recorded) in trace.iter().enumerate() {
        let actions = run.available_actions();
        let matches: Vec<_> = actions.iter().filter(|a| recorded.matches(a)).collect();
        assert_eq!(matches.len(), 1,
            "seed={seed} decision={decision} binding={recorded:?}; legal={actions:?}");
        let action = *matches[0];
        eprintln!("seed={seed} decision={decision} action={action:?}");
        run.take_action(action).unwrap_or_else(|reason| {
            panic!("seed={seed} decision={decision} action={recorded:?}: {reason}")
        });
    }
    let decision = trace.len();
    assert_eq!(run.phase(), RunPhase::Event, "seed={seed} decision={decision}");
    assert_eq!(run.current_event_id(), Some(event));
    let actions = run.available_actions();
    // Private event-local fields are diagnostic only: don't fossilize the bug by
    // asserting that options stay empty or that unported stays Some after a fix.
    let debug = format!("{run:#?}");
    let state = debug.split("    current_event:").nth(1).unwrap_or(&debug);
    eprintln!("seed={seed} character=IRONCLAD decision={decision} event={event} unported={:?}\ncurrent_event:{state}",
        run.unported);
    assert!(!actions.is_empty(),
        "nonterminal lane must offer a legal action: seed={seed} character=IRONCLAD decision={decision} event={event}; unported={:?}",
        run.unported);
    run
}


#[test]
fn seed_4100009_event_mask_is_nonempty() {
    // At decision 48, just after entering BRAIN_LEECH:
    // EventState { options: [], ints: [0; 8], flags: 0, cards: [], relics: [] };
    // pending_run_choice=None. Event RNG base_seed=seed=14926145472243032230,
    // counter=inner.counter=0, inner.s=[
    // 13112340419739722989, 4503019482569819572,
    // 3928330681139766475, 9827116734883576569].
    // unported="EVENT:BRAIN_LEECH r7 act1.py:37-90 CardReward flow".
    // C# Models/Events/BrainLeech.cs::GenerateInitialOptions should offer
    // ShareKnowledge (choose 1 of 5 character cards) or Rip (lose 5 HP,
    // then a 3-card colorless reward). Neither branch has been chosen yet.
    let mut run = reproduce(4100009, "BRAIN_LEECH", &[
        Event("positive_2"),
        Indexed(Choose, 9, Some(Bash)),
        MoveTo(6, 1),
        Play(4, DefendIronclad, None),
        Play(2, StrikeIronclad, Some(0)),
        Play(1, DefendIronclad, None),
        Plain(EndTurn),
        Play(1, DefendIronclad, None),
        Play(3, Bash, Some(0)),
        Plain(EndTurn),
        Play(2, DefendIronclad, None),
        Play(2, StrikeIronclad, Some(0)),
        Plain(EndTurn),
        Play(1, StrikeIronclad, Some(0)),
        Play(2, Bash, Some(0)),
        Plain(EndTurn),
        Play(1, DefendIronclad, None),
        Play(2, StrikeIronclad, Some(0)),
        Indexed(PickCard, 0, Some(Taunt)),
        MoveTo(6, 2),
        Indexed(BuyCard, 3, Some(Tremble)),
        Indexed(BuyPotion, 2, None),
        Slot(DiscardPotion, 0),
        Plain(LeaveShop),
        MoveTo(6, 3),
        Play(1, StrikeIronclad, Some(1)),
        Play(0, DefendIronclad, None),
        Play(1, DefendIronclad, None),
        Plain(EndTurn),
        Play(2, Taunt, Some(1)),
        Play(3, StrikeIronclad, Some(0)),
        Play(1, StrikeIronclad, Some(0)),
        Plain(EndTurn),
        Play(3, StrikeIronclad, Some(0)),
        Play(0, StrikeIronclad, Some(0)),
        Plain(EndTurn),
        Play(4, StrikeIronclad, Some(1)),
        Play(0, DefendIronclad, None),
        Play(1, DefendIronclad, None),
        Plain(EndTurn),
        Play(0, Taunt, Some(1)),
        Play(2, StrikeIronclad, Some(1)),
        Play(1, Tremble, Some(1)),
        Plain(EndTurn),
        Play(2, Bash, Some(0)),
        Play(2, StrikeIronclad, Some(0)),
        Indexed(PickCard, 1, Some(SwordBoomerang)),
        MoveTo(6, 4),
    ]);
    let hp = run.run_ctx().hp;
    let deck = run.run_ctx().deck.len();
    let rip = *run.available_actions().iter()
        .find(|a| a.kind == EventChoice && a.option_id == Some("rip")).unwrap();
    run.take_action(rip).unwrap();
    assert_eq!(run.run_ctx().hp, hp - 5);
    let options: Vec<_> = run.available_actions().into_iter().filter(|a| a.kind == PickCard).collect();
    assert_eq!(options.len(), 3);
    assert!(options.iter().all(|a| a.upgraded == Some(false)));
    run.take_action(options[0]).unwrap();
    assert_eq!(run.run_ctx().deck.len(), deck + 1);
    assert_eq!(run.phase(), RunPhase::MapChoice);
}
