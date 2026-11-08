use super::*;
use crate::{orbs::OrbKind, run::{RunManager, RunOptions}};

#[test]
fn completed_orb_spans_stay_bounded_across_recorded_actions() {
    let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
    let mut combat = Combat::new(run, 7);
    combat.channel_orb(0, OrbKind::Lightning);
    let recorder = Recorder::default();
    recorder.record_combat(&mut combat);
    let orb = combat.players[0].orbs.orbs[0];
    for action in 0..1024 {
        let capture = recorder.begin(action);
        let (first, second);
        {
            let _activation = combat.record_orb_activation(0, orb, "evoke", false);
            let _hook = combat.orb_trace("after_orb_evoked", None);
            let context = combat.current_orb_context();
            first = combat.suspend_orb_context(context);
            second = combat.suspend_orb_context(context);
        }
        assert_eq!(first, second);
        assert_eq!(recorder.0.lock().unwrap().orb_pending.len(), 1);
        let transition = if action % 2 == 0 { "end" } else { "cancel" };
        combat.finish_orb_context(&first, transition);
        assert_eq!(recorder.0.lock().unwrap().orb_pending.len(), 1);
        assert!(!recorder.orb_traces().iter().any(|event|
            matches!(event.transition, "end" | "cancel")));
        combat.finish_orb_context(&second, "end");
        drop(capture);
        assert_eq!(recorder.orb_traces().iter().map(|event| event.transition).collect::<Vec<_>>(),
            ["begin", "suspend", transition]);
        let log = recorder.0.lock().unwrap();
        assert!(log.orb_pending.is_empty(), "finished spans accumulated at action {action}");
        assert!(log.orb_deferred_spans.is_empty());
        assert!(log.orb_canceled.is_empty());
    }
}
