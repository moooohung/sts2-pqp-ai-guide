//! C# history logs rolled moves, including an interrupted move and its rolled follow-up.
use sts2_sim::combat::Combat;
use sts2_sim::monster::{BranchCtx, STUNNED};
use sts2_sim::rng::Rng;
use sts2_sim::run::{RunManager, RunOptions};
use sts2_sim::tables::StateKind;

#[test]
fn injected_stun_does_not_break_move_history_or_cooldown_bookkeeping() {
    let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.setup_encounter("CORPSE_SLUGS_NORMAL", &mut Rng::new(7));
    let e = c.enemies[0];
    let ai = c.cr(e).ai.as_ref().unwrap();
    let selected = ai.current;
    assert_eq!(ai.log.as_slice(), &[selected]); // C# constructor
    assert!(c.stun_enemy(e));
    let ai = c.cr_mut(e).ai.as_mut().unwrap();
    ai.mark_performing(STUNNED);
    ai.on_move_performed(STUNNED);
    assert_eq!(ai.log.as_slice(), &[selected]);
    assert_ne!(ai.used & (1u64 << selected), 0);
    let ctx = BranchCtx { weight: &|_, _| 1.0, condition: &|_, targets| targets[0] };
    assert_eq!(ai.roll_move(&mut Rng::new(7), &ctx), selected);
    assert_eq!(ai.log.as_slice(), &[selected, selected]);
    ai.mark_performing(selected);
    ai.on_move_performed(selected);
    assert_eq!(ai.log.as_slice(), &[selected, selected]); // never append on perform
    assert!(!ai.log.contains(&STUNNED));
    // An already-performed canonical move and an already-performed injected
    // stun must not append another entry when replaced.
    assert!(c.stun_enemy(e));
    let ai = c.cr_mut(e).ai.as_mut().unwrap();
    ai.mark_performing(STUNNED);
    ai.on_move_performed(STUNNED);
    assert!(c.stun_enemy(e));
    let ai = c.cr(e).ai.as_ref().unwrap();
    assert_eq!(ai.log.as_slice(), &[selected, selected]);
    assert_eq!(ai.stun_follow_up, selected);
}

#[test]
fn flyconid_cooldown_counts_the_interrupted_move_and_its_rolled_follow_up() {
    let run = RunManager::new(7, RunOptions::default()).run_ctx().clone();
    let mut c = Combat::new(run, 1);
    c.setup_encounter("FLYCONID_NORMAL", &mut Rng::new(7));
    let e = *c.enemies.iter().find(|&&e| c.cr(e).monster_id == "FLYCONID").unwrap();
    let ai = c.cr_mut(e).ai.as_mut().unwrap();
    let find = |name: &str| ai.def().states.iter().position(|s| s.id == name).unwrap() as u8;
    let vuln = find("VULNERABLE_SPORES_MOVE");
    let smash = find("SMASH_MOVE");
    let frail = find("FRAIL_SPORES_MOVE");
    let branch = find("RAND");
    // Source-backed C# cooldown window: VULN, SMASH, selected FRAIL.
    ai.log.clear();
    ai.log.extend([vuln, smash, frail]);
    ai.used = (1 << vuln) | (1 << smash) | (1 << frail);
    ai.current = frail;
    ai.performed_first = true;
    assert!(c.stun_enemy(e));
    let ai = c.cr_mut(e).ai.as_mut().unwrap();
    ai.mark_performing(STUNNED);
    ai.on_move_performed(STUNNED);
    let ctx = BranchCtx { weight: &|_, _| 1.0, condition: &|_, targets| targets[0] };
    assert_eq!(ai.roll_move(&mut Rng::new(7), &ctx), frail);
    assert_eq!(ai.log.as_slice(), &[vuln, smash, frail, frail]);
    ai.mark_performing(frail);
    ai.on_move_performed(frail);
    // C# Flyconid RAND has equal weight 1 for all three moves. FRAIL is
    // suppressed (repeat/cooldown 2), but VULN is outside cooldown 3.
    assert!(matches!(ai.state(branch).kind, StateKind::Random { .. }));
    let mut saw_vuln = false;
    for seed in 0..16 {
        let mut trial = ai.clone();
        let expected = if Rng::new(seed).next_float(2.0) < 1.0 { vuln } else { smash };
        let result = trial.roll_move(&mut Rng::new(seed), &ctx);
        assert_eq!(result, expected);
        saw_vuln |= result == vuln;
    }
    assert!(saw_vuln, "VULN must be outside the last-three-moves cooldown window");
}
